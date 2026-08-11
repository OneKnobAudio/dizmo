//! Small shared helpers for reading DrumGizmo XML documents.

use std::path::Path;

use roxmltree::{Document, Node};

use crate::KitError;

/// Reads an XML file to a string, wrapping IO errors with the file path.
pub(crate) fn read_file(path: &Path) -> Result<String, KitError> {
    std::fs::read_to_string(path).map_err(|source| KitError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Parses the XML text into a document, wrapping errors with the file path.
pub(crate) fn load_document<'text>(
    text: &'text str,
    path: &Path,
) -> Result<Document<'text>, KitError> {
    Document::parse(text).map_err(|error| KitError::Parse {
        path: path.to_path_buf(),
        message: format!("XML parse error: {error}"),
    })
}

/// Returns the root element, checking it has the expected tag name.
pub(crate) fn root_element<'a, 'i>(
    doc: &'a Document<'i>,
    expected: &str,
    path: &Path,
) -> Result<Node<'a, 'i>, KitError> {
    let root = doc.root_element();
    if root.tag_name().name() == expected {
        Ok(root)
    } else {
        Err(KitError::Parse {
            path: path.to_path_buf(),
            message: format!(
                "expected <{expected}> root element, found <{}>",
                root.tag_name().name()
            ),
        })
    }
}

/// The value of an attribute, if present.
pub(crate) fn attr(node: &Node, name: &str) -> Option<String> {
    node.attribute(name).map(str::to_owned)
}

/// The value of a required attribute.
pub(crate) fn required_attr(node: &Node, name: &str, path: &Path) -> Result<String, KitError> {
    attr(node, name).ok_or_else(|| {
        KitError::missing(
            path,
            format!("attribute '{name}' on <{}>", node.tag_name().name()),
        )
    })
}

/// Text content of the first child element with the given tag name.
pub(crate) fn child_text(node: &Node, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.has_tag_name(name))
        .and_then(|child| child.text())
        .map(str::to_owned)
}

/// Text content of `<metadata><name>` inside the given root node.
pub(crate) fn metadata_text(root: &Node, name: &str) -> Option<String> {
    root.children()
        .find(|child| child.has_tag_name("metadata"))
        .and_then(|metadata| child_text(&metadata, name))
}

/// The `src` attribute of `<metadata><name>` inside the given root node.
pub(crate) fn metadata_attr(root: &Node, name: &str) -> Option<String> {
    root.children()
        .find(|child| child.has_tag_name("metadata"))
        .and_then(|metadata| metadata.children().find(|child| child.has_tag_name(name)))
        .and_then(|child| child.attribute("src"))
        .map(str::to_owned)
}

pub(crate) fn parse_f64(value: &str, what: &str, path: &Path) -> Result<f64, KitError> {
    value
        .trim()
        .parse()
        .map_err(|_| KitError::invalid(path, what, value))
}

pub(crate) fn parse_f32(value: &str, what: &str, path: &Path) -> Result<f32, KitError> {
    value
        .trim()
        .parse()
        .map_err(|_| KitError::invalid(path, what, value))
}

pub(crate) fn parse_u32(value: &str, what: &str, path: &Path) -> Result<u32, KitError> {
    value
        .trim()
        .parse()
        .map_err(|_| KitError::invalid(path, what, value))
}

/// Parses a MIDI note value (0..=127).
pub(crate) fn parse_note(value: &str, what: &str, path: &Path) -> Result<u8, KitError> {
    let note: u8 = value
        .trim()
        .parse()
        .map_err(|_| KitError::invalid(path, what, value))?;
    if note > 127 {
        return Err(KitError::invalid(path, what, value));
    }
    Ok(note)
}

/// DrumGizmo booleans are strictly `true` or `false`.
pub(crate) fn parse_bool(value: &str, what: &str, path: &Path) -> Result<bool, KitError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(KitError::invalid(path, what, value)),
    }
}
