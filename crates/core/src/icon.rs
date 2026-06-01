//! Icon resolution chain.
//!
//! An app's icon can come from several sources, tried in priority order until
//! one resolves; the last step is a generated fallback that always succeeds.
//! This module owns the OS-independent parts: classifying an icon hint
//! (`--icon` accepts a file path, URL, domain, or brand/app name) and building
//! the ordered [`IconStep`] chain for a notification. Actually fetching,
//! caching, and rendering to a 48x48 PNG is consumer/platform I/O (later).

use crate::Notification;

/// What an icon reference string denotes, used to route it into the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconRef {
    /// A local file path.
    FilePath(String),
    /// A full URL to an image.
    Url(String),
    /// A bare domain → favicon lookup.
    Domain(String),
    /// A brand/app name → brand-icon lookup.
    Name(String),
}

/// One step in the resolution chain. The consumer tries each in order and uses
/// the first that yields an icon; [`Fallback`](IconStep::Fallback) is terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconStep {
    /// Image data carried inline on the notification.
    Embedded,
    /// Load from a local file path.
    File(String),
    /// Download from an image URL.
    Url(String),
    /// Fetch the favicon for a domain.
    Favicon(String),
    /// Look up a brand icon by name.
    Brand(String),
    /// Use a previously cached icon for this app.
    CachedApp(String),
    /// Generate a placeholder from the app name (initials). Always succeeds.
    Fallback(String),
}

/// Classify an icon hint string into an [`IconRef`]. Never fails — anything
/// that isn't clearly a path/URL/domain is treated as a brand/app name.
pub fn classify(reference: &str) -> IconRef {
    let r = reference.trim();
    if r.starts_with("http://") || r.starts_with("https://") {
        IconRef::Url(r.to_string())
    } else if let Some(path) = r.strip_prefix("file://") {
        IconRef::FilePath(path.to_string())
    } else if is_file_path(r) {
        IconRef::FilePath(r.to_string())
    } else if is_domain(r) {
        IconRef::Domain(r.to_string())
    } else {
        IconRef::Name(r.to_string())
    }
}

fn is_file_path(r: &str) -> bool {
    r.contains('/')
        || r.contains('\\')
        || r.starts_with('.')
        || r.starts_with('~')
        // Windows drive prefix like `C:`.
        || (r.len() >= 2 && r.as_bytes()[1] == b':' && r.as_bytes()[0].is_ascii_alphabetic())
}

fn is_domain(r: &str) -> bool {
    if r.is_empty() || r.contains(' ') || !r.contains('.') {
        return false;
    }
    let tld = r.rsplit('.').next().unwrap_or("");
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// Build the ordered icon-resolution chain for a notification: an explicit
/// producer hint wins, then a cached app icon, then a brand lookup, and finally
/// the always-succeeds generated fallback.
pub fn icon_chain(n: &Notification) -> Vec<IconStep> {
    let mut steps = Vec::new();

    // 1. Inline image carried with the notification.
    if n.image.is_some() {
        steps.push(IconStep::Embedded);
    }

    // 2. Explicit icon hint from the producer (most authoritative).
    if let Some(hint) = n
        .app_icon
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        steps.push(match classify(hint) {
            IconRef::FilePath(p) => IconStep::File(p),
            IconRef::Url(u) => IconStep::Url(u),
            IconRef::Domain(d) => IconStep::Favicon(d),
            IconRef::Name(name) => IconStep::Brand(name),
        });
    }

    // 3. Cached icon for this app, 4. brand lookup by app name.
    steps.push(IconStep::CachedApp(n.app_name.clone()));
    steps.push(IconStep::Brand(n.app_name.clone()));

    // 5. Generated fallback — terminal, never fails.
    steps.push(IconStep::Fallback(n.app_name.clone()));

    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourcePlatform;

    fn note(app: &str) -> Notification {
        Notification::new(
            "id",
            "node",
            SourcePlatform::Windows,
            app,
            "title",
            "body",
            "2026-06-01T00:00:00Z",
        )
    }

    #[test]
    fn classify_routes_each_kind() {
        assert_eq!(
            classify("https://x.com/a.png"),
            IconRef::Url("https://x.com/a.png".into())
        );
        assert_eq!(
            classify("file:///c/icon.png"),
            IconRef::FilePath("/c/icon.png".into())
        );
        assert_eq!(
            classify("./icon.png"),
            IconRef::FilePath("./icon.png".into())
        );
        assert_eq!(
            classify(r"C:\icons\a.ico"),
            IconRef::FilePath(r"C:\icons\a.ico".into())
        );
        assert_eq!(classify("github.com"), IconRef::Domain("github.com".into()));
        assert_eq!(classify("Slack"), IconRef::Name("Slack".into()));
        // A name with a space is never a domain.
        assert_eq!(
            classify("HP System Event"),
            IconRef::Name("HP System Event".into())
        );
    }

    #[test]
    fn chain_ends_in_fallback() {
        let chain = icon_chain(&note("Slack"));
        assert_eq!(chain.last(), Some(&IconStep::Fallback("Slack".into())));
    }

    #[test]
    fn no_hint_chain_is_cache_brand_fallback() {
        assert_eq!(
            icon_chain(&note("Teams")),
            vec![
                IconStep::CachedApp("Teams".into()),
                IconStep::Brand("Teams".into()),
                IconStep::Fallback("Teams".into()),
            ]
        );
    }

    #[test]
    fn explicit_hint_takes_priority() {
        let mut n = note("rsync");
        n.app_icon = Some("github.com".into());
        let chain = icon_chain(&n);
        assert_eq!(chain[0], IconStep::Favicon("github.com".into()));
        assert_eq!(chain.last(), Some(&IconStep::Fallback("rsync".into())));
    }

    #[test]
    fn embedded_image_comes_first() {
        let mut n = note("Photos");
        n.image = Some("data:...".into());
        n.app_icon = Some("Photos".into());
        let chain = icon_chain(&n);
        assert_eq!(chain[0], IconStep::Embedded);
        assert_eq!(chain[1], IconStep::Brand("Photos".into())); // name hint → brand
    }

    #[test]
    fn blank_hint_is_ignored() {
        let mut n = note("App");
        n.app_icon = Some("   ".into());
        let chain = icon_chain(&n);
        assert_eq!(chain[0], IconStep::CachedApp("App".into()));
    }
}
