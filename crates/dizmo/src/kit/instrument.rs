//! Parser for DrumGizmo instrument XML files.

use std::path::{Path, PathBuf};

use super::format_major;
use super::xml::{attr, load_document, parse_bool, parse_f32, parse_u32, read_file, required_attr};
use super::{ChannelMap, Choke, KitError};

/// A single DrumGizmo instrument, resolved against the kit.
#[derive(Debug)]
pub struct Instrument {
    /// Canonical name (from the `drumkit.xml` reference).
    pub name: String,
    pub description: String,
    /// Instrument format version, e.g. `2.0` or `1.0`.
    pub version: String,
    /// Index of the instrument in the kit (assigned by [`super::Kit::load`]).
    pub id: usize,
    /// Directory containing the instrument XML file; sample paths are relative
    /// to this directory.
    pub base_dir: PathBuf,
    /// Drumkit group, e.g. `hihat` (set by [`super::Kit::load`]).
    pub group: Option<String>,
    /// `in` -> `out` channel mapping from the drumkit (set by [`super::Kit::load`]).
    pub channel_map: Vec<ChannelMap>,
    /// Instruments cut on trigger (set by [`super::Kit::load`]).
    pub chokes: Vec<Choke>,
    /// Instrument-level output channels.
    pub channels: Vec<InstrumentChannel>,
    /// The hit samples of the instrument.
    pub samples: Vec<Sample>,
    /// Velocity groups (version 1.0 instruments only).
    pub velocities: Vec<VelocityGroup>,
}

impl Instrument {
    /// Whether this uses the version 2.0 power-based sample selection.
    pub fn is_v2(&self) -> bool {
        format_major(&self.version) >= 2
    }

    /// The samples ordered by ascending power: the velocity-layer order used by
    /// version 2.0 kits.
    pub fn samples_by_power(&self) -> Vec<&Sample> {
        let mut samples: Vec<&Sample> = self.samples.iter().collect();
        samples.sort_by(|a, b| {
            a.power
                .partial_cmp(&b.power)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        samples
    }
}

/// A channel declared by the instrument's `<channels>` node.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentChannel {
    pub name: String,
    pub is_main: bool,
}

/// One hit sample: a single multi-channel WAV, one channel per output.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    /// Hit strength (0..=1, roughly); only meaningful for version 2.0.
    pub power: f32,
    /// Whether the sample is normalized (version 2.0 only).
    pub normalized: bool,
    pub audio_files: Vec<AudioFile>,
}

/// A single (channel, wav file) reference inside a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFile {
    /// Instrument channel this channel feeds.
    pub channel: String,
    /// Path of the WAV file, relative to the instrument XML file.
    pub file: String,
    /// Zero-based channel index inside the WAV (the XML is 1-based).
    pub file_channel: usize,
}

/// A velocity group (version 1.0 instruments only).
#[derive(Debug, Clone, PartialEq)]
pub struct VelocityGroup {
    pub lower: f32,
    pub upper: f32,
    pub sample_refs: Vec<VelocitySampleRef>,
}

/// A sample reference inside a velocity group.
#[derive(Debug, Clone, PartialEq)]
pub struct VelocitySampleRef {
    pub probability: f32,
    pub name: String,
}

pub fn parse_file(path: &Path) -> Result<Instrument, KitError> {
    parse_str(&read_file(path)?, path)
}

