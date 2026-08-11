//! In-memory, editable kit model.
//!
//! The UI only talks to this module. File I/O (load/save round-trip) lands in
//! Phase 1 / Phase 3 of EDITOR_PLAN.md; the new-kit constructors below are
//! implemented so the skeleton UI is fully usable end to end.

use std::path::PathBuf;

use dizmo_kit::drumkit::InstrumentRef;
use dizmo_kit::{
    AudioFile, ChannelMap, DrumKit, Instrument, InstrumentChannel, KitChannel, MidiMap,
    MidiMapEntry, Sample,
};

/// The editable kit: the drumkit plus each instrument kept paired with the
/// `InstrumentRef` that references it (so we always know its XML file path).
#[derive(Debug)]
pub struct EditorKit {
    /// Kit directory, once the user has picked one (Open / Save As).
    pub root_dir: Option<PathBuf>,
    pub drumkit: DrumKit,
    pub instruments: Vec<EditorInstrument>,
    pub midimap: MidiMap,
    pub dirty: bool,
}

#[derive(Debug)]
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
            })
            .collect();
        Self {
            root_dir: None,
            drumkit: DrumKit {
                version: "2.0".into(),
                samplerate,
                name: name.into(),
                description: String::new(),
                default_midimap: Some("midimap.xml".into()),
                channels: kit_channels,
                instrument_refs: Vec::new(),
            },
            instruments: Vec::new(),
            midimap: MidiMap::default(),
            dirty: true,
        }
    }

    pub fn channel_names(&self) -> Vec<String> {
        self.drumkit.channels.iter().map(|c| c.name.clone()).collect()
    }

    /// Workflow step 2: add a new, unassigned instrument.
    pub fn add_instrument(&mut self, name: &str) -> usize {
        let id = self.instruments.len();
        let file = PathBuf::from(format!("inst_{}.xml", slug(name)));
        let channels: Vec<InstrumentChannel> = self
            .drumkit
            .channels
            .iter()
            .enumerate()
            .map(|(i, ch)| InstrumentChannel {
                name: ch.name.clone(),
                is_main: i == 0,
            })
            .collect();
        let instrument = Instrument {
            name: name.into(),
            description: String::new(),
            version: "2.0".into(),
            id,
            base_dir: PathBuf::new(),
            group: None,
            channel_map: Vec::new(),
            chokes: Vec::new(),
            channels,
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

    /// Workflow step 4: assign the instrument to a kit channel.
    pub fn assign_channel(&mut self, index: usize, channel: Option<usize>) {
        let Some(inst) = self.instruments.get_mut(index) else {
            return;
        };
        let Some(out) = channel.and_then(|i| self.drumkit.channels.get(i)) else {
            inst.reference.channel_map.clear();
            inst.instrument.channel_map.clear();
            self.dirty = true;
            return;
        };
        let map = ChannelMap {
            in_name: out.name.clone(),
            out_name: out.name.clone(),
            is_main: true,
        };
        inst.reference.channel_map = vec![map.clone()];
        inst.instrument.channel_map = vec![map];
        self.dirty = true;
    }

    /// Workflow step 3: add a sample with one audio file per instrument channel.
    pub fn add_sample(&mut self, instrument: usize, name: &str, wav: &str) -> usize {
        let Some(inst) = self.instruments.get_mut(instrument) else {
            return 0;
        };
        let audio_files: Vec<AudioFile> = inst
            .instrument
            .channels
            .iter()
            .map(|ch| AudioFile {
                channel: ch.name.clone(),
                file: wav.to_string(),
                file_channel: 0,
            })
            .collect();
        inst.instrument.samples.push(Sample {
            name: name.into(),
            power: 0.5,
            normalized: true,
            audio_files,
        });
        self.dirty = true;
        inst.instrument.samples.len() - 1
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
