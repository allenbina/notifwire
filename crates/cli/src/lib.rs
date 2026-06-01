//! `notifwire-send` internals: build a normalized notification from simple
//! inputs and POST it to a node's localhost ingest endpoint.
//!
//! Kept as a library so the build step is unit-testable without a running node
//! and so the same path can be reused (e.g. by the E2E loopback test).

use anyhow::{Context, Result};
use notifwire_core::{Notification, Priority, SourcePlatform};

/// Inputs for constructing a notification from the command line.
#[derive(Debug, Clone)]
pub struct SendOpts {
    pub title: String,
    pub body: String,
    pub app: String,
    pub priority: Option<Priority>,
    pub icon: Option<String>,
    pub producer_node: String,
}

/// Build a normalized [`Notification`] from CLI inputs. The id is a fresh UUID
/// and the timestamp is now (RFC 3339, UTC); the source is the plugin channel,
/// since `notifwire-send` is a synthetic injector rather than OS capture.
pub fn build_notification(opts: &SendOpts) -> Notification {
    let mut n = Notification::new(
        uuid::Uuid::new_v4().to_string(),
        opts.producer_node.clone(),
        SourcePlatform::Plugin,
        opts.app.clone(),
        opts.title.clone(),
        opts.body.clone(),
        chrono::Utc::now().to_rfc3339(),
    );
    n.plugin_id = Some("notifwire-send".to_string());
    n.priority = opts.priority;
    n.app_icon = opts.icon.clone();
    n
}

/// POST a notification to a node's `/ingest` endpoint, returning the cursor the
/// node assigned it.
pub fn send(node_url: &str, notification: &Notification) -> Result<u64> {
    let url = format!("{}/ingest", node_url.trim_end_matches('/'));
    let resp = reqwest::blocking::Client::new()
        .post(&url)
        .header("content-type", "application/json")
        .body(serde_json::to_string(notification)?)
        .send()
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("ingest failed: HTTP {status}: {text}");
    }
    let v: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing ingest response: {text}"))?;
    v.get("seq")
        .and_then(serde_json::Value::as_u64)
        .context("ingest response missing seq")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> SendOpts {
        SendOpts {
            title: "Backup complete".into(),
            body: "42GB synced".into(),
            app: "rsync".into(),
            priority: Some(Priority::High),
            icon: Some("rsync".into()),
            producer_node: "thinkpad".into(),
        }
    }

    #[test]
    fn build_maps_inputs_onto_the_model() {
        let n = build_notification(&opts());
        assert_eq!(n.title, "Backup complete");
        assert_eq!(n.body, "42GB synced");
        assert_eq!(n.app_name, "rsync");
        assert_eq!(n.priority, Some(Priority::High));
        assert_eq!(n.app_icon.as_deref(), Some("rsync"));
        assert_eq!(n.source_platform, SourcePlatform::Plugin);
        assert_eq!(n.plugin_id.as_deref(), Some("notifwire-send"));
        assert_eq!(n.producer_node, "thinkpad");
        assert!(!n.id.is_empty(), "id should be a generated uuid");
        assert!(n.timestamp.contains('T'), "timestamp should be RFC 3339");
    }

    #[test]
    fn absent_priority_resolves_to_normal() {
        let mut o = opts();
        o.priority = None;
        let n = build_notification(&o);
        assert_eq!(n.priority, None);
        assert_eq!(n.priority(), Priority::Normal);
    }

    #[test]
    fn fresh_ids_each_call() {
        assert_ne!(
            build_notification(&opts()).id,
            build_notification(&opts()).id
        );
    }
}