pub fn parse_str(text: &str, path: &Path) -> Result<Instrument, KitError> {
    let doc = load_document(text, path)?;
    let instrument = super::xml::root_element(&doc, "instrument", path)?;

    let name = required_attr(&instrument, "name", path)?;
    let version = attr(&instrument, "version").unwrap_or_else(|| "1.0".to_string());
    let description = attr(&instrument, "description").unwrap_or_default();
    let base_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let is_v2 = format_major(&version) >= 2;

    let channels = instrument
        .children()
        .find(|child| child.has_tag_name("channels"))
        .map(|channels_node| {
            channels_node
                .children()
                .filter(|child| child.has_tag_name("channel"))
                .map(|channel| {
                    let name = required_attr(&channel, "name", path)?;
                    let is_main = match attr(&channel, "main") {
                        Some(value) => parse_bool(&value, "attribute 'main' on <channel>", path)?,
                        None => false,
                    };
                    Ok(InstrumentChannel { name, is_main })
                })
                .collect::<Result<Vec<_>, KitError>>()
        })
        .transpose()?
        .unwrap_or_default();

    let samples = instrument
        .children()
        .find(|child| child.has_tag_name("samples"))
        .map(|samples_node| {
            samples_node
                .children()
                .filter(|child| child.has_tag_name("sample"))
                .map(|sample| {
                    let sample_name = required_attr(&sample, "name", path)?;
                    let power = if is_v2 {
                        parse_f32(
                            &required_attr(&sample, "power", path)?,
                            &format!("attribute 'power' on <sample> '{sample_name}'"),
                            path,
                        )?
                    } else {
                        0.0
                    };
                    let normalized = match attr(&sample, "normalized") {
                        Some(value) => parse_bool(
                            &value,
                            &format!("attribute 'normalized' on <sample> '{sample_name}'"),
                            path,
                        )?,
                        None => false,
                    };

                    let audio_files = sample
                        .children()
                        .filter(|child| child.has_tag_name("audiofile"))
                        .map(|audiofile| {
                            let channel = required_attr(&audiofile, "channel", path)?;
                            let file = required_attr(&audiofile, "file", path)?;
                            // The XML filechannel is 1-based; DrumGizmo stores it
                            // 0-based internally.
                            let file_channel_raw =
                                attr(&audiofile, "filechannel").unwrap_or_else(|| "1".to_string());
                            let file_channel = parse_u32(
                                &file_channel_raw,
                                &format!(
                                    "attribute 'filechannel' on <audiofile> for sample '{sample_name}'"
                                ),
                                path,
                            )?;
                            let file_channel = file_channel
                                .checked_sub(1)
                                .ok_or_else(|| {
                                    KitError::invalid(
                                        path,
                                        format!(
                                            "attribute 'filechannel' on <audiofile> for sample '{sample_name}'"
                                        ),
                                        "must be at least 1",
                                    )
                                })?;

                            Ok(AudioFile {
                                channel,
                                file,
                                file_channel: file_channel as usize,
                            })
                        })
                        .collect::<Result<Vec<_>, KitError>>()?;

                    Ok(Sample {
                        name: sample_name,
                        power,
                        normalized,
                        audio_files,
                    })
                })
                .collect::<Result<Vec<_>, KitError>>()
        })
        .transpose()?
        .unwrap_or_default();

    let velocities = if is_v2 {
        Vec::new()
    } else {
        instrument
            .children()
            .find(|child| child.has_tag_name("velocities"))
            .map(|velocities_node| {
                velocities_node
                    .children()
                    .filter(|child| child.has_tag_name("velocity"))
                    .map(|velocity| {
                        let lower = parse_f32(
                            &required_attr(&velocity, "lower", path)?,
                            "attribute 'lower' on <velocity>",
                            path,
                        )?;
                        let upper = parse_f32(
                            &required_attr(&velocity, "upper", path)?,
                            "attribute 'upper' on <velocity>",
                            path,
                        )?;
                        let sample_refs = velocity
                            .children()
                            .filter(|child| child.has_tag_name("sampleref"))
                            .map(|sample_ref| {
                                let name = required_attr(&sample_ref, "name", path)?;
                                let probability = parse_f32(
                                    &required_attr(&sample_ref, "probability", path)?,
                                    &format!(
                                        "attribute 'probability' on <sampleref> for sample '{name}'"
                                    ),
                                    path,
                                )?;
                                Ok(VelocitySampleRef { name, probability })
                            })
                            .collect::<Result<Vec<_>, KitError>>()?;
                        Ok(VelocityGroup {
                            lower,
                            upper,
                            sample_refs,
                        })
                    })
                    .collect::<Result<Vec<_>, KitError>>()
            })
            .transpose()?
            .unwrap_or_default()
    };

    Ok(Instrument {
        name,
        description,
        version,
        id: 0,
        base_dir,
        group: None,
        channel_map: Vec::new(),
        chokes: Vec::new(),
        channels,
        samples,
        velocities,
    })
}
