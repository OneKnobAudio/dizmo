use std::path::{Path, PathBuf};

use dizmo_kit::drumkit::InstrumentRef;
use dizmo_kit::{
    AudioFile, ChannelMap, DrumKit, Instrument, InstrumentChannel, KitChannel, KitMetadata,
    MidiMap, MidiMapEntry, Sample,
};

/// The editable kit: the drumkit plus each instrument kept paired with the
/// `InstrumentRef` that references it (so we always know its XML file path).
#[derive(Debug, Clone)]
pub struct EditorKit {
    /// Kit directory, once the user has picked one (Open / Save As).
    pub root_dir: Option<PathBuf>,
    /// Filename of the kit XML file, e.g. `"My Kit.xml"`. Derived from the kit
    /// name for new kits and preserved from the opened file for existing kits.
    pub kit_file_name: Option<String>,
    /// Resolved midimap filename (`midimap.xml` or `Midimap_<variation>.xml`),
    /// derived from the kit filename. `None` means no midimap is associated.
    pub default_midimap: Option<String>,
    pub drumkit: DrumKit,
    pub instruments: Vec<EditorInstrument>,
    pub midimap: MidiMap,
    pub dirty: bool,
}

#[derive(Debug, Clone)]
pub struct EditorInstrument {
    /// Path of the instrument XML file, relative to `drumkit.xml`.
    pub file: PathBuf,
    /// The drumkit.xml-side reference (channel map, chokes, group).
    pub reference: InstrumentRef,
    /// The instrument-side data (channels, samples).
    pub instrument: Instrument,
}

impl EditorKit {
    /// An empty v2.0 kit from the New Kit dialog (workflow step 1: channels).
    pub fn new_kit(name: &str, samplerate: f64, channels: &[String]) -> Self {
        let kit_channels: Vec<KitChannel> = channels
            .iter()
            .enumerate()
            .map(|(num, name)| KitChannel {
                name: name.clone(),
                num,
                title: None,
            })
            .collect();
        Self {
            root_dir: None,
            kit_file_name: Some(format!("{name}.xml")),
            default_midimap: Some("midimap.xml".into()),
            drumkit: DrumKit {
                version: "2.0".into(),
                samplerate,
                name: name.into(),
                description: String::new(),
                metadata: KitMetadata {
                    title: Some(name.into()),
                    ..KitMetadata::default()
                },
                channels: kit_channels,
                instrument_refs: Vec::new(),
            },
            instruments: Vec::new(),
            midimap: MidiMap::default(),
            dirty: true,
        }
    }

    /// Workflow step 2: add a new, unassigned instrument.
    pub fn add_instrument(&mut self, name: &str) -> usize {
        let id = self.instruments.len();
        let file = PathBuf::from(format!("inst_{}.xml", slug(name)));
        let instrument = Instrument {
            name: name.into(),
            description: String::new(),
            metadata: Default::default(),
            version: "2.0".into(),
            id,
            base_dir: PathBuf::new(),
            group: None,
            channel_map: Vec::new(),
            chokes: Vec::new(),
            channels: Vec::new(),
            samples: Vec::new(),
            velocities: Vec::new(),
        };
        let reference = InstrumentRef {
            name: name.into(),
            file: file.to_string_lossy().into_owned(),
            group: None,
            channel_map: Vec::new(),
            chokes: Vec::new(),
        };
        self.drumkit.instrument_refs.push(reference.clone());
        self.instruments.push(EditorInstrument {
            file,
            reference,
            instrument,
        });
        self.dirty = true;
        id
    }

    pub fn remove_instrument(&mut self, index: usize) {
        if index >= self.instruments.len() {
            return;
        }
        let name = self.instruments[index].reference.name.clone();
        self.instruments.remove(index);
        self.drumkit.instrument_refs.remove(index);
        self.midimap.entries.retain(|e| e.instrument != name);
        self.dirty = true;
    }

    pub fn rename_instrument(&mut self, index: usize, name: &str) {
        let Some(inst) = self.instruments.get_mut(index) else {
            return;
        };
        let old = inst.reference.name.clone();
        if old == name {
            return;
        }
        inst.reference.name = name.into();
        inst.instrument.name = name.into();
        for entry in &mut self.midimap.entries {
            if entry.instrument == old {
                entry.instrument = name.into();
            }
        }
        self.dirty = true;
    }

    /// Toggles whether a kit channel is assigned to this instrument. When
    /// assigned, an instrument channel and a 1:1 channel map are created; when
    /// unassigned, both are removed. Each kit channel can be assigned at most
    /// once per instrument.
    pub fn toggle_channel_assignment(&mut self, instrument: usize, channel_name: &str) {
        let Some(inst) = self.instruments.get_mut(instrument) else {
            return;
        };
        let already_assigned = inst
            .instrument
            .channels
            .iter()
            .any(|ch| ch.name == channel_name);
        if already_assigned {
            inst.instrument
                .channels
                .retain(|ch| ch.name != channel_name);
            inst.reference
                .channel_map
                .retain(|m| m.in_name != channel_name);
            for sample in &mut inst.instrument.samples {
                sample.audio_files.retain(|a| a.channel != channel_name);
            }
        } else {
            inst.instrument.channels.push(InstrumentChannel {
                name: channel_name.into(),
            });
            inst.reference.channel_map.push(ChannelMap {
                in_name: channel_name.into(),
                out_name: channel_name.into(),
                is_main: false,
            });
        }
        inst.instrument.channel_map = inst.reference.channel_map.clone();
        if let Some(reference) = self.drumkit.instrument_refs.get_mut(instrument) {
            reference.channel_map = inst.reference.channel_map.clone();
        }
        self.dirty = true;
    }

