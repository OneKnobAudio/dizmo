#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Debug, Clone)]
pub struct Node {
    id: NodeId,
    name: String,
    kind: NodeKind,
    expanded: bool,
    children: Vec<Node>,
}

#[derive(Debug, Clone, Copy)]
pub enum NodeKind {
    Folder,
    File,
}
