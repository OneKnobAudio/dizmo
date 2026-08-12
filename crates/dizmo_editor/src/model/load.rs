//! Loading a kit from disk via `dizmo_kit` (EDITOR_PLAN.md Phase 2).

use std::path::{Path, PathBuf};

use dizmo_kit::{DizmoKit, KitError, MidiMap};

use crate::model::{EditorInstrument, EditorKit};

/// Loads a kit from its `drumkit.xml` file into the editable model.
///
/// All parsing and reference resolution is delegated to
/// [`dizmo_kit::DizmoKit::load`] — the canonical loader, with the same rules
/// the plugin uses. This function only re-packages its output into the
/// editor's `EditorKit`, pairing each instrument with the `InstrumentRef`
/// that references it (which also carries its XML file path).
///
/// The kit's declared midimap is read too, but a broken or missing one is not
/// fatal: it yields an empty map plus a warning string for the status bar.
pub fn load(file_path: &Path) -> Result<(EditorKit, Option<String>), KitError> {
    let kit = DizmoKit::load(file_path)?;

    let instruments = kit
        .drums
        .instrument_refs
        .iter()
        .zip(&kit.instruments)
        .map(|(reference, instrument)| EditorInstrument {
            file: PathBuf::from(&reference.file),
            reference: reference.clone(),
            instrument: instrument.clone(),
        })
        .collect::<Vec<_>>();

    let (midimap, warning) = match &kit.default_midimap {
        Some(relative) => match kit.load_midimap(relative) {
            Ok(map) => (map, None),
            Err(err) => (
                MidiMap::default(),
                Some(format!("MIDI map '{relative}' could not be read: {err}")),
            ),
        },
        None => (MidiMap::default(), None),
    };

    let mut drumkit = kit.drums;
    drumkit.default_midimap = kit.default_midimap;

    Ok((
        EditorKit {
            root_dir: Some(kit.root_dir),
            drumkit,
            instruments,
            midimap,
            dirty: false,
        },
        warning,
    ))
}
