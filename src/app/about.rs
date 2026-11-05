//! Help > About ↔ ui/about.slint (`AboutState`): the version, and the project links opened in the browser. At each launch
//! a background thread asks GitHub for the newest release of this fork (pre-releases included, drafts are not public) and,
//! when it is newer than this build, `UpdateOverlay` offers it: UPDATE downloads it, checks it against the published SHA256
//! and replaces the app in place ([`super::update`]). Offline or any error: nothing is shown, or the error in the overlay.

use super::{library, update, AboutState, App};
use slint::ComponentHandle;
use std::sync::Mutex;
use std::time::Duration;

// The legacy repository, never the main one: its releases are arm64 and would be offered as an update this build cannot run.
const RELEASES: &str =
    "https://api.github.com/repos/ronellcl/MyOpenFadeR-Mac-Legacy/releases?per_page=10";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The legacy build number of this tree: tagged `v<version>-catalina.<n>`, so `n` counts the rebuilds of one upstream version
/// (the main repository does one release per version). Bump it with the tag; the release workflow refuses a tag that differs.
const BUILD: u64 = 4;

/// The release the overlay is offering, kept for its UPDATE button.
static FOUND: Mutex<Option<update::Release>> = Mutex::new(None);

/// This build, as the releases are tagged: `0.4.2-catalina.1`.
fn have() -> String {
    format!("{VERSION}-catalina.{BUILD}")
}

pub fn wire(app: &App) {
    let s = app.global::<AboutState>();
    s.set_version(have().into());
    s.on_open(|url| {
        let _ = library::open_external(&url);
    });
    s.on_install({
        let weak = app.as_weak();
        move || {
            let Some(release) = FOUND.lock().ok().and_then(|f| f.clone()) else {
                return;
            };
            let Some(app) = weak.upgrade() else {
                return;
            };
            let s = app.global::<AboutState>();
            if !s.get_update_status().is_empty() {
                return; // already installing
            }
            s.set_update_status("Starting…".into());
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let progress = {
                    let weak = weak.clone();
                    move |done: f32| {
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            app.global::<AboutState>().set_update_progress(done)
                        });
                    }
                };
                let say = {
                    let weak = weak.clone();
                    move |line: &str| {
                        let line = line.to_owned();
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            app.global::<AboutState>().set_update_status(line.into())
                        });
                    }
                };
                match update::install(&release, &progress, &say) {
                    Ok(()) => {
                        let _ = weak.upgrade_in_event_loop(|app| {
                            app.global::<AboutState>()
                                .set_update_status("Restarting…".into());
                            super::restart();
                        });
                    }
                    Err(e) => {
                        eprintln!("update: {e}");
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            app.global::<AboutState>().set_update_status(e.into())
                        });
                    }
                }
            });
        }
    });
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let Some(release) = latest() else {
            return;
        };
        *FOUND.lock().unwrap() = Some(release.clone());
        let _ = weak.upgrade_in_event_loop(move |app| {
            let s = app.global::<AboutState>();
            s.set_update_url(release.page.clone().into());
            s.set_update(release.version().into());
        });
    });
}

/// The newest release of this repository that is newer than this build and carries an archive for this platform.
fn latest() -> Option<update::Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let body = agent
        .get(RELEASES)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.as_array()?
        .iter()
        .filter_map(update::release)
        .find(|r| is_version(&r.tag) && newer(&r.tag, &have()))
}

/// `tag` looks like `v1.2.3` or `1.2.3` (a `-suffix` is ignored).
fn is_version(tag: &str) -> bool {
    let s = tag.trim_start_matches('v');
    s.split('-')
        .next()
        .unwrap_or(s)
        .split('.')
        .any(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// `tag` is a higher version than `have`.
///
/// The numbers decide first (`0.10.0` over `0.9.9`). With the same numbers, the legacy build number does: this tree publishes
/// `v0.4.2-catalina.1`, `-catalina.2` and so on, because a `-suffix` used to be ignored and every release of a version would
/// have looked the same. A plain `v0.4.2` counts as build 0.
fn newer(tag: &str, have: &str) -> bool {
    let parts = |s: &str| -> (Vec<u64>, u64) {
        let s = s.trim_start_matches('v');
        let (core, build) = match s.split_once("-catalina.") {
            Some((core, n)) => (core, n.parse().unwrap_or(0)),
            None => (s.split('-').next().unwrap_or(s), 0),
        };
        let nums = core.split('.').map(|n| n.parse().unwrap_or(0)).collect();
        (nums, build)
    };
    let (mut a, build_a) = parts(tag);
    let (mut b, build_b) = parts(have);
    a.resize(a.len().max(b.len()), 0);
    b.resize(a.len(), 0);
    (a, build_a) > (b, build_b)
}

#[test]
fn versions() {
    assert!(newer("v0.2.0", "0.1.0"));
    assert!(newer("v0.10.0", "0.9.9"));
    assert!(newer("1.0.0-beta", "0.9.0"));
    assert!(!newer("v0.1.0", "0.1.0"));
    assert!(!newer("v0.1.0", "0.2.0"));
}

#[test]
fn legacy_build_numbers() {
    assert!(newer("v0.4.2-catalina.2", "0.4.2-catalina.1"), "same numbers");
    assert!(newer("v0.4.2-catalina.1", "0.4.2"), "a plain release is build 0");
    assert!(!newer("v0.4.2-catalina.1", "0.4.2-catalina.1"));
    assert!(!newer("v0.4.2-catalina.1", "0.4.3-catalina.0"));
    assert!(newer("v0.5.0-catalina.0", "0.4.9-catalina.7"), "the numbers first");
}

#[test]
fn version_tags() {
    assert!(is_version("v0.4.1"));
    assert!(is_version("1.2.3"));
    assert!(is_version("v1.0.0-beta"));
    assert!(!is_version("models"));
    assert!(!is_version("v"));
    assert!(!is_version(""));
}
