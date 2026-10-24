//! Host-side Mermaid lowering into DaVinci scenes.
//!
//! `notedown-formats` keeps a Mermaid fence as `Block::Code` with its language
//! tag, so this adapter lowers those code blocks through `davinci-diagram` and
//! registers each scene under the block id that authored it. Layout then places
//! the scene instead of the fence text. A failed lowering registers no scene,
//! which keeps the authored fence visible and never invents diagram geometry.

use davinci_diagram::mermaid_to_scene;
use notedown_ir::{Block, DocumentGraph};
use vutex_layout::Diagrams;

use crate::document::DiagnosticRecord;

/// Alternative text carried by every lowered Mermaid diagram.
pub const MERMAID_ALT: &str = "Mermaid diagram";

/// Diagnostic code for a scene whose canvas cannot be placed.
pub const DIAGRAM_INVALID_CANVAS: &str = "VUTEX_DIAGRAM_INVALID_CANVAS";

/// Scenes lowered from Mermaid code blocks together with their diagnostics.
#[derive(Debug)]
pub struct LoweredDiagrams {
    /// Scenes keyed by the code block that authored them.
    pub diagrams: Diagrams,
    /// Lowering diagnostics in document order.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// Lower every Mermaid code block in `graph` into a diagram scene.
///
/// Only `Block::Code` blocks whose language tag is `mermaid` are considered.
/// Fatal lowering failures and non-fatal layout loss both become diagnostics;
/// a block with a fatal failure or an unplaceable canvas keeps no scene, so
/// layout falls back to the authored fence text.
pub fn lower_mermaid_diagrams(graph: &DocumentGraph, path: &str) -> LoweredDiagrams {
    let mut lowered = LoweredDiagrams { diagrams: Diagrams::new(), diagnostics: Vec::new() };
    let authored = std::fs::read_to_string(path)
        .ok()
        .map(|text| authored_mermaid_fences(&text))
        .filter(|fences| fences.len() == mermaid_block_count(graph));
    let mut authored = authored.iter().flatten();
    for node in &graph.blocks {
        let Block::Code { language: Some(language), content } = &node.block
        else {
            continue;
        };
        if !language.trim().eq_ignore_ascii_case("mermaid") {
            continue;
        }
        // The Markdown importer lowers fenced code token by token, so markers
        // such as `[` can be lost before the graph reaches the host. When the
        // authored fence count matches, the original fence text is authoritative.
        let source = authored.next().map_or(content.as_str(), String::as_str);
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

/// Count the Mermaid code blocks the importer produced.
fn mermaid_block_count(graph: &DocumentGraph) -> usize {
    graph
        .blocks
        .iter()
        .filter(|node| {
            matches!(
                &node.block,
                Block::Code { language: Some(language), .. } if language.trim().eq_ignore_ascii_case("mermaid")
            )
        })
        .count()
}

/// Extract the content of every top-level Mermaid fence in authored text.
///
/// Only fences whose opening line carries a `mermaid` info string are returned.
/// The caller pairs them with importer blocks by order and must fall back when
/// the counts differ, which happens for nested fences or non-Markdown sources.
fn authored_mermaid_fences(text: &str) -> Vec<String> {
    let mut fences = Vec::new();
    let mut open: Option<(&str, Vec<&str>)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, body)) = open.as_mut() {
            if is_closing_fence(trimmed, marker) {
                fences.push(body.join("\n"));
                open = None;
            }
            else {
                body.push(line);
            }
            continue;
        }
        if let Some((marker, info)) = opening_fence(trimmed)
            && info.eq_ignore_ascii_case("mermaid")
        {
            open = Some((marker, Vec::new()));
        }
    }
    fences
}

/// Split an opening fence line into its marker and info string.
fn opening_fence(line: &str) -> Option<(&str, &str)> {
    let fence_char = line.chars().next().filter(|character| *character == '`' || *character == '~')?;
    let marker_len = line.chars().take_while(|character| *character == fence_char).count();
    if marker_len < 3 {
        return None;
    }
    let (marker, rest) = line.split_at(marker_len);
    Some((marker, rest.trim()))
}

/// Whether `line` closes a fence opened with `marker`.
fn is_closing_fence(line: &str, marker: &str) -> bool {
    line.strip_prefix(marker).is_some_and(|rest| rest.trim().is_empty())
}
