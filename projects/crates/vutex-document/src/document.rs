use crate::node::NodeId;

/// Root language-neutral document container.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub root: NodeId,
}
