//! Host-supplied diagram display scenes attached to the blocks that authored them.
//!
//! VuTeX does not own diagram semantics. A producer such as `davinci-diagram`
//! resolves its semantic scene into a [`davinci_scene::DisplayScene`], and the
//! host registers that display scene here under the Notedown block id it belongs
//! to. The layout pass then places the scene when it reaches that block.

use std::collections::HashMap;

use davinci_scene::DisplayScene;
use notedown_ir::NodeId;

/// One diagram display scene with its alternative text.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagramScene {
    /// Resolved display scene the producer lowered from the diagram source.
    pub scene: DisplayScene,
    /// Alternative text used by backends that cannot paint the scene.
    pub alt: String,
}

/// Diagram scenes keyed by the block that authored them.
///
/// Blocks without a registered scene are dropped by layout instead of being
/// guessed at, so a missing or failed lowering never invents geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagrams {
    scenes: HashMap<NodeId, DiagramScene>,
}

impl Diagrams {
    /// Empty diagram set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the scene authored by `node`, replacing any previous scene.
    pub fn insert(&mut self, node: NodeId, scene: DisplayScene, alt: impl Into<String>) -> Option<DiagramScene> {
        self.scenes.insert(node, DiagramScene { scene, alt: alt.into() })
    }

    /// Scene registered for `node`, if any.
    pub fn get(&self, node: NodeId) -> Option<&DiagramScene> {
        self.scenes.get(&node)
    }

    /// Number of registered scenes.
    pub fn len(&self) -> usize {
        self.scenes.len()
    }

    /// Whether no scene is registered.
    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }
}
