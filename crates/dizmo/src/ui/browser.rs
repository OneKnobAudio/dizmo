//! A minimal in-window file browser for picking a DrumGizmo `drumkit.xml`.

use crate::ui::{
    ACCENT, CARD_BG, CARD_BORDER, FIELD_BG, FIELD_BORDER, FIELD_HOVER, Message, TEXT, TEXT_DIM,
};
use iced::core::border::Radius;
use iced::widget::{Button, Space, button, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Border, Color, Length, widget::Id};
use nice_plug_iced::iced;
use std::path::PathBuf;

/// One entry shown in the kit browser: a directory or a `*.xml` file.
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub path: PathBuf,
}

/// The in-window DrumGizmo kit picker.
pub struct Browser {
    open: bool,
    cwd: PathBuf,
    entries: Vec<Entry>,
    scroll: Id,
}

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

impl Browser {
    /// Creates a browser rooted at the user's home directory.
    pub fn new() -> Self {
        Self {
            open: false,
            cwd: home_dir(),
            entries: Vec::new(),
            scroll: Id::new("dizmo-kit-browser"),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the browser and lists the current directory.
    pub fn open(&mut self) {
        self.open = true;
        self.refresh();
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Refreshes the entry list from the current directory.
    fn refresh(&mut self) {
        self.entries = list_entries(&self.cwd);
    }

    /// Navigates to the parent directory.
    pub fn up(&mut self) {
        if let Some(parent) = self.cwd.parent().filter(|p| *p != self.cwd) {
            self.cwd = parent.to_path_buf();
            self.refresh();
        }
    }

    /// Navigates to the home directory.
    pub fn home(&mut self) {
        self.cwd = home_dir();
        self.refresh();
    }

    /// Opens the entry at `index`. Directories are entered and `None` is
    /// returned; for a kit file the path is returned so the caller can load it.
    pub fn open_entry(&mut self, index: usize) -> Option<PathBuf> {
        let entry = self.entries.get(index)?;
        if entry.is_dir {
            self.cwd = entry.path.clone();
            self.refresh();
            None
        } else {
            Some(entry.path.clone())
        }
    }

    /// The scroll anchor for the entry list, kept stable across redraws.
    pub fn scroll_id(&self) -> &Id {
        &self.scroll
    }
}

/// The full-window modal overlay: a dim backdrop plus a centered panel with a
/// header row and a scrollable list of entries. `scale` is the uniform UI zoom.
pub fn view(browser: &Browser, scale: f32) -> iced::widget::Stack<'_, Message> {
    let s = scale;
    let mut list =
        column![entry_row("..", true, Message::BrowserUp, s)].spacing(2.0 * s);
    for (index, entry) in browser.entries().iter().enumerate() {
        list = list.push(entry_row(
            &entry.name,
            entry.is_dir,
            Message::BrowserEntry(index),
            s,
        ));
    }

    let list = scrollable(container(list).padding(4.0 * s).width(Length::Fill))
        .id(browser.scroll_id().clone())
        .height(Length::Fill);

    let header = row![
        text(truncate(browser.cwd().display().to_string(), 56))
            .size(11.0 * s)
            .color(TEXT_DIM),
        Space::new().width(Length::Fill),
        button("HOME")
            .on_press(Message::BrowserHome)
            .style(nav_button),
        button("UP").on_press(Message::BrowserUp).style(nav_button),
        button("CLOSE")
            .on_press(Message::BrowserClose)
            .style(nav_button),
    ]
    .align_y(Alignment::Center)
    .spacing(8.0 * s);

    let panel = container(
        column![header, iced::widget::rule::horizontal(1), list].spacing(8.0 * s),
    )
    .width(560.0 * s)
    .height(400.0 * s)
    .padding(12.0 * s)
    .style(panel_style);

    let backdrop = mouse_area(container(
        Space::new().width(Length::Fill).height(Length::Fill),
    ))
    .on_press(Message::BrowserClose);

    iced::widget::Stack::with_children([
        container(backdrop)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(backdrop_style)
            .into(),
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    ])
}

/// A single selectable row in the browser list.
fn entry_row<'a>(name: &'a str, is_dir: bool, on_press: Message, s: f32) -> Button<'a, Message> {
    let icon = if is_dir { "▸ " } else { "  " };
    let color = if is_dir { TEXT } else { TEXT_DIM };
    let content = text(format!("{icon}{name}"))
        .size(12.0 * s)
        .color(color)
        .width(Length::Fill);
    button(row![content].width(Length::Fill))
        .on_press(on_press)
        .style(entry_button)
}

fn panel_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(CARD_BG)),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(6.0),
        },
        ..Default::default()
    }
}

fn backdrop_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let _ = theme;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(Color::from_rgba8(0, 0, 0, 0.55))),
        ..Default::default()
    }
}

fn nav_button(theme: &iced::Theme, status: button::Status) -> iced::widget::button::Style {
    let _ = theme;
    let (bg, border) = match status {
        button::Status::Hovered => (FIELD_HOVER, ACCENT),
        _ => (FIELD_BG, FIELD_BORDER),
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: TEXT,
        border: Border {
            color: border,
            width: 1.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

fn entry_button(theme: &iced::Theme, status: button::Status) -> iced::widget::button::Style {
    let _ = theme;
    let bg = match status {
        button::Status::Hovered => iced::Background::Color(FIELD_HOVER),
        button::Status::Pressed => iced::Background::Color(ACCENT),
        _ => iced::Background::Color(Color::TRANSPARENT),
    };
    iced::widget::button::Style {
        background: Some(bg),
        text_color: TEXT,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::from(4.0),
        },
        ..Default::default()
    }
}

/// The directory the browser starts in: the user's home directory.
fn home_dir() -> PathBuf {
    #[allow(deprecated)]
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Lists the current directory: directories first, then `*.xml` files, both
/// sorted case-insensitively.
fn list_entries(cwd: &PathBuf) -> Vec<Entry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(read) = std::fs::read_dir(cwd) {
        for entry in read.flatten() {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_dir {
                dirs.push(Entry {
                    name,
                    is_dir: true,
                    path: entry.path(),
                });
            } else if entry
                .path()
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("xml"))
                .unwrap_or(false)
            {
                files.push(Entry {
                    name,
                    is_dir: false,
                    path: entry.path(),
                });
            }
        }
    }
    dirs.sort_by_key(|a| a.name.to_lowercase());
    files.sort_by_key(|a| a.name.to_lowercase());
    dirs.into_iter().chain(files).collect()
}

/// Truncates `text` to roughly `max_chars` characters, appending an ellipsis.
fn truncate(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
