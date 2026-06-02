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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
}
