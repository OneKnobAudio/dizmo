use iced::keyboard;

use crate::ui::treeview::NodeId;

mod treeview;

#[derive(Debug, Clone)]
enum Message {
    Toggle(NodeId),
    Select(NodeId),
}
