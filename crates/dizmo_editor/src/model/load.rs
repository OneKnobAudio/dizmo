//! Loading a kit from disk via the `dizmo_kit` parsers (EDITOR_PLAN.md Phase 2).

use std::path::{Path, PathBuf};

use dizmo_kit::drumkit;
use dizmo_kit::instrument;
use dizmo_kit::midimap;
use dizmo_kit::{DrumKit, Instrument, KitError, MidiMap};

use crate::model::{EditorInstrument, EditorKit};

/// Loads the kit rooted at `root` (the directory containing `drumkit.xml`).
///
/// Instrument XML files are resolved relative to `root`. Each instrument is
/// kept paired with its `drumkit.xml` reference; like
/// [`dizmo_kit::DizmoKit::load`], the reference name, group, channel map and
/// chokes are canonical, so editing them here matches what the engine loads.
///
/// The declared midimap is read too, but a broken or missing one is not fatal:
/// it yields an empty map plus a warning string to surface in the status bar.
pub fn load(root: &Path) -> Result<(EditorKit, Option<String>), KitError> {
    let drumkit: DrumKit = drumkit::parse_file(&root.join("drumkit.xml"))?;

    let mut instruments = Vec::with_capacity(drumkit.instrument_refs.len());
    for (id, reference) in drumkit.instrument_refs.iter().enumerate() {
        let file = PathBuf::from(&reference.file);
        let mut instrument: Instrument = instrument::parse_file(&root.join(&file))?;
        instrument.id = id;
        instrument.name = reference.name.clone();
        instrument.group = reference.group.clone();
        instrument.channel_map = reference.channel_map.clone();
        instrument.chokes = reference.chokes.clone();
        instruments.push(EditorInstrument {
            file,
            reference: reference.clone(),
            instrument,
        });
    }

    let declared = drumkit.default_midimap.clone();
    let midimap_relative = declared.as_deref().unwrap_or("midimap.xml");
    let midimap_path = root.join(midimap_relative);
    let (midimap, warning) = if midimap_path.is_file() {
        match midimap::parse_file(&midimap_path) {
            Ok(map) => (map, None),
            Err(err) => (
                MidiMap::default(),
                Some(format!("MIDI map '{midimap_relative}' could not be read: {err}")),
            ),
        }
    } else if declared.is_some() {
        (
            MidiMap::default(),
            Some(format!("Declared MIDI map '{midimap_relative}' is missing.")),
        )
    } else {
        (MidiMap::default(), None)
    };

    Ok((
        EditorKit {
            root_dir: Some(root.to_path_buf()),
            drumkit,
            instruments,
            midimap,
            dirty: false,
        },
        warning,
    ))
}
