//! The EXPORT / IMPORT chips of the MIDI overlay: the mapping of the connected controller as one file.
//!
//! Export writes the mapping file in use with the user's own MIDI LEARN rows merged over it (for the same control the user's
//! row wins, as it does when playing), so the file alone carries the whole setup: it can be dropped into another Mac's
//! mapping folder or imported here again. Import validates such a file, writes it into this Mac's mapping folder and reloads
//! the controller.

use super::{Midi, mapfile};
use crate::app::library::dialog;
use crate::app::{App, MidiState};
use slint::ComponentHandle;
use openfader::paths::config_dir;
use std::path::{Path, PathBuf};

/// The mapping in use for the connected controller, with the user's bindings over it.
///
/// `None` when no controller is connected or no file matches it (the note says which).
fn merged(midi: &Midi) -> Result<(mapfile::Mapping, String, String), String> {
    let ports = midi.names.borrow().clone();
    if ports.is_empty() {
        return Err("Connect the controller whose mapping you want to export.".into());
    }
    let picked = mapfile::load(&ports);
    let Some(mut m) = picked.map else {
        return Err("No mapping file matches this controller: there is nothing to export.".into());
    };
    // MIDI LEARN rows first, exactly as `input::find` reads them: for the same control the user's binding wins.
    for b in midi.user.borrow().iter() {
        m.binds.retain(|f| (f.status, f.d1) != (b.status, b.d1));
        m.binds.push(*b);
    }
    Ok((m, picked.name, picked.port))
}

/// `EXPORT MAPPING`: asks for a file and writes the mapping in use, MIDI LEARN rows included.
pub fn export(app: &App, midi: &Midi) {
    let state = app.global::<MidiState>();
    let (m, _, port) = match merged(midi) {
        Ok(v) => v,
        Err(why) => {
            state.set_map_note(why.into());
            return;
        }
    };
    let name = format!("{}-openfader.xml", plain(&m.controller));
    let Some(path) = dialog::browse(dialog::Pick::Save, &name) else {
        return;
    };
    let path = with_xml(path);
    match std::fs::write(&path, mapfile::write(&m, &port)) {
        Ok(()) => {
            state.set_map_note(format!("Exported {} controls to {}", m.binds.len(), path.display()).into());
            dialog::show_in_folder(Path::new(&path));
        }
        Err(e) => state.set_map_note(format!("Could not write {}: {e}", path.display()).into()),
    }
}

/// `IMPORT MAPPING`: reads a mapping file, checks it, installs it in the mapping folder and loads it.
///
/// It is written under the name of the file the connected controller uses now, so that it replaces it: a file with another
/// name that matches the same port equally well would lose the tie against the bundled copy.
pub fn import(app: &App, midi: &Midi) {
    let state = app.global::<MidiState>();
    let Some(path) = dialog::browse(dialog::Pick::File, "") else {
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            state.set_map_note(format!("Could not read {path}: {e}").into());
            return;
        }
    };
    let (m, port) = match mapfile::parse(&text) {
        Ok(v) => v,
        Err(e) => {
            state.set_map_note(format!("{path} is not a mapping file: {e}").into());
            return;
        }
    };
    let ports = midi.names.borrow().clone();
    let in_use = mapfile::load(&ports).name;
    let name = if in_use.is_empty() {
        match Path::new(&path).file_name() {
            Some(n) if n.to_string_lossy().ends_with(".xml") => n.to_string_lossy().into_owned(),
            _ => "imported.xml".to_owned(),
        }
    } else {
        in_use
    };
    let Some(dir) = config_dir().map(|d| d.join("mappings")) else {
        state.set_map_note("No config folder to import into (set HOME).".into());
        return;
    };
    let to = dir.join(&name);
    match install(&dir, &to, &text) {
        Ok(()) => {
            state.set_map_note(format!("Imported {} controls as {name}", m.binds.len()).into());
            super::get::reload_if_connected(&super::get::Entry {
                name,
                controller: m.controller,
                port,
                text,
                cards: None,
            });
        }
        Err(e) => state.set_map_note(format!("Could not import into {}: {e}", to.display()).into()),
    }
}

/// Writes `text` to `to` through a `.part` file, as the repository install does: a failure never leaves half a mapping.
fn install(dir: &Path, to: &Path, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = to.with_extension("xml.part");
    let written = std::fs::write(&part, text).and_then(|()| std::fs::rename(&part, to));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written.map_err(|e| e.to_string())
}

/// `path` with `.xml` when it has no extension (a save dialog may return the bare name).
fn with_xml(path: String) -> PathBuf {
    let p = PathBuf::from(path);
    if p.extension().is_some() {
        p
    } else {
        p.with_extension("xml")
    }
}

/// A controller name safe to use as a file name.
fn plain(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '-' })
        .collect();
    let s = s.trim().trim_matches('.').to_owned();
    if s.is_empty() { "mapping".to_owned() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controller_name_becomes_a_file_name() {
        assert_eq!(plain("PIONEER DDJ-SX2"), "PIONEER DDJ-SX2");
        assert_eq!(plain("A/B: C"), "A-B- C");
        assert_eq!(plain(" / "), "-");
        assert_eq!(plain(""), "mapping");
    }

    #[test]
    fn a_saved_name_without_extension_gets_one() {
        assert_eq!(with_xml("/tmp/x".into()), PathBuf::from("/tmp/x.xml"));
        assert_eq!(with_xml("/tmp/x.xml".into()), PathBuf::from("/tmp/x.xml"));
    }
}
