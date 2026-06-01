//! The normalized notification data model.
//!
//! Every notification — whether captured from an OS or injected by an input
//! plugin / `notifwire-send` — is normalized to this common schema before it
//! enters the mesh. The schema mirrors the canonical spec's "Notification Data
//! Model": a lowest-common-denominator core guaranteed on every platform, plus
//! optional extended fields carried when the source provides them and rendered
//! only if the destination supports them.
//!
//! JSON uses `snake_case` field names (serde's default), matching the spec and
//! the `data:` payloads carried over the transport.

use serde::{Deserialize, Serialize};

/// Where a notification originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourcePlatform {
    Macos,
    Windows,
    Linux,
    Android,
    /// Injected by an input plugin or `notifwire-send` (see `plugin_id`).
    Plugin,
}

/// Delivery priority. Maps to each platform's nearest native equivalent at the
/// consumer; absent means treat as [`Priority::Normal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

/// A single normalized notification.
///
/// The first block is the lowest-common-denominator schema guaranteed on every
/// platform; the rest are extended fields carried only when available. Optional
/// fields are omitted from the wire form when unset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    // --- Lowest common denominator (guaranteed) ---
    /// UUID generated at capture.
    pub id: String,
    /// Node ID that captured it.
    pub producer_node: String,
    /// Originating platform.
    pub source_platform: SourcePlatform,
    /// App that fired the notification. Always present.
    pub app_name: String,
    /// Always present.
    pub title: String,
    /// Always present.
    pub body: String,
    /// ISO 8601, captured at receive time. Kept as a string at this layer; the
    /// catch-up cursor is the transport sequence id, not this timestamp.
    pub timestamp: String,

    // --- Extended fields (carried if available) ---
    /// Set when [`source_platform`](Self::source_platform) is
    /// [`SourcePlatform::Plugin`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Icon reference; normalized to a 48x48 PNG at the consumer (see Icon System).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_icon: Option<String>,
    /// Middle line between title and body (macOS, iOS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Inline or hero image (Android, Windows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// App-defined notification category (macOS, Android).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Grouping key for related notifications (macOS, Android).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Source icon resolution before normalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_resolution: Option<String>,
    /// Delivery priority; `None` is treated as [`Priority::Normal`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// True for battery events, plugin-generated notifications, etc.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_synthetic: bool,
}

impl Notification {
    /// Construct a notification from the guaranteed fields, leaving every
    /// extended field unset. Builder-ish setters can fill the rest.
    pub fn new(
        id: impl Into<String>,
        producer_node: impl Into<String>,
        source_platform: SourcePlatform,
        app_name: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            producer_node: producer_node.into(),
            source_platform,
            app_name: app_name.into(),
            title: title.into(),
            body: body.into(),
            timestamp: timestamp.into(),
            plugin_id: None,
            app_icon: None,
            subtitle: None,
            image: None,
            category: None,
            thread_id: None,
            icon_resolution: None,
            priority: None,
            is_synthetic: false,
        }
    }

    /// Effective priority, resolving an absent value to [`Priority::Normal`].
    pub fn priority(&self) -> Priority {
        self.priority.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Notification {
        Notification::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "thinkpad-win",
            SourcePlatform::Windows,
            "rsync",
            "Backup complete",
            "42GB synced in 4m 12s",
            "2026-05-31T12:00:00Z",
        )
    }

    #[test]
    fn full_round_trip() {
        let mut n = sample();
        n.priority = Some(Priority::High);
        n.subtitle = Some("nightly".into());
        n.is_synthetic = true;

        let json = serde_json::to_string(&n).unwrap();
        let back: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(n, back);
    }

    #[test]
    fn minimal_deserializes_with_defaults() {
        // Only the lowest-common-denominator fields present.
        let json = r#"{
            "id": "abc",
            "producer_node": "node1",
            "source_platform": "linux",
            "app_name": "weechat",
            "title": "ping",
            "body": "hi",
            "timestamp": "2026-05-31T00:00:00Z"
        }"#;
        let n: Notification = serde_json::from_str(json).unwrap();
        assert_eq!(n.app_name, "weechat");
        assert_eq!(n.priority, None);
        assert_eq!(n.priority(), Priority::Normal); // absent → Normal
        assert!(!n.is_synthetic);
        assert_eq!(n.subtitle, None);
    }

    #[test]
    fn unset_optionals_are_omitted_from_wire() {
        let json = serde_json::to_value(sample()).unwrap();
        let obj = json.as_object().unwrap();
        // Guaranteed fields are present...
        assert!(obj.contains_key("app_name"));
        // ...optional ones are absent rather than null.
        for absent in ["subtitle", "image", "priority", "plugin_id", "is_synthetic"] {
            assert!(
                !obj.contains_key(absent),
                "{absent} should be omitted when unset"
            );
        }
    }

    #[test]
    fn enums_serialize_lowercase() {
        assert_eq!(
            serde_json::to_string(&SourcePlatform::Macos).unwrap(),
            "\"macos\""
        );
        assert_eq!(
            serde_json::to_string(&Priority::Urgent).unwrap(),
            "\"urgent\""
        );
    }

    #[test]
    fn plugin_source_carries_plugin_id() {
        let mut n = sample();
        n.source_platform = SourcePlatform::Plugin;
        n.plugin_id = Some("rss".into());
        let json = serde_json::to_value(&n).unwrap();
        assert_eq!(json["source_platform"], "plugin");
        assert_eq!(json["plugin_id"], "rss");
    }

    #[test]
    fn default_priority_is_normal() {
        assert_eq!(Priority::default(), Priority::Normal);
    }
}
