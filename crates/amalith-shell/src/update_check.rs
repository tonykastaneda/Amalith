//! One-shot, background check against GitHub's releases API for a newer
//! Amalith build than this one. See `App::drain_update_check` for how the
//! result reaches the UI, and `crate::update_banner` for the corner card
//! itself.

use std::sync::mpsc::{self, Receiver};

/// Spawns the check on its own thread and returns immediately; the
/// channel yields exactly one value, and only when a strictly newer
/// version is actually found (never on a network failure, and never when
/// already up to date), so the common case has nothing for
/// `App::drain_update_check` to do.
pub fn spawn() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Some(latest) = fetch_latest_release_version() {
            if is_newer_version(&latest, crate::version::VERSION) {
                let _ = tx.send(latest);
            }
        }
    });
    rx
}

fn fetch_latest_release_version() -> Option<String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(8)))
        .build();
    let agent: ureq::Agent = config.into();
    // GitHub's singular "latest release" endpoint explicitly skips
    // pre-releases, and every Amalith release published so far is one —
    // so this hits the list endpoint (newest first) and takes the first
    // non-draft entry instead.
    let releases: serde_json::Value = agent
        .get("https://api.github.com/repos/tonykastaneda/Amalith/releases")
        // GitHub's API rejects requests with no User-Agent at all.
        .header("User-Agent", "Amalith-UpdateCheck")
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    let latest = releases
        .as_array()?
        .iter()
        .find(|release| !release.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))?;
    let tag = latest.get("tag_name")?.as_str()?;
    Some(tag.trim_start_matches('v').to_string())
}

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// `false` on anything unparsable, not just "not newer" — an unrecognized
/// version format (a stray non-numeric tag, say) should never show an
/// update banner for something that might not even be a real newer build.
fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer_version;

    #[test]
    fn newer_patch_and_minor_and_major_are_detected() {
        assert!(is_newer_version("0.1.1", "0.1.0"));
        assert!(is_newer_version("0.2.0", "0.1.9"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
    }

    #[test]
    fn equal_or_older_is_not_newer() {
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.1"));
    }

    #[test]
    fn unparsable_versions_never_trigger_an_update() {
        assert!(!is_newer_version("not-a-version", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "not-a-version"));
        assert!(!is_newer_version("v1.2", "0.1.0"));
    }
}
