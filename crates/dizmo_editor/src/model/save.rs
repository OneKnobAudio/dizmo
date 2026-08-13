//! Saving a kit to disk: `drumkit.xml`, `midimap.xml`, one folder per
//! instrument containing its instrument XML and a `samples/` folder with the
//! copied WAVs (EDITOR_PLAN.md §4/§5).
//!
//! The serializer **normalizes** every kit into this layout on save:
//!
//! ```text
//! <root>/
//!   drumkit.xml
//!   midimap.xml
//!   <name>/              ← one folder per instrument, named after it (case kept)
//!     <name>.xml         ← that instrument's definition
//!     samples/           ← its samples folder (created if missing)
//!       …wav             ← sample audio, copied in
//! ```
//!
//! Rules:
//! - A WAV already inside the kit root is referenced in place (relative path)
//!   instead of being copied.
//! - A referenced WAV that does not exist on disk keeps a deterministic path
//!   under the instrument's `samples/` (dangling; import fills it in) rather
//!   than aborting — but any *actual* copy/write error aborts the whole save.
//! - All file writes go to a temp file then an atomic rename; `drumkit.xml` is
//!   written last, so an interrupted save never leaves a loadable-but-broken kit.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use dizmo_kit::{DrumKit, Instrument, MidiMap};

use crate::model::{EditorInstrument, EditorKit};

struct InstrumentPlan {
    folder: PathBuf,
    file: PathBuf,
    rel_file: String,
    xml: String,
    copies: Vec<(PathBuf, PathBuf)>,
    audio_paths: Vec<String>,
}

/// Writes the complete kit into `kit.root_dir`, normalizing the layout, and
/// updates the model in place to match what was written.
pub fn save(kit: &mut EditorKit) -> Result<(), String> {
    let root = kit
        .root_dir
        .clone()
        .ok_or_else(|| "No save directory chosen yet.".to_string())?;
    std::fs::create_dir_all(&root)
        .map_err(|err| format!("Could not create '{}': {err}", root.display()))?;

    let mut seen = std::collections::HashSet::new();
    for inst in &kit.instruments {
        let name = inst.reference.name.trim();
        if !seen.insert(name.to_string()) {
            return Err(format!(
                "Duplicate instrument name '{name}'; names must be unique to save."
            ));
        }
    }

    let plans = kit
        .instruments
        .iter()
        .map(|inst| plan_instrument(inst, &root))
        .collect::<Result<Vec<_>, _>>()?;

    for plan in &plans {
        for (source, dest) in &plan.copies {
            std::fs::copy(source, dest).map_err(|err| {
                format!(
                    "Could not copy sample '{}' to '{}': {err}",
                    source.display(),
                    dest.display()
                )
            })?;
        }
    }
    for plan in &plans {
        write_atomic(&plan.file, &plan.xml)?;
    }

    let write_midimap = !kit.midimap.entries.is_empty() || kit.drumkit.default_midimap.is_some();
    let midimap_name = write_midimap.then(|| "midimap.xml".to_string());
    if let Some(name) = &midimap_name {
        write_atomic(&root.join(name), &serialize_midimap(&kit.midimap))?;
    }

    let ref_files: Vec<String> = plans.iter().map(|plan| plan.rel_file.clone()).collect();
    write_atomic(
        &root.join("drumkit.xml"),
        &serialize_drumkit(&kit.drumkit, &ref_files, midimap_name.as_deref()),
    )?;

    for (inst, plan) in kit.instruments.iter_mut().zip(&plans) {
        inst.file = PathBuf::from(&plan.rel_file);
        inst.reference.file = plan.rel_file.clone();
        inst.instrument.base_dir = plan.folder.clone();
        let mut index = 0;
        for sample in &mut inst.instrument.samples {
            for audio in &mut sample.audio_files {
                if let Some(path) = plan.audio_paths.get(index) {
                    audio.file = path.clone();
                }
                index += 1;
            }
        }
    }
    kit.drumkit.default_midimap = midimap_name;
    kit.dirty = false;
    Ok(())
}

