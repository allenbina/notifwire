//! Health & status vocabulary shared across nodes.
//!
//! These are pure, serializable DTOs — no clock, no I/O. A producer fills in a
//! [`ProducerHealth`] (it owns the clock and the capture subsystem) and serves
//! it at `GET /health`; a consumer parses the same struct when probing each
//! producer it subscribes to. The [`HealthStatus`] tri-state maps directly to
//! the green / yellow / red status dots in the UI.
//!
//! JSON uses `snake_case` fields and `lowercase` enum variants, matching the
//! rest of the wire format.

use serde::{Deserialize, Serialize};

/// Overall health verdict for a node or one of its subsystems.
///
/// - [`Ok`](HealthStatus::Ok) 🟢 — fully operational.
/// - [`Degraded`](HealthStatus::Degraded) 🟡 — reachable / running, but
///   something is wrong (e.g. notification access not granted).
/// - [`Unhealthy`](HealthStatus::Unhealthy) 🔴 — not working.
///
/// Variants are ordered worst-last (`Ok < Degraded < Unhealthy`) so a roll-up
/// across subsystems is just the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    #[default]
    Ok,
    Degraded,
    Unhealthy,
}

/// Status of a producer's capture subsystem (the OS notification bridge).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CaptureHealth {
    /// Whether capture was requested for this node at all. When `false` the
    /// other fields are meaningless (the node is ingest-only).
    pub enabled: bool,
    /// Whether the capture worker is currently running.
    pub running: bool,
    /// OS notification-access permission, if known. `None` = not yet determined
    /// or not applicable.
    pub access_granted: Option<bool>,
    /// Human-readable detail (e.g. an error message) when degraded.
    pub detail: Option<String>,
}

impl CaptureHealth {
    /// Capture was not requested for this node (ingest-only producer).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Capture requested, worker running, access granted — the happy path.
    pub fn running() -> Self {
        Self {
            enabled: true,
            running: true,
            access_granted: Some(true),
            detail: None,
        }
    }

    /// Capture requested but not working; `detail` explains why.
    pub fn stopped(access_granted: Option<bool>, detail: impl Into<String>) -> Self {
        Self {
            enabled: true,
            running: false,
            access_granted,
            detail: Some(detail.into()),
        }
    }

    /// Roll this subsystem up into an overall status. A disabled subsystem can't
    /// be "wrong", so it reports [`Ok`](HealthStatus::Ok); an enabled one that
    /// isn't running (or was denied access) is [`Degraded`](HealthStatus::Degraded).
    pub fn status(&self) -> HealthStatus {
        if !self.enabled {
            return HealthStatus::Ok;
        }
        if self.running && self.access_granted != Some(false) {
            HealthStatus::Ok
        } else {
            HealthStatus::Degraded
        }
    }
}

/// A producer's self-reported health, served at `GET /health`.
///
/// Cheap for a consumer to poll: a missing response means "unreachable" 🔴,
/// while a `200` carrying a [`Degraded`](HealthStatus::Degraded) body means
/// "reachable but capture is broken" 🟡 — the distinction the consumer's
/// dashboard needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerHealth {
    /// Overall roll-up (currently driven by the capture subsystem).
    pub status: HealthStatus,
    /// Seconds since this producer started serving.
    pub uptime_secs: u64,
    /// Highest cursor the producer has assigned (`0` = nothing published yet).
    pub latest_cursor: u64,
    /// Notifications currently retained in the catch-up outbox.
    pub outbox_len: usize,
    /// Maximum the outbox will retain.
    pub outbox_capacity: usize,
    /// Capture subsystem status.
    pub capture: CaptureHealth,
}

/// A consumer's connection state to a single producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    /// Initial dial in progress; never connected yet.
    #[default]
    Connecting,
    /// Stream open and receiving.
    Connected,
    /// Was connected, lost the link, retrying with backoff.
    Reconnecting,
    /// Repeated connect attempts are failing.
    Unreachable,
}

/// A consumer's view of one producer it subscribes to — the per-producer status
/// dot in the UI, plus the producer's last self-report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerStatus {
    /// Producer base URL.
    pub url: String,
    /// Current connection state.
    pub state: ConnectionState,
    /// Unix ms of the last event received, if any.
    pub last_event_unix_ms: Option<i64>,
    /// Last connection/stream error, when the link is troubled.
    pub last_error: Option<String>,
    /// The producer's last successful `/health` self-report, if probed.
    pub health: Option<ProducerHealth>,
}

impl ProducerStatus {
    /// A freshly-created status for `url`, in the [`Connecting`](ConnectionState::Connecting) state.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            state: ConnectionState::Connecting,
            last_event_unix_ms: None,
            last_error: None,
            health: None,
        }
    }

    /// Green/yellow/red for this producer: unreachable is 🔴, (re)connecting is
    /// 🟡, and connected defers to the producer's own self-report (so a
    /// reachable-but-capture-broken producer shows 🟡, not 🟢).
    pub fn status(&self) -> HealthStatus {
        match self.state {
            ConnectionState::Connected => self
                .health
                .as_ref()
                .map(|h| h.status)
                .unwrap_or(HealthStatus::Ok),
            ConnectionState::Connecting | ConnectionState::Reconnecting => HealthStatus::Degraded,
            ConnectionState::Unreachable => HealthStatus::Unhealthy,
        }
    }
}

