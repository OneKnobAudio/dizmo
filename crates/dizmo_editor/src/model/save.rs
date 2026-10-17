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

use dizmo_kit::{DrumKit, Instrument, InstrumentMetadata, KitMetadata, MidiMap};

use crate::model::{EditorInstrument, EditorKit, PowerScale};

struct InstrumentPlan {
    folder: PathBuf,
    file: PathBuf,
    rel_file: String,
    xml: String,
    /// (source, dest, optional target rate to resample the copy to).
    copies: Vec<(PathBuf, PathBuf, Option<u32>)>,
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

    let kit_file_name = kit
        .kit_file_name
        .clone()
        .unwrap_or_else(|| "drumkit.xml".to_string());
    let kit_path = root.join(&kit_file_name);
    let resolved_midimap = midimap_name_for_kit(&kit_path);

    for inst in &mut kit.instruments {
        convert_v1_instrument_to_v2(&mut inst.instrument);
    }

    let mut seen = std::collections::HashSet::new();
    for inst in &kit.instruments {
        // Duplicate check is case-insensitive and trimmed, so two instruments
        // cannot collide on the filesystem (macOS/Windows).
        let name = inst.reference.name.trim().to_lowercase();
        if !seen.insert(name) {
            return Err(format!(
                "Duplicate instrument name '{}'; names must be unique to save.",
                inst.reference.name.trim()
            ));
        }
    }

    let plans = kit
        .instruments
        .iter()
        .map(|inst| plan_instrument(inst, &root))
        .collect::<Result<Vec<_>, _>>()?;

    for plan in &plans {
        for (source, dest, resample_to) in &plan.copies {
            std::fs::copy(source, dest).map_err(|err| {
                format!(
                    "Could not copy sample '{}' to '{}': {err}",
                    source.display(),
                    dest.display()
                )
            })?;
            // Resample the copied file (never the user's original) after it is
            // in place, so the kit matches the declared sample rate.
            if let Some(target_rate) = resample_to {
                crate::audio::resample_wav(dest, *target_rate)?;
            }
        }
    }
    for plan in &plans {
        write_atomic(&plan.file, &plan.xml)?;
    }

    let write_midimap = !kit.midimap.entries.is_empty() || kit.default_midimap.is_some();
    let midimap_name =
        write_midimap.then(|| resolved_midimap.unwrap_or_else(|| "midimap.xml".to_string()));
    if let Some(name) = &midimap_name {
        write_atomic(&root.join(name), &serialize_midimap(&kit.midimap))?;
    }

    let ref_files: Vec<String> = plans.iter().map(|plan| plan.rel_file.clone()).collect();
    write_atomic(&kit_path, &serialize_drumkit(&kit.drumkit, &ref_files))?;

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
            // The resample was applied to the copied files above.
            sample.resample = None;
        }
    }
    kit.default_midimap = midimap_name;
    kit.dirty = false;
    Ok(())
}

/// Converts a version 1.0 (velocity-group) instrument to version 2.0
/// (power-based samples) in place.
///
/// Each sample's `power` is derived from the velocity groups that reference it,
/// weighted by their `sampleref` probabilities. The midpoint of each velocity
/// range `(lower + upper) / 2` is used as the representative power for that
/// reference. After conversion the instrument's `<velocities>` node is emptied
/// and its version is set to `"2.0"`.
fn convert_v1_instrument_to_v2(inst: &mut Instrument) {
    if inst.is_v2() || inst.velocities.is_empty() {
        return;
    }

    let mut weighted: HashMap<String, (f32, f32)> = HashMap::new();
    for group in &inst.velocities {
        let midpoint = (group.lower + group.upper) / 2.0;
        for sample_ref in &group.sample_refs {
            let (sum, weight) = weighted
                .entry(sample_ref.name.clone())
                .or_insert((0.0, 0.0));
            *sum += sample_ref.probability * midpoint;
            *weight += sample_ref.probability;
        }
    }

    for sample in &mut inst.samples {
        let power = weighted
            .get(&sample.name)
            .and_then(|(sum, weight)| {
                if *weight > 0.0 {
                    Some((sum / weight).clamp(0.0, 1.0))
                } else {
                    None
                }
            })
            .unwrap_or(0.5);
        sample.power = power;
        sample.normalized = false;
    }

    inst.velocities.clear();
    inst.version = "2.0".to_string();
}

