//! Host-side Mermaid lowering into resolved DaVinci display scenes.
//!
//! `notedown-formats` lowers an explicit Mermaid fence to `Block::Diagram`, so
//! this adapter lowers those diagram blocks through `davinci-diagram` and
//! registers each display scene under the block id that authored it. Layout then
//! places the scene instead of the source text. A failed lowering registers no
//! scene, which keeps the authored source visible and never invents diagram
//! geometry.

use davinci_diagram::mermaid_to_scene;
use notedown_ir::{Block, DocumentGraph};
use vutex_layout::Diagrams;

use crate::document::DiagnosticRecord;

/// Alternative text carried by every lowered Mermaid diagram.
pub const MERMAID_ALT: &str = "Mermaid diagram";

/// Diagnostic code for a scene whose canvas cannot be placed.
pub const DIAGRAM_INVALID_CANVAS: &str = "VUTEX_DIAGRAM_INVALID_CANVAS";

/// Display scenes lowered from Mermaid diagram blocks together with their diagnostics.
#[derive(Debug)]
pub struct LoweredDiagrams {
    /// Display scenes keyed by the diagram block that authored them.
    pub diagrams: Diagrams,
    /// Lowering diagnostics in document order.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// Lower every Mermaid diagram block in `graph` into a diagram scene.
///
/// Only `Block::Diagram` blocks whose language tag is `mermaid` are considered.
/// Fatal lowering failures and non-fatal layout loss both become diagnostics;
/// a block with a fatal failure or an unplaceable canvas keeps no scene, so
/// layout falls back to the authored source text.
pub fn lower_mermaid_diagrams(graph: &DocumentGraph, path: &str) -> LoweredDiagrams {
    let mut lowered = LoweredDiagrams { diagrams: Diagrams::new(), diagnostics: Vec::new() };
    for node in &graph.blocks {
        let Block::Diagram { language, source } = &node.block
        else {
            continue;
        };
        if !language.trim().eq_ignore_ascii_case("mermaid") {
            continue;
        }
        match mermaid_to_scene(source) {
            Ok(diagram) => {
                lowered.diagnostics.extend(diagram.diagnostics.iter().map(|diagnostic| DiagnosticRecord {
                    code: diagnostic.code().as_str().to_string(),
                    severity: "warning".to_string(),
                    message: diagnostic.message().to_string(),
                    stage: "diagram".to_string(),
                    path: Some(path.to_string()),
                }));
                let size = diagram.scene.size;
                if size.width <= 0.0 || size.height <= 0.0 {
                    lowered.diagnostics.push(DiagnosticRecord {
                        code: DIAGRAM_INVALID_CANVAS.to_string(),
                        severity: "error".to_string(),
                        message: format!("Mermaid diagram lowered with a non-positive canvas: {size:?}"),
                        stage: "diagram".to_string(),
                        path: Some(path.to_string()),
                    });
                    continue;
                }
                lowered.diagrams.insert(node.id, diagram.scene, MERMAID_ALT);
            }
            Err(error) => lowered.diagnostics.push(DiagnosticRecord {
                code: error.code().to_string(),
                severity: "error".to_string(),
                message: error.message().to_string(),
                stage: "diagram".to_string(),
                path: Some(path.to_string()),
            }),
        }
    }
    lowered
}
