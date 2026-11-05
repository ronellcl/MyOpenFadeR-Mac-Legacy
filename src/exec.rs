//! Execute bits.
//!
//! The program starts other programs: `ffmpeg` and `ffprobe`, the Python interpreter that runs a plugin, the `command` of
//! a plugin, and the tools a plugin downloaded into its own `bin/` (yt-dlp is the one that bites). A file that arrives
//! without its execute bits fails with "Permission denied", and the user cannot be asked to run `chmod`: it is a release
//! archive's job. [`crate::zip::Zip::unpack`] does restore the mode stored in the zip, but a copy that an older build
//! made, a binary that a plugin downloaded itself, or one unpacked by a tool that drops the mode does not have it. So the
//! bit is given back where it matters, and nothing else:
//!
//! - [`make_executable`] before starting a program;
//! - [`repair_tools`] once at start-up, over the bundled `tools/` and `python/` and over the `bin/` folder of every plugin
//!   the app reads, which covers the programs the app does not start itself (the Python plugins shelling out).

use std::path::{Path, PathBuf};

/// Adds the execute bits to `path` when it has none. A path that does not exist, that is not a file, or that cannot be
/// changed (a read-only bundle, a system program owned by root) is left alone: the start that follows reports the real
/// problem. Windows has no such bit.
pub fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = path.metadata() else {
            return;
        };
        let mode = meta.permissions().mode();
        if meta.is_file() && mode & 0o111 == 0 {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o111));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// [`make_executable`] on every regular file inside `dir`, `depth` folders deep (`0`: the files of `dir` itself).
///
/// Data files are skipped: a licence, a readme or an AppleDouble `._name` left by a zip unpacked on macOS is not a
/// program, and `ls` showing it executable would only confuse. A folder that cannot be read ends that branch.
pub fn make_tree_executable(dir: &Path, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(kind) = e.file_type() else {
            continue;
        };
        let path = e.path();
        if kind.is_dir() {
            if depth > 0 {
                make_tree_executable(&path, depth - 1);
            }
            continue;
        }
        let name = e.file_name();
        let name = name.to_string_lossy();
        let data = name.starts_with('.') || name.to_ascii_uppercase().starts_with("LICENSE");
        if kind.is_file() && !data {
            make_executable(&path);
        }
    }
}

/// Repairs the programs that travel with the app: `tools/` (ffmpeg, ffprobe) and `python/` beside the executable or under
/// a macOS bundle's `Resources/`, and the `bin/` folder of every plugin folder the app reads (bundled, the user's, the
/// ones named by `$OPENFADER_PLUGINS`). Called once, before anything starts a child process.
pub fn repair_tools() {
    for sub in ["tools", "python"] {
        if let Some(dir) = beside_exe(sub) {
            make_tree_executable(&dir, 2);
        }
    }
    for root in crate::paths::search_path("OPENFADER_PLUGINS", "extension/plugin") {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let bin = e.path().join("bin");
            if bin.is_dir() {
                make_tree_executable(&bin, 2);
            }
        }
    }
}

/// The first `<sub>` next to the executable, or under the `Resources` folder of a macOS bundle above it.
fn beside_exe(sub: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .flat_map(|d| [d.join(sub), d.join("Resources").join(sub)])
        .find(|d| d.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("openfader-exec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn a_program_gets_its_execute_bits_back_and_keeps_the_rest() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("one");
        let p = d.join("tool");
        std::fs::write(&p, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        make_executable(&p);
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o751, "0o111 added, the rest of the mode kept");
        make_executable(&p); // already executable: no change
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o751);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_tree_is_repaired_but_licences_and_hidden_files_are_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("tree");
        let bin = d.join("bin");
        std::fs::create_dir_all(bin.join("nested")).unwrap();
        for p in [bin.join("tool"), bin.join("nested").join("deep"), bin.join("LICENSE.txt"), bin.join("._tool")] {
            std::fs::write(&p, b"x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        make_tree_executable(&d, 2);
        let x = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o111 != 0;
        assert!(x(&bin.join("tool")));
        assert!(x(&bin.join("nested").join("deep")));
        assert!(!x(&bin.join("LICENSE.txt")));
        assert!(!x(&bin.join("._tool")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_there_is_not_an_error() {
        make_executable(&std::env::temp_dir().join("openfader-exec-missing"));
        make_tree_executable(&std::env::temp_dir().join("openfader-exec-missing"), 1);
    }
}