fn plan_instrument(inst: &EditorInstrument, root: &Path) -> Result<InstrumentPlan, String> {
    let name = inst.reference.name.trim();
    crate::model::types::validate_file_name(name)
        .map_err(|error| format!("Instrument '{}': {error}.", inst.reference.name.trim()))?;
    let folder = root.join(name);
    let samples_dir = folder.join("samples");
    std::fs::create_dir_all(&samples_dir)
        .map_err(|err| format!("Could not create '{}': {err}", samples_dir.display()))?;

    let mut copies: Vec<(PathBuf, PathBuf, Option<u32>)> = Vec::new();
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
                    copies.push((source, samples_dir.join(&unique), sample.resample));
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
        xml: serialize_instrument(&inst.instrument, &audio_paths, inst.power_scale),
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

/// Resolves the midimap filename from the kit XML filename using the same
/// convention as `dizmo_kit`: `<name>.xml` → `midimap.xml`,
/// `<name>_<variation>.xml` → `Midimap_<variation>.xml`.
fn midimap_name_for_kit(kit_path: &Path) -> Option<String> {
    let stem = kit_path.file_stem()?.to_str()?;
    match stem.rfind('_') {
        Some(underscore) => {
            let variation = &stem[underscore + 1..];
            if variation.is_empty() {
                None
            } else {
                Some(format!("Midimap_{variation}.xml"))
            }
        }
        None => Some("midimap.xml".to_string()),
    }
}

fn serialize_metadata(name: &str, description: &str, metadata: &KitMetadata) -> String {
    let mut out = String::from("  <metadata>\n");
    if let Some(version) = &metadata.version {
        out.push_str(&format!("    <version>{}</version>\n", esc(version)));
    }
    out.push_str(&format!("    <title>{}</title>\n", esc(name)));
    if let Some(logo) = &metadata.logo {
        out.push_str(&format!("    <logo src=\"{}\"/>\n", esc(logo)));
    }
    if !description.is_empty() {
        out.push_str(&format!(
            "    <description>{}</description>\n",
            esc(description)
        ));
    }
    if let Some(license) = &metadata.license {
        out.push_str(&format!("    <license>{}</license>\n", esc(license)));
    }
    if let Some(notes) = &metadata.notes {
        out.push_str(&format!("    <notes>{}</notes>\n", esc(notes)));
    }
    if let Some(author) = &metadata.author {
        out.push_str(&format!("    <author>{}</author>\n", esc(author)));
    }
    if let Some(email) = &metadata.email {
        out.push_str(&format!("    <email>{}</email>\n", esc(email)));
    }
    if let Some(website) = &metadata.website {
        out.push_str(&format!("    <website>{}</website>\n", esc(website)));
    }
    if let Some(image) = &metadata.image {
        let map = image
            .map
            .as_deref()
            .map(|m| format!(" map=\"{}\"", esc(m)))
            .unwrap_or_default();
        out.push_str(&format!("    <image src=\"{}\"{map}>\n", esc(&image.src)));
        for clickmap in &image.clickmap {
            out.push_str(&format!(
                "      <clickmap colour=\"{}\" instrument=\"{}\"/>\n",
                esc(&clickmap.colour),
                esc(&clickmap.instrument)
            ));
        }
        out.push_str("    </image>\n");
    }
    out.push_str("  </metadata>\n");
    out
}

fn serialize_drumkit(drumkit: &DrumKit, instrument_files: &[String]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<drumkit samplerate=\"{}\" version=\"{}\">\n",
        drumkit.samplerate,
        esc(&drumkit.version)
    ));
    out.push_str(&serialize_metadata(
        &drumkit.name,
        &drumkit.description,
        &drumkit.metadata,
    ));
    out.push_str("  <channels>\n");
    for channel in &drumkit.channels {
        match &channel.title {
            Some(title) => {
                out.push_str(&format!(
                    "    <channel name=\"{}\">\n      <title>{}</title>\n    </channel>\n",
                    esc(&channel.name),
                    esc(title)
                ));
            }
            None => out.push_str(&format!("    <channel name=\"{}\"/>\n", esc(&channel.name))),
        }
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

fn serialize_instrument(
    inst: &Instrument,
    audio_paths: &[String],
    power_scale: Option<PowerScale>,
) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    // DrumGizmo requires both the `name` and `description` attributes on the
    // instrument node (dgxmlparser.cc), so `description` is always written,
    // even when empty.
    out.push_str(&format!(
        "<instrument version=\"{}\" name=\"{}\" description=\"{}\">\n",
        esc(&inst.version),
        esc(&inst.name),
        esc(&inst.description)
    ));
    out.push_str(&serialize_instrument_metadata(&inst.metadata));
    out.push_str("  <channels>\n");
    for channel in &inst.channels {
        out.push_str(&format!("    <channel name=\"{}\"/>\n", esc(&channel.name)));
    }
    out.push_str("  </channels>\n");

    let v2 = inst.is_v2();
    if !inst.samples.is_empty() {
        out.push_str("  <samples>\n");
        let mut audio_index = 0;
        for sample in &inst.samples {
            if v2 {
                // Sample powers are stored normalized to 0..1; map them back
                // into the kit file's original scale before writing, so a
                // slider edit lands in the file's own loudness range.
                let power = match power_scale {
                    Some(scale) => scale.to_file(sample.power),
                    None => sample.power,
                };
                out.push_str(&format!(
                    "    <sample name=\"{}\" power=\"{}\" normalized=\"{}\">",
                    esc(&sample.name),
                    power,
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
                    "      <audiofile channel=\"{}\" file=\"{}\" filechannel=\"{}\"/>",
                    esc(&audio.channel),
                    esc(file),
                    // A row without a chosen side saves as the left channel
                    // (DrumGizmo's default when filechannel is absent).
                    audio.file_channel.unwrap_or(0) + 1
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

/// The instrument `<metadata>` block (documented v2 format; DrumGizmo ignores
/// its contents, so it is preserved verbatim). Written only when at least one
/// field is present.
fn serialize_instrument_metadata(metadata: &InstrumentMetadata) -> String {
    let has_fields = metadata.title.is_some()
        || metadata.description.is_some()
        || metadata.license.is_some()
        || metadata.notes.is_some()
        || metadata.author.is_some()
        || metadata.email.is_some()
        || metadata.website.is_some()
        || metadata.image.is_some();
    if !has_fields {
        return String::new();
    }

    let mut out = String::from("  <metadata>\n");
    if let Some(title) = &metadata.title {
        out.push_str(&format!("    <title>{}</title>\n", esc(title)));
    }
    if let Some(description) = &metadata.description {
        out.push_str(&format!(
            "    <description>{}</description>\n",
            esc(description)
        ));
    }
    if let Some(license) = &metadata.license {
        out.push_str(&format!("    <license>{}</license>\n", esc(license)));
    }
    if let Some(notes) = &metadata.notes {
        out.push_str(&format!("    <notes>{}</notes>\n", esc(notes)));
    }
    if let Some(author) = &metadata.author {
        out.push_str(&format!("    <author>{}</author>\n", esc(author)));
    }
    if let Some(email) = &metadata.email {
        out.push_str(&format!("    <email>{}</email>\n", esc(email)));
    }
    if let Some(website) = &metadata.website {
        out.push_str(&format!("    <website>{}</website>\n", esc(website)));
    }
    if let Some(image) = &metadata.image {
        out.push_str(&format!(
            "    <image filename=\"{}\"/>\n",
            esc(&image.filename)
        ));
    }
    out.push_str("  </metadata>\n");
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
#[path = "save_tests.rs"]
mod tests;
