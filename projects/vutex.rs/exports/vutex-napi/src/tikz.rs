//! Host-side wiring for TikZ pictures that reach the document graph.
//!
//! # Upstream status
//!
//! Lowering lives upstream in `plotter.input.tikz`. This module only walks the
//! graph, extracts the authored picture source, and registers the scene that
//! `plotter_input_tikz::tikz_to_scene` resolves under `TIKZ_ALT`. A rejection
//! keeps the `davinci.diagram.unsupported` code and the inline byte span the
//! frontend reports, so hosts can point at the rejected source.
//!
//! Notedown delivers a TikZ picture in one of two shapes. A Markdown or
//! Notedown fence tagged `tikz` stays a `Block::Code` with its language tag, and
//! a TeX `tikzpicture` environment that the importer does not model becomes a
//! `Block::Opaque` whose `kind` is `tex.environment.tikzpicture` and whose
//! payload hint is the raw environment source. Both forms are lowered here into
//! a display scene and registered under the block that authored them, so
//! `vutex_layout` places the picture as a graphic instead of the source text.

use document_ir::{Block, DocumentGraph};
use plotter_input_tikz::{TIKZ_ALT, tikz_to_scene};

use crate::{
    diagram::{LoweredDiagrams, document_span},
    document::{DiagnosticRecord, DiagnosticSpan},
};

/// Lower every TikZ picture in `graph` into a diagram scene.
///
/// Both a code fence tagged `tikz` and a TeX opaque environment of kind
/// `tex.environment.tikzpicture` are considered. Scenes are registered in
/// `lowered`, and a failed lowering leaves the block without a scene so layout
/// keeps the authored source visible or drops it exactly as before.
pub fn lower_tikz_diagrams(graph: &DocumentGraph, path: &str, lowered: &mut LoweredDiagrams) {
    for node in &graph.blocks {
        let source = match &node.block {
            Block::Code { language: Some(language), content } if language.trim().eq_ignore_ascii_case("tikz") => {
                content.as_str()
            }
            Block::Opaque { kind, payload_hint, .. } if kind == "tex.environment.tikzpicture" => payload_hint.as_str(),
            _ => continue,
        };
        match tikz_to_scene(source) {
            Ok(scene) => {
                lowered.diagrams.insert(node.id, scene, TIKZ_ALT);
            }
            Err(error) => lowered.diagnostics.push(DiagnosticRecord {
                code: error.code().to_string(),
                severity: "error".to_string(),
                message: error.message().to_string(),
                stage: "diagram".to_string(),
                path: Some(path.to_string()),
                span: document_span(
                    graph.block_span(node.id),
                    error.span().map(|span| DiagnosticSpan { start: span.start, end: span.end }),
                ),
            }),
        }
    }
}
