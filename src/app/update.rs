//! One-click update (`UpdateOverlay`, [`super::about`]): the app downloads the release archive itself, checks it against the
//! SHA256 that GitHub publishes for the asset, unpacks it beside the running bundle and swaps the bundle in place.
//!
//! Nothing is quarantined: `com.apple.quarantine` comes from the browser that saved the file, and here the bytes are written
//! by this process. Only `MyOpenFadeR.app` is replaced; `~/Library/Application Support/openfader`, the model cache and every
//! user file stay where they are. Any failure before the swap leaves the running app untouched, and a failure after it puts
//! the old bundle back.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An installable release: the archive, and the size and SHA256 that GitHub reports for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub page: String,
    pub asset: String,
    pub size: u64,
    pub sha256: String,
}

impl Release {
    /// `0.5.0-catalina.3`: the tag without its `v`.
    pub fn version(&self) -> &str {
        self.tag.trim_start_matches('v')
    }
}

/// The installable asset of one release, out of GitHub's JSON (`GET /releases`): the `…-macos-catalina-x86_64.zip`, with
/// the `sha256:…` digest that the API reports for it. A release without that asset or without a digest is not offered.
pub fn release(r: &serde_json::Value) -> Option<Release> {
    let tag = r["tag_name"].as_str()?.to_owned();
    let page = r["html_url"].as_str()?.to_owned();
    let a = r["assets"].as_array()?.iter().find(|a| {
        a["name"]
            .as_str()
            .is_some_and(|n| n.starts_with("MyOpenFadeR-") && n.ends_with("-macos-catalina-x86_64.zip"))
    })?;
    let asset = a["browser_download_url"].as_str()?.to_owned();
    let sha256 = a["digest"].as_str()?.strip_prefix("sha256:")?.to_owned();
    let size = a["size"].as_u64().unwrap_or(0);
    Some(Release { tag, page, asset, size, sha256 })
}

/// Downloads `r`, verifies it, and replaces the running `.app` with it. `progress` gets 0 to 1 while the bytes arrive and
/// `say` a line for each step; the caller restarts the app once this returns `Ok`.
pub fn install(r: &Release, progress: &dyn Fn(f32), say: &dyn Fn(&str)) -> Result<(), String> {
    let app = bundle()?;
    let (scratch, admin) = scratch(&app)?;
    let clean = || {
        let _ = std::fs::remove_dir_all(&scratch);
    };
    say("Downloading…");
    let archive = scratch.join("update.zip");
    if let Err(e) = download(r, &archive, progress) {
        clean();
        return Err(e);
    }
    say("Checking the download…");
    match sha256(&archive) {
        Ok(got) if got.eq_ignore_ascii_case(&r.sha256) => {}
        Ok(got) => {
            clean();
            return Err(format!(
                "the download does not match the published SHA256 ({got} instead of {}); nothing was changed",
                r.sha256
            ));
        }
        Err(e) => {
            clean();
            return Err(e);
        }
    }
    say("Unpacking…");
    let new = scratch.join("MyOpenFadeR.app");
    if let Err(e) = unpack(&archive, &new) {
        clean();
        return Err(e);
    }
    say("Installing…");
    if let Err(e) = swap(&app, &new, admin) {
        clean();
        return Err(e);
    }
    // The old bundle is `scratch/old.app` now: removing the scratch folder takes it away, after this process is done with
    // it (macOS keeps the running program's pages, so deleting it under our feet is fine).
    clean();
    Ok(())
}

