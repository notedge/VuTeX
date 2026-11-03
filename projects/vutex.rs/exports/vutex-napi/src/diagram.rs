//! Host-side Mermaid, DOT, and PlantUML lowering into resolved DaVinci display
//! scenes.
//!
//! `notedown-formats` lowers an explicit `mermaid`, `dot`, or `plantuml` fence
//! to `Block::Diagram`, so this adapter lowers those blocks through
//! `davinci-diagram` and registers each display scene under the block id that
//! authored it. Layout then places the scene instead of the source text. A
//! failed lowering registers no scene, which keeps the authored source visible
//! and never invents diagram geometry. Rejections carry the byte range the
//! producer reported, so hosts can point at the rejected source.

use davinci_diagram::{DiagramError, LoweredDiagram, dot_to_scene, mermaid_to_scene, plantuml_to_scene};
use davinci_types::{Diagnostic, DiagnosticLocation};
use notedown_ir::{Block, DocumentGraph};
use vutex_layout::Diagrams;

use crate::document::{DiagnosticRecord, DiagnosticSpan};

/// Alternative text carried by every lowered Mermaid diagram.
pub const MERMAID_ALT: &str = "Mermaid diagram";

/// Alternative text carried by every lowered DOT graph.
pub const DOT_ALT: &str = "DOT graph";

/// Alternative text carried by every lowered PlantUML diagram.
pub const PLANTUML_ALT: &str = "PlantUML diagram";

/// Diagnostic code for a scene whose canvas cannot be placed.
pub const DIAGRAM_INVALID_CANVAS: &str = "VUTEX_DIAGRAM_INVALID_CANVAS";

/// Display scenes lowered from diagram blocks together with their diagnostics.
#[derive(Debug)]
pub struct LoweredDiagrams {
    /// Display scenes keyed by the diagram block that authored them.
    pub diagrams: Diagrams,
    /// Lowering diagnostics in document order.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// One diagram frontend accepted from a `Block::Diagram` language tag.
struct Frontend {
    /// Lowering entry point that resolves the authored source.
    lower: fn(&str) -> Result<LoweredDiagram, DiagramError>,
    /// Alternative text used by backends that cannot paint the scene.
    alt: &'static str,
}

/// Resolve the frontend for a fence language tag, or `None` when no diagrams
/// library lowers that language.
fn frontend_for(language: &str) -> Option<Frontend> {
    let language = language.trim();
    if language.eq_ignore_ascii_case("mermaid") {
        Some(Frontend { lower: mermaid_to_scene, alt: MERMAID_ALT })
    }
    else if language.eq_ignore_ascii_case("dot") {
        Some(Frontend { lower: dot_to_scene, alt: DOT_ALT })
    }
    else if language.eq_ignore_ascii_case("plantuml") {
        Some(Frontend { lower: plantuml_to_scene, alt: PLANTUML_ALT })
    }
    else {
        None
    }
}

/// Extract the primary source byte range a producer reported, if any.
fn record_span(diagnostic: &Diagnostic) -> Option<DiagnosticSpan> {
    let DiagnosticLocation::Text { range, .. } = diagnostic.primary()?.location()
    else {
        return None;
    };
    Some(DiagnosticSpan { start: range.start() as usize, end: range.end() as usize })
}

/// Lower every known diagram block in `graph` into a diagram scene.
///
/// Only `Block::Diagram` blocks whose language tag is `mermaid`, `dot`, or
/// `plantuml` are considered. Fatal lowering failures and non-fatal layout loss both become
/// diagnostics; a block with a fatal failure or an unplaceable canvas keeps no
/// scene, so layout falls back to the authored source text.
pub fn lower_diagrams(graph: &DocumentGraph, path: &str) -> LoweredDiagrams {
    let mut lowered = LoweredDiagrams { diagrams: Diagrams::new(), diagnostics: Vec::new() };
    for node in &graph.blocks {
        let Block::Diagram { language, source } = &node.block
        else {
            continue;
        };
        let Some(frontend) = frontend_for(language)
        else {
            continue;
        };
        match (frontend.lower)(source) {
            Ok(diagram) => {
                lowered.diagnostics.extend(diagram.diagnostics.iter().map(|diagnostic| DiagnosticRecord {
                    code: diagnostic.code().as_str().to_string(),
                    severity: "warning".to_string(),
                    message: diagnostic.message().to_string(),
                    stage: "diagram".to_string(),
                    path: Some(path.to_string()),
                    span: record_span(diagnostic),
                }));
                let size = diagram.scene.size;
                if size.width <= 0.0 || size.height <= 0.0 {
                    lowered.diagnostics.push(DiagnosticRecord {
                        code: DIAGRAM_INVALID_CANVAS.to_string(),
                        severity: "error".to_string(),
                        message: format!("diagram lowered with a non-positive canvas: {size:?}"),
                        stage: "diagram".to_string(),
                        path: Some(path.to_string()),
                        span: None,
                    });
                    continue;
                }
                lowered.diagrams.insert(node.id, diagram.scene, frontend.alt);
            }
            Err(error) => lowered.diagnostics.push(DiagnosticRecord {
                code: error.code().to_string(),
                severity: "error".to_string(),
                message: error.message().to_string(),
                stage: "diagram".to_string(),
                path: Some(path.to_string()),
                span: error.span().map(|span| DiagnosticSpan { start: span.start, end: span.end }),
            }),
        }
    }
    lowered
}