fn plan_instrument(inst: &EditorInstrument, root: &Path) -> Result<InstrumentPlan, String> {
    let name = inst.reference.name.trim();
    if name.is_empty() {
        return Err("Instrument with no name cannot be saved.".to_string());
    }
    let folder = root.join(name);
    let samples_dir = folder.join("samples");
    std::fs::create_dir_all(&samples_dir)
        .map_err(|err| format!("Could not create '{}': {err}", samples_dir.display()))?;

    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut source_dest: HashMap<PathBuf, String> = HashMap::new();
    let mut dest_names: HashMap<String, ()> = HashMap::new();
    let mut audio_paths: Vec<String> = Vec::new();

    for sample in &inst.instrument.samples {
        for audio in &sample.audio_files {
            let source = inst.instrument.base_dir.join(&audio.file);
            let key = source.canonicalize().unwrap_or_else(|_| source.clone());

            let rel = if let Some(existing) = source_dest.get(&key) {
                existing.clone()
            } else if is_inside(&source, root) {
                let rel = relative_to(&folder, &source)
                    .to_string_lossy()
                    .replace('\\', "/");
                source_dest.insert(key, rel.clone());
                rel
            } else {
                let base = source
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "sample.wav".to_string());
                let unique = unique_name(&base, &mut dest_names);
                if source.exists() {
                    copies.push((source, samples_dir.join(&unique)));
                }
                let rel = format!("samples/{unique}");
                source_dest.insert(key, rel.clone());
                rel
            };
            audio_paths.push(rel);
        }
    }

    Ok(InstrumentPlan {
        file: folder.join(format!("{name}.xml")),
        rel_file: format!("{name}/{name}.xml"),
        xml: serialize_instrument(&inst.instrument, &audio_paths),
        folder,
        copies,
        audio_paths,
    })
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)
        .map_err(|err| format!("Could not write '{}': {err}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|err| format!("Could not write '{}': {err}", path.display()))
}

fn is_inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

/// The relative path from `base` to `target`, using `..` segments as needed.
fn relative_to(base: &Path, target: &Path) -> PathBuf {
    let base_parts: Vec<&std::ffi::OsStr> = base.components().map(Component::as_os_str).collect();
    let target_parts: Vec<&std::ffi::OsStr> =
        target.components().map(Component::as_os_str).collect();
    let common = base_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel = PathBuf::new();
    for _ in common..base_parts.len() {
        rel.push("..");
    }
    for part in &target_parts[common..] {
        rel.push(part);
    }
    rel
}

/// Appends a ` (n)` suffix to a file name until it is free in `taken`.
fn unique_name(base: &str, taken: &mut HashMap<String, ()>) -> String {
    if !taken.contains_key(base) {
        taken.insert(base.to_string(), ());
        return base.to_string();
    }
    let (stem, ext) = match base.rfind('.') {
        Some(index) => (&base[..index], &base[index..]),
        None => (base, ""),
    };
    let mut n = 2;
    loop {
        let candidate = format!("{stem} ({n}){ext}");
        if !taken.contains_key(&candidate) {
            taken.insert(candidate.clone(), ());
            return candidate;
        }
        n += 1;
    }
}

fn serialize_drumkit(
    drumkit: &DrumKit,
    instrument_files: &[String],
    midimap: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<drumkit samplerate=\"{}\" version=\"{}\">\n",
        drumkit.samplerate,
        esc(&drumkit.version)
    ));
    out.push_str("  <metadata>\n");
    out.push_str(&format!("    <title>{}</title>\n", esc(&drumkit.name)));
    out.push_str(&format!(
        "    <description>{}</description>\n",
        esc(&drumkit.description)
    ));
    if let Some(midimap) = midimap {
        out.push_str(&format!("    <defaultmidimap src=\"{}\"/>\n", esc(midimap)));
    }
    out.push_str("  </metadata>\n");
    out.push_str("  <channels>\n");
    for channel in &drumkit.channels {
        out.push_str(&format!("    <channel name=\"{}\"/>\n", esc(&channel.name)));
    }
    out.push_str("  </channels>\n");
    out.push_str("  <instruments>\n");
    for (index, reference) in drumkit.instrument_refs.iter().enumerate() {
        let file = instrument_files
            .get(index)
            .map(String::as_str)
            .unwrap_or(&reference.file);
        let group = reference
            .group
            .as_deref()
            .map(|g| format!(" group=\"{}\"", esc(g)))
            .unwrap_or_default();
        out.push_str(&format!(
            "    <instrument name=\"{}\" file=\"{}\"{group}>\n",
            esc(&reference.name),
            esc(file)
        ));
        for map in &reference.channel_map {
            let main = if map.is_main { " main=\"true\"" } else { "" };
            out.push_str(&format!(
                "      <channelmap in=\"{}\" out=\"{}\"{main}/>\n",
                esc(&map.in_name),
                esc(&map.out_name)
            ));
        }
        if !reference.chokes.is_empty() {
            out.push_str("      <chokes>\n");
            for choke in &reference.chokes {
                out.push_str(&format!(
                    "        <choke instrument=\"{}\" choketime=\"{}\"/>\n",
                    esc(&choke.instrument),
                    choke.choketime_ms
                ));
            }
            out.push_str("      </chokes>\n");
        }
        out.push_str("    </instrument>\n");
    }
    out.push_str("  </instruments>\n");
    out.push_str("</drumkit>\n");
    out
}