/// Where the archive is unpacked, and whether installing will need the administrator prompt.
///
/// Beside the app, so that the swap is the rename of two folders on one volume. When that folder belongs to root
/// (`/Applications` on a Mac where another account installed the app), the archive goes to the user's temporary folder
/// instead and the swap runs through `osascript`, whose `mv` copies across volumes by itself if it has to.
fn scratch(app: &Path) -> Result<(PathBuf, bool), String> {
    let parent = app
        .parent()
        .ok_or("the app is at the root of a volume, there is nowhere to unpack the update")?;
    let beside = parent.join(format!(".openfader-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&beside);
    match std::fs::create_dir_all(&beside) {
        Ok(()) => return Ok((beside, false)),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => return Err(format!("{}: {e}", beside.display())),
    }
    let temp = std::env::temp_dir().join(format!(".openfader-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    Ok((temp, true))
}

/// The `.app` this process runs from; a development build (`target/release/openfader`) has none to replace.
fn bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the running program: {e}"))?;
    let app = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("this is not a packaged MyOpenFadeR.app, so it cannot update itself")?;
    Ok(app.to_path_buf())
}

fn download(r: &Release, to: &Path, progress: &dyn Fn(f32)) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let resp = agent
        .get(&r.asset)
        .call()
        .map_err(|e| format!("the download failed: {e}"))?;
    let mut body = resp.into_body().into_reader();
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?,
    );
    let (mut buf, mut got) = (vec![0u8; 1 << 16], 0u64);
    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("the download was cut short: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("{}: {e}", to.display()))?;
        got += n as u64;
        if r.size > 0 {
            progress((got as f32 / r.size as f32).min(1.0));
        }
    }
    out.flush()
        .map_err(|e| format!("{}: {e}", to.display()))?;
    if r.size > 0 && got != r.size {
        return Err(format!(
            "the download is {got} bytes and the release says {}: trying again later may help",
            r.size
        ));
    }
    Ok(())
}

/// The SHA256 of a file, in hex: what GitHub publishes for each asset (`"sha256:…"` in the API's `digest`).
fn sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// The archive's `MyOpenFadeR.app` into `to`, which must not exist yet.
fn unpack(archive: &Path, to: &Path) -> Result<(), String> {
    let mut z = openfader::zip::Zip::open(archive).map_err(|e| format!("{archive:?}: {e:?}"))?;
    if !z.has("MyOpenFadeR.app/Contents/MacOS/openfader") {
        return Err("the archive does not hold a MyOpenFadeR.app".into());
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    z.unpack("MyOpenFadeR.app/", to, &[])?;
    if !to.join("Contents/Info.plist").is_file() {
        return Err("the archive does not hold a complete MyOpenFadeR.app".into());
    }
    Ok(())
}

/// Puts `new` where `app` is, keeping `app` as `old` until the move works. A failure at the second step puts the running
/// bundle back, so the app is never left half-replaced. `admin` says the app's folder is not writable ([`scratch`]).
fn swap(app: &Path, new: &Path, admin: bool) -> Result<(), String> {
    let old = app.with_file_name(format!(".MyOpenFadeR-old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&old);
    if admin {
        return admin_swap(app, new, &old);
    }
    if let Err(e) = std::fs::rename(app, &old) {
        // `/Applications` belongs to `admin` (usually writable) but can be owned by root: one system prompt fixes it.
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return admin_swap(app, new, &old);
        }
        return Err(format!("cannot replace {}: {e}", app.display()));
    }
    if let Err(e) = std::fs::rename(new, app) {
        let _ = std::fs::rename(&old, app);
        return Err(format!("cannot move the update into {}: {e}", app.display()));
    }
    Ok(())
}

/// [`swap`] with administrator rights, through the one password prompt macOS shows for `osascript`.
#[cfg(target_os = "macos")]
fn admin_swap(app: &Path, new: &Path, old: &Path) -> Result<(), String> {
    let script = format!(
        "do shell script \"mv {} {} && mv {} {}\" with administrator privileges",
        quote(old),
        quote(app),
        quote(new),
        quote(app)
    );
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("cannot ask for administrator rights: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr);
    Err(format!(
        "cannot replace {} ({}); move MyOpenFadeR.app to your own Applications folder and try again",
        app.display(),
        why.trim()
    ))
}

/// [`swap`] with administrator rights: macOS only, the tree packages no app for the other platforms.
#[cfg(not(target_os = "macos"))]
fn admin_swap(app: &Path, _new: &Path, _old: &Path) -> Result<(), String> {
    Err(format!(
        "cannot replace {}: its folder is not writable",
        app.display()
    ))
}

/// A shell word for one path, and the same word inside an AppleScript string.
#[cfg(target_os = "macos")]
fn quote(p: &Path) -> String {
    let word = format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
    word.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asset(digest: Option<&str>) -> serde_json::Value {
        json!({
            "tag_name": "v0.5.0-catalina.3",
            "html_url": "https://github.com/ronellcl/MyOpenFadeR-Mac-Legacy/releases/tag/v0.5.0-catalina.3",
            "assets": [{
                "name": "MyOpenFadeR-0.5.0-catalina.3-macos-catalina-x86_64.zip",
                "browser_download_url": "https://example.invalid/update.zip",
                "size": 95_953_862u64,
                "digest": digest,
            }]
        })
    }

    #[test]
    fn a_release_with_its_checksum_is_installable() {
        let r = release(&asset(Some("sha256:4694A249adc1b959"))).unwrap();
        assert_eq!(r.version(), "0.5.0-catalina.3");
        assert_eq!(r.sha256, "4694A249adc1b959");
        assert_eq!(r.asset, "https://example.invalid/update.zip");
        assert_eq!(r.size, 95_953_862);
    }

    #[test]
    fn a_release_without_checksum_or_asset_is_not_offered() {
        assert!(release(&asset(None)).is_none(), "no digest");
        let mut other = asset(Some("sha256:00"));
        other["assets"][0]["name"] = json!("MyOpenFadeR-0.5.0-macos-arm64.zip");
        assert!(release(&other).is_none(), "another platform");
    }

    #[test]
    fn a_version_is_the_tag_without_its_v() {
        let r = release(&asset(Some("sha256:00"))).unwrap();
        assert_eq!(r.version(), "0.5.0-catalina.3");
    }
}