    /// Sets the `main` flag on the channel map for the given kit channel.
    pub fn set_channel_main(&mut self, instrument: usize, channel_name: &str, is_main: bool) {
        let Some(inst) = self.instruments.get_mut(instrument) else {
            return;
        };
        let Some(map) = inst
            .reference
            .channel_map
            .iter_mut()
            .find(|m| m.in_name == channel_name)
        else {
            return;
        };
        if map.is_main != is_main {
            map.is_main = is_main;
            inst.instrument.channel_map = inst.reference.channel_map.clone();
            if let Some(reference) = self.drumkit.instrument_refs.get_mut(instrument) {
                reference.channel_map = inst.reference.channel_map.clone();
            }
            self.dirty = true;
        }
    }

    /// Workflow step 3: import a WAV file as a new sample.
    ///
    /// Each sample holds exactly **one** audio file; that file feeds **every**
    /// channel of the instrument — one [`AudioFile`] row per instrument
    /// channel, all referencing the same WAV, with `file_channel` picking the
    /// channel at the same position inside the WAV (clamped to the WAV's
    /// channel count). The WAV is referenced in place; the copy into
    /// `<root>/<name>/samples/` happens during Save / Save As.
    pub fn import_sample(&mut self, instrument: usize, path: &Path) -> Result<usize, String> {
        let Some(inst) = self.instruments.get_mut(instrument) else {
            return Err("Instrument not found.".to_string());
        };
        if inst.instrument.channels.is_empty() {
            return Err(format!(
                "'{}' has no channels yet — assign kit channels to it before importing samples.",
                inst.reference.name
            ));
        }
        let reader = hound::WavReader::open(path)
            .map_err(|err| format!("'{}' is not a readable WAV file: {err}", path.display()))?;
        let wav_channels = usize::from(reader.spec().channels).max(1);

        let file = path.to_string_lossy().into_owned();
        let audio_files: Vec<AudioFile> = inst
            .instrument
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| AudioFile {
                channel: channel.name.clone(),
                file: file.clone(),
                file_channel: index.min(wav_channels - 1),
            })
            .collect();
        let name = unique_sample_name(&inst.instrument.samples, path);
        inst.instrument.samples.push(Sample {
            name,
            power: 0.5,
            normalized: true,
            audio_files,
        });
        self.dirty = true;
        Ok(inst.instrument.samples.len() - 1)
    }

    pub fn remove_sample(&mut self, instrument: usize, sample: usize) {
        let Some(inst) = self.instruments.get_mut(instrument) else {
            return;
        };
        if sample >= inst.instrument.samples.len() {
            return;
        }
        inst.instrument.samples.remove(sample);
        self.dirty = true;
    }

    pub fn set_sample_power(&mut self, instrument: usize, sample: usize, power: f32) {
        if let Some(s) = self
            .instruments
            .get_mut(instrument)
            .and_then(|inst| inst.instrument.samples.get_mut(sample))
        {
            s.power = power.clamp(0.0, 1.0);
            self.dirty = true;
        }
    }

    pub fn set_sample_normalized(&mut self, instrument: usize, sample: usize, normalized: bool) {
        if let Some(s) = self
            .instruments
            .get_mut(instrument)
            .and_then(|inst| inst.instrument.samples.get_mut(sample))
        {
            s.normalized = normalized;
            self.dirty = true;
        }
    }

    pub fn add_channel(&mut self, name: &str) -> usize {
        let num = self.drumkit.channels.len();
        self.drumkit.channels.push(KitChannel {
            name: name.into(),
            num,
            title: None,
        });
        self.dirty = true;
        num
    }

    pub fn rename_channel(&mut self, index: usize, name: &str) {
        let Some(ch) = self.drumkit.channels.get_mut(index) else {
            return;
        };
        let old = ch.name.clone();
        if old == name {
            return;
        }
        ch.name = name.into();
        for inst in &mut self.instruments {
            for ic in &mut inst.instrument.channels {
                if ic.name == old {
                    ic.name = name.into();
                }
            }
            for map in &mut inst.reference.channel_map {
                if map.in_name == old {
                    map.in_name = name.into();
                }
                if map.out_name == old {
                    map.out_name = name.into();
                }
            }
        }
        self.dirty = true;
    }

    pub fn remove_channel(&mut self, index: usize) {
        if index >= self.drumkit.channels.len() {
            return;
        }
        let name = self.drumkit.channels[index].name.clone();
        self.drumkit.channels.remove(index);
        for inst in &mut self.instruments {
            inst.instrument.channels.retain(|c| c.name != name);
            inst.reference.channel_map.retain(|m| m.out_name != name);
            inst.instrument.channel_map.retain(|m| m.out_name != name);
            for s in &mut inst.instrument.samples {
                s.audio_files.retain(|a| a.channel != name);
            }
        }
        self.dirty = true;
    }

    /// Adds a midimap row for `note` if it is not mapped yet.
    pub fn add_note(&mut self, note: u8) {
        if self.midimap.entries.iter().any(|e| e.note == note) {
            return;
        }
        self.midimap.entries.push(MidiMapEntry {
            note,
            instrument: String::new(),
        });
        self.midimap.entries.sort_by_key(|e| e.note);
        self.dirty = true;
    }

    pub fn unmap_note(&mut self, index: usize) {
        if index < self.midimap.entries.len() {
            self.midimap.entries.remove(index);
            self.dirty = true;
        }
    }
}

/// A sample name derived from the WAV file stem, deduplicated against the
/// instrument's existing sample names with a ` (n)` suffix.
fn unique_sample_name(samples: &[Sample], path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "sample".to_string());
    let mut name = stem.clone();
    let mut n = 2;
    while samples.iter().any(|sample| sample.name == name) {
        name = format!("{stem} ({n})");
        n += 1;
    }
    name
}

fn slug(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