fn serialize_instrument(inst: &Instrument, audio_paths: &[String]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<instrument version=\"{}\" name=\"{}\"{}>\n",
        esc(&inst.version),
        esc(&inst.name),
        if inst.description.is_empty() {
            String::new()
        } else {
            format!(" description=\"{}\"", esc(&inst.description))
        }
    ));
    out.push_str("  <channels>\n");
    for channel in &inst.channels {
        let main = if channel.is_main {
            " main=\"true\""
        } else {
            ""
        };
        out.push_str(&format!(
            "    <channel name=\"{}\"{main}/>\n",
            esc(&channel.name)
        ));
    }
    out.push_str("  </channels>\n");

    let v2 = inst.is_v2();
    if !inst.samples.is_empty() {
        out.push_str("  <samples>\n");
        let mut audio_index = 0;
        for sample in &inst.samples {
            if v2 {
                out.push_str(&format!(
                    "    <sample name=\"{}\" power=\"{}\" normalized=\"{}\">\n",
                    esc(&sample.name),
                    sample.power,
                    if sample.normalized { "true" } else { "false" }
                ));
            } else {
                out.push_str(&format!("    <sample name=\"{}\">\n", esc(&sample.name)));
            }
            for audio in &sample.audio_files {
                let file = audio_paths
                    .get(audio_index)
                    .map(String::as_str)
                    .unwrap_or(&audio.file);
                out.push_str(&format!(
                    "      <audiofile channel=\"{}\" file=\"{}\" filechannel=\"{}\"/>\n",
                    esc(&audio.channel),
                    esc(file),
                    audio.file_channel + 1
                ));
                audio_index += 1;
            }
            out.push_str("    </sample>\n");
        }
        out.push_str("  </samples>\n");
    }

    if !v2 && !inst.velocities.is_empty() {
        out.push_str("  <velocities>\n");
        for group in &inst.velocities {
            out.push_str(&format!(
                "    <velocity lower=\"{}\" upper=\"{}\">\n",
                group.lower, group.upper
            ));
            for sample_ref in &group.sample_refs {
                out.push_str(&format!(
                    "      <sampleref name=\"{}\" probability=\"{}\"/>\n",
                    esc(&sample_ref.name),
                    sample_ref.probability
                ));
            }
            out.push_str("    </velocity>\n");
        }
        out.push_str("  </velocities>\n");
    }

    out.push_str("</instrument>\n");
    out
}

