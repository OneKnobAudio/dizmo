//! Loading a kit from disk via `dizmo_kit` (EDITOR_PLAN.md Phase 2).

use std::path::{Path, PathBuf};

use dizmo_kit::{DizmoKit, Instrument, KitError, MidiMap};

use crate::model::{EditorInstrument, EditorKit, PowerScale};

/// Rescales a v2 instrument's sample powers into the editor's 0..1 range and
/// returns the original range so `save` can map values back. Powers are left
/// untouched (and `None` returned) for v1 instruments — they use `<velocity>`
/// groups — and when the range is zero or has no finite values (no inverse).
pub(crate) fn rescale_powers(instrument: &mut Instrument) -> Option<PowerScale> {
    if !instrument.is_v2() || instrument.samples.is_empty() {
        return None;
    }
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for sample in &instrument.samples {
        if sample.power.is_finite() {
            min = min.min(sample.power);
            max = max.max(sample.power);
        }
    }
    if min >= max {
        // Zero span (all powers equal) or no finite powers: there is no
        // inverse transform, so the values are kept as they are.
        return None;
    }
    let span = f64::from(max) - f64::from(min);
    for sample in &mut instrument.samples {
        if sample.power.is_finite() {
            sample.power = ((f64::from(sample.power) - f64::from(min)) / span) as f32;
        }
    }
    Some(PowerScale { min, max })
}

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
        .map(|(reference, instrument)| {
            let mut instrument = instrument.clone();
            let power_scale = rescale_powers(&mut instrument);
            EditorInstrument {
                file: PathBuf::from(&reference.file),
                reference: reference.clone(),
                instrument,
                power_scale,
            }
        })
        .collect::<Vec<_>>();

    let (midimap, mut warning) = match &kit.default_midimap {
        Some(relative) => match kit.load_midimap(relative) {
            Ok(map) => (map, None),
            Err(err) => (
                MidiMap::default(),
                Some(format!("MIDI map '{relative}' could not be read: {err}")),
            ),
        },
        None => (MidiMap::default(), None),
    };

    if kit.instruments.iter().any(|inst| !inst.is_v2()) {
        let v1_names: Vec<String> = kit
            .instruments
            .iter()
            .filter(|inst| !inst.is_v2())
            .map(|inst| inst.name.clone())
            .collect();
        let message = format!(
            "This kit contains v1.0 instruments ({}). Saving will convert them to v2.0.",
            v1_names.join(", ")
        );
        warning = Some(match warning {
            Some(existing) => format!("{existing} {message}"),
            None => message,
        });
    }

    let kit_file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());

    Ok((
        EditorKit {
            root_dir: Some(kit.root_dir),
            kit_file_name,
            default_midimap: kit.default_midimap,
            drumkit: kit.drums,
            instruments,
            midimap,
            dirty: false,
            warnings: Vec::new(),
        },
        warning,
    ))
}
