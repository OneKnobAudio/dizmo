//! Parser for the `midimap.xml` file, which maps MIDI notes to instruments.

use std::path::Path;

use super::KitError;
use super::xml::{load_document, parse_note, read_file, required_attr};

/// A parsed `midimap.xml`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MidiMap {
    pub entries: Vec<MidiMapEntry>,
}

impl MidiMap {
    /// The instrument name mapped to `note`, if any.
    pub fn instrument_for_note(&self, note: u8) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.note == note)
            .map(|entry| entry.instrument.as_str())
    }
}

/// One `<map>` entry: a MIDI note connected to an instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiMapEntry {
    pub note: u8,
    /// Name of the instrument in the drumkit.
    pub instrument: String,
}

pub fn parse_file(path: &Path) -> Result<MidiMap, KitError> {
    parse_str(&read_file(path)?, path)
}

pub fn parse_str(text: &str, path: &Path) -> Result<MidiMap, KitError> {
    let doc = load_document(text, path)?;
    let midimap = super::xml::root_element(&doc, "midimap", path)?;

    let entries = midimap
        .children()
        .filter(|child| child.has_tag_name("map"))
        .map(|map| {
            let note = parse_note(
                &required_attr(&map, "note", path)?,
                "attribute 'note' on <map>",
                path,
            )?;
            let instrument = required_attr(&map, "instr", path)?;
            Ok(MidiMapEntry { note, instrument })
        })
        .collect::<Result<Vec<_>, KitError>>()?;

    Ok(MidiMap { entries })
}
