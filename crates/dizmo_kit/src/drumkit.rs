//! Parser for the `drumkit.xml` file.

use std::path::Path;

use roxmltree::Node;

use crate::xml::{attr, parse_bool, parse_f64, parse_u32, read_file, required_attr};
use crate::{ChannelMap, Choke, DEFAULT_CHOKETIME_MS, DEFAULT_SAMPLERATE, KitChannel, KitError};

/// The parsed contents of a `drumkit.xml`, before the referenced instrument
/// files have been loaded.
#[derive(Debug, Clone)]
pub struct DrumKit {
    pub version: String,
    pub samplerate: f64,
    pub name: String,
    pub description: String,
    pub metadata: KitMetadata,
    pub channels: Vec<KitChannel>,
    pub instrument_refs: Vec<InstrumentRef>,
}

/// A reference to an instrument inside the drumkit's `<instruments>` node.
#[derive(Debug, Clone)]
pub struct InstrumentRef {
    pub name: String,
    /// Path of the instrument XML file, relative to the `drumkit.xml`.
    pub file: String,
    pub group: Option<String>,
    pub channel_map: Vec<ChannelMap>,
    pub chokes: Vec<Choke>,
}

/// Drumkit metadata from the `<metadata>` node. All fields are optional and
/// mirror the DrumGizmo file format.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KitMetadata {
    pub version: Option<String>,
    pub title: Option<String>,
    /// Path/URL of the kit logo image (`<logo src="..."/>`).
    pub logo: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub notes: Option<String>,
    pub author: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub image: Option<KitImage>,
}

/// An `<image>` element inside `<metadata>` with an optional click map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KitImage {
    pub src: String,
    pub map: Option<String>,
    pub clickmap: Vec<ClickMap>,
}

/// One `<clickmap>` entry inside `<image>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClickMap {
    pub colour: String,
    pub instrument: String,
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
    let metadata = parse_metadata(&drumkit);
    let name = metadata
        .title
        .clone()
        .or_else(|| attr(&drumkit, "name"))
        .unwrap_or_default();
    let description = metadata
        .description
        .clone()
        .or_else(|| attr(&drumkit, "description"))
        .unwrap_or_default();

    let mut channels = Vec::new();
    for channel in children(drumkit, "channels", "channel") {
        let name = required_attr(&channel, "name", path)?;
        let title = channel
            .children()
            .find(|child| child.has_tag_name("title"))
            .and_then(|node| node.text())
            .map(str::to_owned);
        channels.push(KitChannel {
            name,
            num: channels.len(),
            title,
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
        metadata,
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

/// Parses the `<metadata>` node of a drumkit. Unknown elements are ignored.
fn parse_metadata(root: &Node) -> KitMetadata {
    let Some(metadata) = root.children().find(|child| child.has_tag_name("metadata")) else {
        return KitMetadata::default();
    };

    let mut result = KitMetadata::default();
    for child in metadata.children().filter(|c| c.is_element()) {
        match child.tag_name().name() {
            "version" => result.version = child.text().map(str::to_owned),
            "title" => result.title = child.text().map(str::to_owned),
            "logo" => result.logo = attr(&child, "src"),
            "description" => result.description = child.text().map(str::to_owned),
            "license" => result.license = child.text().map(str::to_owned),
            "notes" => result.notes = child.text().map(str::to_owned),
            "author" => result.author = child.text().map(str::to_owned),
            "email" => result.email = child.text().map(str::to_owned),
            "website" => result.website = child.text().map(str::to_owned),
            "image" => result.image = parse_image(&child),
            _ => {}
        }
    }
    result
}

fn parse_image(image: &Node) -> Option<KitImage> {
    let src = attr(image, "src")?;
    let map = attr(image, "map");
    let clickmap: Vec<ClickMap> = image
        .children()
        .filter(|child| child.has_tag_name("clickmap"))
        .map(|child| ClickMap {
            colour: attr(&child, "colour").unwrap_or_default(),
            instrument: attr(&child, "instrument").unwrap_or_default(),
        })
        .collect();
    Some(KitImage { src, map, clickmap })
}