/// The consumer's own self-checks, independent of any producer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SelfChecks {
    /// History store is readable/writable.
    pub history_ok: bool,
    /// The receive pipeline is running.
    pub pipeline_alive: bool,
    /// Detail when something is off.
    pub detail: Option<String>,
}

impl SelfChecks {
    /// Roll the self-checks into a status: anything failing is [`Degraded`](HealthStatus::Degraded).
    pub fn status(&self) -> HealthStatus {
        if self.history_ok && self.pipeline_alive {
            HealthStatus::Ok
        } else {
            HealthStatus::Degraded
        }
    }
}

/// A consumer's composite health: its own self-checks plus a roll-up across
/// every producer it subscribes to. This is where the user looks — it drives
/// the overall status dot and the self-health view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerHealth {
    /// Overall roll-up (worst of self-checks and every producer).
    pub status: HealthStatus,
    /// The consumer's own self-checks.
    pub self_checks: SelfChecks,
    /// One entry per subscribed producer.
    pub producers: Vec<ProducerStatus>,
}

impl ConsumerHealth {
    /// Combine self-checks and per-producer statuses into one composite, where
    /// the overall status is the worst of all of them (worst-wins).
    pub fn rollup(self_checks: SelfChecks, producers: Vec<ProducerStatus>) -> Self {
        let worst = producers
            .iter()
            .map(|p| p.status())
            .fold(self_checks.status(), HealthStatus::max);
        Self {
            status: worst,
            self_checks,
            producers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_capture_is_ok_not_degraded() {
        assert_eq!(CaptureHealth::disabled().status(), HealthStatus::Ok);
    }

    #[test]
    fn running_capture_is_ok() {
        assert_eq!(CaptureHealth::running().status(), HealthStatus::Ok);
    }

    #[test]
    fn enabled_but_stopped_capture_is_degraded() {
        let h = CaptureHealth::stopped(Some(false), "access not granted");
        assert_eq!(h.status(), HealthStatus::Degraded);
    }

    #[test]
    fn health_status_serializes_lowercase() {
        let json = serde_json::to_string(&HealthStatus::Degraded).unwrap();
        assert_eq!(json, "\"degraded\"");
    }

    #[test]
    fn producer_health_round_trips() {
        let h = ProducerHealth {
            status: HealthStatus::Ok,
            uptime_secs: 42,
            latest_cursor: 7,
            outbox_len: 3,
            outbox_capacity: 1000,
            capture: CaptureHealth::running(),
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: ProducerHealth = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn health_status_orders_worst_last() {
        assert!(HealthStatus::Ok < HealthStatus::Degraded);
        assert!(HealthStatus::Degraded < HealthStatus::Unhealthy);
        assert_eq!(
            HealthStatus::Ok.max(HealthStatus::Unhealthy),
            HealthStatus::Unhealthy
        );
    }

    #[test]
    fn unreachable_producer_is_red_reconnecting_is_yellow() {
        let mut p = ProducerStatus::new("http://x");
        p.state = ConnectionState::Unreachable;
        assert_eq!(p.status(), HealthStatus::Unhealthy);
        p.state = ConnectionState::Reconnecting;
        assert_eq!(p.status(), HealthStatus::Degraded);
    }

    #[test]
    fn connected_producer_defers_to_its_self_report() {
        let mut p = ProducerStatus::new("http://x");
        p.state = ConnectionState::Connected;
        // No probe yet → assume ok.
        assert_eq!(p.status(), HealthStatus::Ok);
        // Reachable but capture broken → yellow, not green.
        p.health = Some(ProducerHealth {
            status: HealthStatus::Degraded,
            uptime_secs: 1,
            latest_cursor: 0,
            outbox_len: 0,
            outbox_capacity: 10,
            capture: CaptureHealth::stopped(Some(false), "no access"),
        });
        assert_eq!(p.status(), HealthStatus::Degraded);
    }

    #[test]
    fn consumer_rollup_takes_the_worst() {
        let healthy_self = SelfChecks {
            history_ok: true,
            pipeline_alive: true,
            detail: None,
        };
        let mut bad = ProducerStatus::new("http://down");
        bad.state = ConnectionState::Unreachable;
        let mut good = ProducerStatus::new("http://up");
        good.state = ConnectionState::Connected;

        let h = ConsumerHealth::rollup(healthy_self.clone(), vec![good.clone(), bad]);
        assert_eq!(h.status, HealthStatus::Unhealthy); // one unreachable drags it red

        let h = ConsumerHealth::rollup(healthy_self, vec![good]);
        assert_eq!(h.status, HealthStatus::Ok); // all good
    }
}
