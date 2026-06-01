//! The consumer-side rules engine.
//!
//! Given a received [`Notification`] and a [`Rules`] set, decide whether to
//! show it. This is the OS-independent core that the focuses tree (D3)
//! configures: each app has an on/off checkmark, a default mode governs
//! apps not yet configured (so a newly-discovered app behaves predictably),
//! and text [`Filter`]s allow/block by keyword.
//!
//! Evaluation order: app gate → block filters (any match suppresses) → allow
//! filters (if any exist, at least one must match).

use crate::Notification;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How apps that aren't explicitly configured are treated — i.e. whitelist
/// (`Block` by default) vs blacklist (`Allow` by default) behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultMode {
    /// Show notifications from apps not in the list (blacklist-style). The
    /// default: show by default, hide what you opt out of.
    #[default]
    Allow,
    /// Hide notifications from apps not in the list (whitelist-style).
    Block,
}

/// Which part of a notification a [`Filter`] matches against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchField {
    Title,
    Body,
    AppName,
    /// Title, body, or app name.
    Any,
}

/// What a matching [`Filter`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterAction {
    /// A notification must match at least one `Allow` filter (when any exist).
    Allow,
    /// A matching notification is suppressed.
    Block,
}

/// A case-insensitive substring filter on one field of a notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub field: MatchField,
    /// Substring to look for (matched case-insensitively).
    pub contains: String,
    pub action: FilterAction,
}

impl Filter {
    fn matches(&self, n: &Notification) -> bool {
        let needle = self.contains.to_lowercase();
        if needle.is_empty() {
            return false;
        }
        let hit = |s: &str| s.to_lowercase().contains(&needle);
        match self.field {
            MatchField::Title => hit(&n.title),
            MatchField::Body => hit(&n.body),
            MatchField::AppName => hit(&n.app_name),
            MatchField::Any => hit(&n.title) || hit(&n.body) || hit(&n.app_name),
        }
    }
}

/// The decision for a single notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Show,
    Suppress,
}

/// A set of rules: the default mode, per-app on/off toggles, and text filters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub default_mode: DefaultMode,
    /// Per-app checkmark: `true` = shown, `false` = hidden. Apps not present
    /// fall back to `default_mode`.
    #[serde(default)]
    pub apps: BTreeMap<String, bool>,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

impl Rules {
    /// Decide whether a notification should be shown.
    pub fn evaluate(&self, n: &Notification) -> Verdict {
        // 1. App gate — explicit checkmark, else the default mode.
        let app_allowed = self
            .apps
            .get(&n.app_name)
            .copied()
            .unwrap_or(self.default_mode == DefaultMode::Allow);
        if !app_allowed {
            return Verdict::Suppress;
        }

        // 2. Block filters — any match suppresses.
        if self
            .filters
            .iter()
            .any(|f| f.action == FilterAction::Block && f.matches(n))
        {
            return Verdict::Suppress;
        }

        // 3. Allow filters — if any exist, at least one must match.
        let mut allows = self
            .filters
            .iter()
            .filter(|f| f.action == FilterAction::Allow);
        if let Some(first) = allows.next() {
            let any_match = first.matches(n) || allows.any(|f| f.matches(n));
            if !any_match {
                return Verdict::Suppress;
            }
        }

        Verdict::Show
    }

    /// Convenience: `true` if the notification should be shown.
    pub fn allows(&self, n: &Notification) -> bool {
        self.evaluate(n) == Verdict::Show
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePlatform;

    fn note(app: &str, title: &str, body: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Windows,
            app,
            title,
            body,
            "2026-06-01T00:00:00Z",
        )
    }

    fn filter(field: MatchField, contains: &str, action: FilterAction) -> Filter {
        Filter {
            field,
            contains: contains.into(),
            action,
        }
    }

    #[test]
    fn default_allow_shows_unconfigured_apps() {
        let rules = Rules::default();
        assert_eq!(rules.evaluate(&note("Slack", "hi", "")), Verdict::Show);
    }

    #[test]
    fn default_block_hides_unconfigured_apps() {
        let rules = Rules {
            default_mode: DefaultMode::Block,
            ..Default::default()
        };
        assert_eq!(rules.evaluate(&note("Slack", "hi", "")), Verdict::Suppress);
    }

    #[test]
    fn app_checkmark_overrides_default() {
        let mut on_in_block = Rules {
            default_mode: DefaultMode::Block,
            ..Default::default()
        };
        on_in_block.apps.insert("Slack".into(), true);
        assert!(on_in_block.allows(&note("Slack", "hi", "")));
        assert!(!on_in_block.allows(&note("Spam", "hi", ""))); // unconfigured → blocked

        let mut off_in_allow = Rules::default(); // default Allow
        off_in_allow.apps.insert("Spam".into(), false);
        assert!(!off_in_allow.allows(&note("Spam", "hi", "")));
        assert!(off_in_allow.allows(&note("Slack", "hi", "")));
    }

    #[test]
    fn block_filter_suppresses_match() {
        let rules = Rules {
            filters: vec![filter(MatchField::Title, "OTP", FilterAction::Block)],
            ..Default::default()
        };
        assert_eq!(
            rules.evaluate(&note("Bank", "Your OTP is 123", "")),
            Verdict::Suppress
        );
        assert_eq!(rules.evaluate(&note("Bank", "Balance", "")), Verdict::Show);
    }

    #[test]
    fn allow_filters_require_a_match() {
        // "notify only if title contains 'deploy'"
        let rules = Rules {
            filters: vec![filter(MatchField::Title, "deploy", FilterAction::Allow)],
            ..Default::default()
        };
        assert_eq!(rules.evaluate(&note("CI", "deploy ok", "")), Verdict::Show);
        assert_eq!(
            rules.evaluate(&note("CI", "test ok", "")),
            Verdict::Suppress
        );
    }

    #[test]
    fn matching_is_case_insensitive_and_field_scoped() {
        let body_filter = Rules {
            filters: vec![filter(MatchField::Body, "urgent", FilterAction::Block)],
            ..Default::default()
        };
        // Matches in body regardless of case...
        assert_eq!(
            body_filter.evaluate(&note("X", "hi", "URGENT now")),
            Verdict::Suppress
        );
        // ...but the same word in the title is out of scope for a Body filter.
        assert_eq!(
            body_filter.evaluate(&note("X", "URGENT", "calm")),
            Verdict::Show
        );
    }

    #[test]
    fn block_wins_over_allow() {
        let rules = Rules {
            filters: vec![
                filter(MatchField::Any, "ok", FilterAction::Allow),
                filter(MatchField::Title, "secret", FilterAction::Block),
            ],
            ..Default::default()
        };
        // Matches the allow ("ok") but also the block ("secret") → suppressed.
        assert_eq!(
            rules.evaluate(&note("X", "secret ok", "")),
            Verdict::Suppress
        );
    }

    #[test]
    fn round_trips_through_json() {
        let mut rules = Rules {
            default_mode: DefaultMode::Block,
            filters: vec![filter(MatchField::Any, "x", FilterAction::Allow)],
            ..Default::default()
        };
        rules.apps.insert("Slack".into(), true);
        let back: Rules = serde_json::from_str(&serde_json::to_string(&rules).unwrap()).unwrap();
        assert_eq!(rules, back);
    }
}
