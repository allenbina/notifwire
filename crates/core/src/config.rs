//! Versioned config schema and apply-if-newer semantics.
//!
//! Config sync is single-writer / many-readers by convention for v1: one
//! device is the source of truth, and the version stamp carried *inside* the
//! JSON is the guardrail. The same high-water-mark logic as the notification
//! cursor decides what to keep — a reader applies an incoming config only when
//! it is strictly newer, so a node that's been offline catches its config up
//! the same way a consumer catches up notifications.
//!
//! The envelope is modeled here; the typed `producers` / `focuses` payloads
//! land in D3, so for now they ride along as opaque JSON and round-trip intact.

use serde::{Deserialize, Serialize};

/// A synced configuration document, version-stamped in-band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Monotonic high-water mark. The primary freshness signal.
    pub config_version: u64,
    /// ISO 8601 timestamp of the edit. Tie-breaker when versions are equal.
    pub updated_at: String,
    /// Node ID that produced this revision.
    pub updated_by: String,
    /// Producer definitions. Typed in D3; opaque passthrough until then.
    #[serde(default)]
    pub producers: Vec<serde_json::Value>,
    /// Focus profiles. Typed in D3; opaque passthrough until then.
    #[serde(default)]
    pub focuses: Vec<serde_json::Value>,
}

/// Result of comparing an incoming config against the one a reader currently holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// The incoming config supersedes the current one and should be applied.
    Newer,
    /// The incoming config is the same revision or older; ignore it.
    SameOrOlder,
}

impl Config {
    /// Compare `self` (treated as the *incoming* config) against the config a
    /// reader `current`ly holds.
    ///
    /// Incoming wins when its `config_version` is greater; on a tie, the later
    /// `updated_at` wins. Versions are the real guardrail — `updated_at` only
    /// breaks ties and is compared lexicographically, which is correct for
    /// same-format UTC ISO 8601 stamps.
    pub fn freshness_vs(&self, current: &Config) -> Freshness {
        let newer = self.config_version > current.config_version
            || (self.config_version == current.config_version
                && self.updated_at > current.updated_at);
        if newer {
            Freshness::Newer
        } else {
            Freshness::SameOrOlder
        }
    }

    /// True iff `self` (incoming) is strictly newer than `current`.
    pub fn is_newer_than(&self, current: &Config) -> bool {
        self.freshness_vs(current) == Freshness::Newer
    }

    /// Replace `self` with `incoming` only if `incoming` is newer. Returns
    /// `true` if the config was applied, `false` if it was ignored as stale.
    pub fn apply_if_newer(&mut self, incoming: Config) -> bool {
        if incoming.is_newer_than(self) {
            *self = incoming;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(version: u64, updated_at: &str) -> Config {
        Config {
            config_version: version,
            updated_at: updated_at.into(),
            updated_by: "node-a".into(),
            producers: Vec::new(),
            focuses: Vec::new(),
        }
    }

    #[test]
    fn higher_version_is_newer() {
        let current = cfg(47, "2026-05-31T18:04:00Z");
        let incoming = cfg(48, "2026-05-31T18:00:00Z"); // older clock, higher version
        assert!(incoming.is_newer_than(&current));
    }

    #[test]
    fn lower_version_is_stale() {
        let current = cfg(48, "2026-05-31T18:00:00Z");
        let incoming = cfg(47, "2026-05-31T23:59:59Z"); // newer clock, lower version
        assert!(!incoming.is_newer_than(&current));
    }

    #[test]
    fn equal_version_breaks_tie_on_timestamp() {
        let current = cfg(47, "2026-05-31T18:04:00Z");
        let later = cfg(47, "2026-05-31T18:05:00Z");
        let same = cfg(47, "2026-05-31T18:04:00Z");
        assert!(later.is_newer_than(&current));
        assert!(!same.is_newer_than(&current)); // equal is not newer
    }

    #[test]
    fn apply_if_newer_applies_and_reports() {
        let mut current = cfg(47, "2026-05-31T18:04:00Z");
        let applied = current.apply_if_newer(cfg(48, "2026-05-31T18:10:00Z"));
        assert!(applied);
        assert_eq!(current.config_version, 48);

        let ignored = !current.apply_if_newer(cfg(10, "2020-01-01T00:00:00Z"));
        assert!(ignored);
        assert_eq!(current.config_version, 48); // unchanged
    }

    #[test]
    fn round_trips_with_opaque_payloads() {
        let json = r#"{
            "config_version": 47,
            "updated_at": "2026-05-31T18:04:00Z",
            "updated_by": "allen-macbook",
            "producers": [{"id": "mac1", "url": "https://mac.allenbina.uk"}],
            "focuses": [{"name": "Work"}]
        }"#;
        let parsed: Config = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.config_version, 47);
        assert_eq!(parsed.producers.len(), 1);
        assert_eq!(parsed.producers[0]["id"], "mac1");

        // Opaque payloads survive a round-trip unchanged.
        let reparsed: Config =
            serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn payload_lists_default_when_absent() {
        let json = r#"{
            "config_version": 1,
            "updated_at": "2026-05-31T00:00:00Z",
            "updated_by": "node-a"
        }"#;
        let parsed: Config = serde_json::from_str(json).unwrap();
        assert!(parsed.producers.is_empty());
        assert!(parsed.focuses.is_empty());
    }
}
