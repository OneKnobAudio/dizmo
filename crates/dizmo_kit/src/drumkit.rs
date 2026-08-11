//! Parser for the `drumkit.xml` file.

use std::path::Path;

use roxmltree::Node;

use crate::xml::{
    attr, metadata_attr, metadata_text, parse_bool, parse_f64, parse_u32, read_file, required_attr,
};
use crate::{ChannelMap, Choke, DEFAULT_CHOKETIME_MS, DEFAULT_SAMPLERATE, KitChannel, KitError};

/// The parsed contents of a `drumkit.xml`, before the referenced instrument
/// files have been loaded.
#[derive(Debug)]
pub struct DrumKit {
    pub version: String,
    pub samplerate: f64,
    pub name: String,
    pub description: String,
    /// Relative path to the kit's default `midimap.xml`, if declared.
    pub default_midimap: Option<String>,
    pub channels: Vec<KitChannel>,
    pub instrument_refs: Vec<InstrumentRef>,
}

/// A reference to an instrument inside the drumkit's `<instruments>` node.
#[derive(Debug)]
pub struct InstrumentRef {
    pub name: String,
    /// Path of the instrument XML file, relative to the `drumkit.xml`.
    pub file: String,
    pub group: Option<String>,
    pub channel_map: Vec<ChannelMap>,
    pub chokes: Vec<Choke>,
}

pub fn parse_file(path: &Path) -> Result<DrumKit, KitError> {
    parse_str(&read_file(path)?, path)
}

pub fn parse_str(text: &str, path: &Path) -> Result<DrumKit, KitError> {
    let doc = crate::xml::load_document(text, path)?;
    let drumkit = crate::xml::root_element(&doc, "drumkit", path)?;

    let version = attr(&drumkit, "version").unwrap_or_else(|| "1.0".to_string());
    let samplerate = match attr(&drumkit, "samplerate") {
        Some(value) => parse_f64(&value, "attribute 'samplerate' on <drumkit>", path)?,
        None => DEFAULT_SAMPLERATE,
    };

    // The modern format stores these in <metadata>; the old format used
    // attributes directly on the <drumkit> node.
    let name = metadata_text(&drumkit, "title")
        .or_else(|| attr(&drumkit, "name"))
        .unwrap_or_default();
    let description = metadata_text(&drumkit, "description")
        .or_else(|| attr(&drumkit, "description"))
        .unwrap_or_default();
    let default_midimap = metadata_attr(&drumkit, "defaultmidimap");

    let mut channels = Vec::new();
    for channel in children(drumkit, "channels", "channel") {
        let name = required_attr(&channel, "name", path)?;
        channels.push(KitChannel {
            name,
            num: channels.len(),
        });
    }

    let mut instrument_refs = Vec::new();
    for instrument in children(drumkit, "instruments", "instrument") {
        let name = required_attr(&instrument, "name", path)?;
        let file = required_attr(&instrument, "file", path)?;
        let group = attr(&instrument, "group");

        let mut channel_map = Vec::new();
        for channelmap in instrument
            .children()
            .filter(|child| child.has_tag_name("channelmap"))
        {
            let in_name = required_attr(&channelmap, "in", path)?;
            let out_name = required_attr(&channelmap, "out", path)?;
            let is_main = match attr(&channelmap, "main") {
                Some(value) => parse_bool(&value, "attribute 'main' on <channelmap>", path)?,
                None => false,
            };
            channel_map.push(ChannelMap {
                in_name,
                out_name,
                is_main,
            });
        }

        let chokes = parse_chokes(instrument, path)?;

        instrument_refs.push(InstrumentRef {
            name,
            file,
            group,
            channel_map,
            chokes,
        });
    }

    Ok(DrumKit {
        version,
        samplerate,
        name,
        description,
        default_midimap,
        channels,
        instrument_refs,
    })
}

/// Iterates the elements named `child_name` inside the first element named
/// `parent_name` (an absent parent yields no children).
fn children<'a, 'i>(
    parent: Node<'a, 'i>,
    parent_name: &str,
    child_name: &str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    parent
        .children()
        .find(|child| child.has_tag_name(parent_name))
        .into_iter()
        .flat_map(move |node| node.children())
        .filter(move |child| child.has_tag_name(child_name))
}

/// Parses the `<chokes>` node of a drumkit instrument (at most one allowed).
fn parse_chokes(instrument: Node, path: &Path) -> Result<Vec<Choke>, KitError> {
    let choke_nodes: Vec<Node> = instrument
        .children()
        .filter(|child| child.has_tag_name("chokes"))
        .collect();

    if choke_nodes.len() > 1 {
        return Err(KitError::Parse {
            path: path.to_path_buf(),
            message: "at most one <chokes> node allowed per instrument".to_string(),
        });
    }

    let Some(chokes_node) = choke_nodes.first() else {
        return Ok(Vec::new());
    };

    chokes_node
        .children()
        .filter(|child| child.has_tag_name("choke"))
        .map(|choke| {
            let instrument = required_attr(&choke, "instrument", path)?;
            let choketime_ms = match attr(&choke, "choketime") {
                Some(value) => parse_u32(
                    &value,
                    &format!("attribute 'choketime' on <choke> for instrument '{instrument}'"),
                    path,
                )?,
                None => DEFAULT_CHOKETIME_MS,
            };
            Ok(Choke {
                instrument,
                choketime_ms,
            })
        })
        .collect()
}
