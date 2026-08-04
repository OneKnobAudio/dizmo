//! DrumGizmo kit loading: drumkit XML, instrument XML and midimap XML parsing.
//!
//! The data model mirrors the DrumGizmo on-disk format (see
//! https://www.drumgizmo.org/wiki/doku.php?id=documentation:file_formats):
//!
//! - A kit is described by a `drumkit.xml` that references one instrument XML
//!   file per instrument, plus (optionally) a `midimap.xml`.
//! - Instrument XML files define the sample hits. Version 2.0 instruments use a
//!   `power` value per sample (velocity layering); version 1.0 instruments use
//!   `<velocities>` groups instead.
//! - Each hit is one WAV file that may contain several channels (`filechannel`
//!   picks one, 1-based in XML). Every channel maps to a kit output channel.
//!
//! All parsing happens off the audio thread. The engine only reads the
//! immutable data built by [`Kit::load`].

use std::path::{Path, PathBuf};

pub mod drumkit;
pub mod instrument;
pub mod midimap;
pub mod samples;

mod xml;

pub use drumkit::DrumKit;
pub use instrument::{
    AudioFile, Instrument, InstrumentChannel, Sample, VelocityGroup, VelocitySampleRef,
};
pub use midimap::{MidiMap, MidiMapEntry};
pub use samples::{DecodedFile, SampleBank, SampleError, load_samples};

/// The default sample rate used when a `drumkit.xml` does not declare one.
pub const DEFAULT_SAMPLERATE: f64 = 44100.0;

/// The default choke time in milliseconds (matches DrumGizmo's default).
pub const DEFAULT_CHOKETIME_MS: u32 = 68;

/// Errors that can occur while loading a DrumGizmo kit.
#[derive(Debug, thiserror::Error)]
pub enum KitError {
    #[error("failed to read '{path}': {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse '{path}': {message}")]
    Parse { path: PathBuf, message: String },

    #[error("missing {what} in '{path}'")]
    Missing { path: PathBuf, what: String },

    #[error("invalid {what} in '{path}': {value:?}")]
    Invalid {
        path: PathBuf,
        what: String,
        value: String,
    },
}

impl KitError {
    fn missing(path: &Path, what: impl Into<String>) -> Self {
        Self::Missing {
            path: path.to_path_buf(),
            what: what.into(),
        }
    }

    fn invalid(path: &Path, what: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            what: what.into(),
            value: value.into(),
        }
    }
}

/// A fully resolved DrumGizmo kit, ready for the engine to use.
#[derive(Debug)]
pub struct Kit {
    /// The `version` attribute of the `drumkit.xml` (x.y.z versioning).
    pub version: String,
    pub name: String,
    pub description: String,
    /// The kit's declared sample rate (defaults to 44100 Hz).
    pub samplerate: f64,
    /// Directory containing the `drumkit.xml`. Instrument and sample paths are
    /// relative to this directory.
    pub root_dir: PathBuf,
    /// The output channels declared by the kit.
    pub channels: Vec<KitChannel>,
    /// The instruments, in drumkit declaration order (their `id` is the index).
    pub instruments: Vec<Instrument>,
    /// Relative path to a `midimap.xml` bundled with the kit, if any.
    pub default_midimap: Option<String>,
}

impl Kit {
    /// Loads a kit from its `drumkit.xml` file, resolving every referenced
    /// instrument XML file relative to the drumkit file.
    pub fn load(path: impl AsRef<Path>) -> Result<Kit, KitError> {
        let path = path.as_ref();
        let drumkit = drumkit::parse_file(path)?;
        let root_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();

        let mut instruments = Vec::with_capacity(drumkit.instrument_refs.len());
        for (id, reference) in drumkit.instrument_refs.iter().enumerate() {
            let file = root_dir.join(&reference.file);
            let mut instrument = instrument::parse_file(&file)?;
            // The drumkit reference name is canonical: it is what midimap.xml
            // and <choke> nodes refer to.
            instrument.id = id;
            instrument.name = reference.name.clone();
            instrument.group = reference.group.clone();
            instrument.channel_map = reference.channel_map.clone();
            instrument.chokes = reference.chokes.clone();
            instruments.push(instrument);
        }

        Ok(Kit {
            version: drumkit.version,
            name: drumkit.name,
            description: drumkit.description,
            samplerate: drumkit.samplerate,
            root_dir,
            channels: drumkit.channels,
            instruments,
            default_midimap: drumkit.default_midimap,
        })
    }

    /// Looks up an instrument by its canonical name.
    pub fn instrument(&self, name: &str) -> Option<&Instrument> {
        self.instruments
            .iter()
            .find(|instrument| instrument.name == name)
    }

    /// Loads a `midimap.xml` file. The path is resolved relative to the kit
    /// root directory unless it is already absolute.
    pub fn load_midimap(&self, path: impl AsRef<Path>) -> Result<MidiMap, KitError> {
        let path = path.as_ref();
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_dir.join(path)
        };
        midimap::parse_file(&resolved)
    }
}

/// A single output channel declared by the kit's `<channels>` node.
#[derive(Debug, Clone, PartialEq)]
pub struct KitChannel {
    pub name: String,
    /// Index of the channel in the kit's channel list (0-based).
    pub num: usize,
}

/// A `<channelmap>` inside a drumkit `<instrument>` node: connects an
/// instrument channel (`in`) to a kit output channel (`out`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelMap {
    pub in_name: String,
    pub out_name: String,
    /// Whether this is a main channel for the instrument (bleed control).
    pub is_main: bool,
}

/// A `<choke>` inside a drumkit `<instrument>` node.
#[derive(Debug, Clone, PartialEq)]
pub struct Choke {
    /// Name of the instrument that gets cut on trigger.
    pub instrument: String,
    /// Fade-out time in milliseconds.
    pub choketime_ms: u32,
}

/// The major part of a DrumGizmo version string (e.g. `2` for `2.1.0`).
pub(crate) fn format_major(version: &str) -> u32 {
    version
        .trim()
        .split('.')
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(1)
}