fn serialize_midimap(midimap: &MidiMap) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<midimap>\n");
    for entry in &midimap.entries {
        out.push_str(&format!(
            "  <map note=\"{}\" instr=\"{}\"/>\n",
            entry.note,
            esc(&entry.instrument)
        ));
    }
    out.push_str("</midimap>\n");
    out
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::load::load;
    use dizmo_kit::DizmoKit;

    fn fixture_drumkit() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("dizmo_kit")
            .join("tests")
            .join("fixtures")
            .join("kit")
            .join("drumkit.xml")
    }

    fn write_wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..100 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn save_normalizes_layout_copies_and_round_trips() {
        let root =
            std::env::temp_dir().join(format!("dizmo_editor_save_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let external = root.parent().unwrap().join("outside.wav");
        write_wav(&external);

        let mut kit = EditorKit::new_kit("Save Test", 48000.0, &["Kick".into()]);
        kit.root_dir = Some(root.clone());

        let kick = kit.add_instrument("Kick Drum");
        kit.instruments[kick].instrument.base_dir = external.parent().unwrap().to_path_buf();
        kit.add_sample(kick, "Kick-1", "outside.wav");

        let snare = kit.add_instrument("Snare");
        kit.instruments[snare].instrument.base_dir = root.clone();
        let inside = root.join("inside.wav");
        write_wav(&inside);
        kit.add_sample(snare, "Snare-1", "inside.wav");

        kit.add_note(35);
        let note_row = kit
            .midimap
            .entries
            .iter()
            .position(|e| e.note == 35)
            .unwrap();
        kit.midimap.entries[note_row].instrument = "Kick Drum".into();

        save(&mut kit).unwrap();
        assert!(!kit.dirty);
        assert_eq!(kit.drumkit.default_midimap.as_deref(), Some("midimap.xml"));

        assert!(root.join("Kick Drum/Kick Drum.xml").exists());
        assert!(root.join("Kick Drum/samples/outside.wav").exists());
        assert!(root.join("Snare/Snare.xml").exists());
        assert!(!root.join("Snare/samples/inside.wav").exists());

        let loaded = DizmoKit::load(root.join("drumkit.xml")).unwrap();
        assert_eq!(loaded.drums.name, "Save Test");
        assert_eq!(loaded.drums.samplerate, 48000.0);
        assert_eq!(loaded.instruments.len(), 2);

        assert_eq!(loaded.instruments[0].name, "Kick Drum");
        assert_eq!(
            loaded.drums.instrument_refs[0].file,
            "Kick Drum/Kick Drum.xml"
        );
        assert_eq!(
            loaded.instruments[0].samples[0].audio_files[0].file,
            "samples/outside.wav"
        );

        assert_eq!(loaded.instruments[1].name, "Snare");
        assert_eq!(loaded.drums.instrument_refs[1].file, "Snare/Snare.xml");
        assert_eq!(
            loaded.instruments[1].samples[0].audio_files[0].file,
            "../inside.wav"
        );

        let midimap = loaded.load_midimap("midimap.xml").unwrap();
        assert_eq!(midimap.instrument_for_note(35), Some("Kick Drum"));

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&external);
    }

    #[test]
    fn save_missing_source_does_not_abort() {
        let root =
            std::env::temp_dir().join(format!("dizmo_editor_save_missing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let mut kit = EditorKit::new_kit("Ghost Kit", 44100.0, &["A".into()]);
        kit.root_dir = Some(root.clone());
        let index = kit.add_instrument("Ghost");
        kit.add_sample(index, "Ghost-1", "samples/ghost.wav");

        save(&mut kit).unwrap();

        let loaded = DizmoKit::load(root.join("drumkit.xml")).unwrap();
        assert_eq!(loaded.instruments[0].name, "Ghost");
        assert_eq!(
            loaded.instruments[0].samples[0].audio_files[0].file,
            "samples/ghost.wav"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_rejects_duplicate_instrument_names() {
        let root =
            std::env::temp_dir().join(format!("dizmo_editor_save_dup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let mut kit = EditorKit::new_kit("Dup Kit", 44100.0, &["A".into()]);
        kit.root_dir = Some(root.clone());
        kit.add_instrument("Same");
        kit.add_instrument("Same");

        assert!(save(&mut kit).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fixture_kit_round_trips() {
        let fixture = fixture_drumkit();
        let (mut kit, warning) = load(&fixture).unwrap();
        assert!(
            warning.is_none(),
            "fixture kit should load without warnings"
        );

        let root =
            std::env::temp_dir().join(format!("dizmo_editor_fixture_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);

        kit.root_dir = Some(root.clone());
        save(&mut kit).unwrap();

        let loaded = DizmoKit::load(root.join("drumkit.xml")).unwrap();

        assert_eq!(loaded.drums.name, kit.drumkit.name);
        assert_eq!(loaded.drums.description, kit.drumkit.description);
        assert_eq!(loaded.drums.samplerate, kit.drumkit.samplerate);
        assert_eq!(loaded.drums.version, kit.drumkit.version);
        assert_eq!(loaded.default_midimap, kit.drumkit.default_midimap);

        assert_eq!(loaded.drums.channels.len(), kit.drumkit.channels.len());
        for (expected, actual) in kit.drumkit.channels.iter().zip(&loaded.drums.channels) {
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.num, expected.num);
        }

        assert_eq!(loaded.instruments.len(), kit.instruments.len());
        for (expected, actual) in kit.instruments.iter().zip(&loaded.instruments) {
            assert_eq!(actual.name, expected.reference.name);
            assert_eq!(actual.group, expected.reference.group);
            assert_eq!(actual.channel_map, expected.reference.channel_map);
            assert_eq!(actual.chokes, expected.reference.chokes);

            assert_eq!(actual.version, expected.instrument.version);
            assert_eq!(actual.description, expected.instrument.description);
            assert_eq!(actual.channels, expected.instrument.channels);

            assert_eq!(actual.samples.len(), expected.instrument.samples.len());
            for (expected_sample, actual_sample) in
                expected.instrument.samples.iter().zip(&actual.samples)
            {
                assert_eq!(actual_sample.name, expected_sample.name);
                assert_eq!(actual_sample.power, expected_sample.power);
                assert_eq!(actual_sample.normalized, expected_sample.normalized);
                assert_eq!(
                    actual_sample.audio_files.len(),
                    expected_sample.audio_files.len()
                );
                for (expected_af, actual_af) in expected_sample
                    .audio_files
                    .iter()
                    .zip(&actual_sample.audio_files)
                {
                    assert_eq!(actual_af.channel, expected_af.channel);
                    assert_eq!(actual_af.file_channel, expected_af.file_channel);
                    assert!(
                        actual_af.file.starts_with("samples/"),
                        "audio file should be normalized to samples/: {}",
                        actual_af.file
                    );
                }
            }

            assert_eq!(actual.velocities, expected.instrument.velocities);
        }

        let midimap = loaded.load_midimap("midimap.xml").unwrap();
        assert_eq!(midimap.entries, kit.midimap.entries);

        let _ = std::fs::remove_dir_all(&root);
    }
}
