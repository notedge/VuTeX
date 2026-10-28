//! Single immutable layout result consumed by SVG / PDF / MathML / measure.
//!
//! The goal is a pipeline where every backend renders one shared
//! [`FormulaLayoutResult`] instead of each backend walking its own tree.
//! This module defines the data skeleton only — no fonts, no backends, no
//! layout engine.

use serde::{Deserialize, Serialize};

use crate::Diagnostic;

use super::Fixed;

/// Ink bounding box (minimal axis-aligned rectangle).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct BBox {
    pub x: Fixed,
    pub y: Fixed,
    pub width: Fixed,
    pub height: Fixed,
}

/// Metrics of a fully laid-out formula.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormulaMetrics {
    /// Total advance width in design units.
    pub width: Fixed,
    /// Distance from baseline to top.
    pub height: Fixed,
    /// Distance from baseline to bottom (positive value means the formula
    /// descends below the baseline).
    pub depth: Fixed,
    /// Baseline-to-axis distance for large operators.
    pub axis: Fixed,
    /// Minimal rectangle containing all ink.
    pub ink_bounds: BBox,
}

/// Immutable paint tree — one node = one visual primitive.
///
/// Pure data.  Every renderer (SVG / PDF / MathML / measure) walks this
/// tree and emits its own dialect.  No font handles, no backend state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaintNode {
    /// A group of children rendered in sequence.  Offset is accumulated by
    /// the parent; every child position is relative to the group origin.
    Group(Vec<PaintNode>),
    /// One or more glyphs rendered at per-glyph offsets.
    GlyphRun {
        /// Font unique identifier (hash key used by the renderer to look up
        /// the actual face; matches [`FontFaceSnapshot::font_id`]).
        font_id: String,
        /// Glyph ids within the referenced font.
        glyph_ids: Vec<u16>,
        /// Per-glyph `(x, y)` offset relative to the group origin.
        positions: Vec<(Fixed, Fixed)>,
        /// Design size in the same units as [`Fixed`].
        size: Fixed,
        /// Per-glyph SVG `d` outline path.  Same length as `glyph_ids` /
        /// `positions`.  `None` entries mean the glyph has no outline
        /// (e.g. whitespace or a character that only has a text fallback).
        path_data: Vec<Option<String>>,
    },
    /// A filled rectangle (rule bar, fraction line, underline, ...).
    Rect { x: Fixed, y: Fixed, width: Fixed, height: Fixed, stroke_width: Fixed },
    /// A raw SVG path for complex glyph outlines that cannot be represented
    /// as a [`PaintNode::GlyphRun`].
    VectorPath {
        /// SVG `d` attribute string.
        d: String,
        /// Pre-computed bounding box of the path.
        bbox: BBox,
    },
}

impl Default for PaintNode {
    fn default() -> Self {
        // Empty group keeps the "paint tree is always a Group" invariant.
        PaintNode::Group(Vec::new())
    }
}

impl PaintNode {
    /// Compute the total axis-aligned bounding box of this node and all of
    /// its descendants.
    ///
    /// The `GlyphRun` variant is a first-pass placeholder: it returns an
    /// empty [`BBox`] because metrics live in the font snapshot and the
    /// paint tree must remain backend-free.  Layout will fill the real box
    /// in a follow-up change.
    pub fn bbox(&self) -> BBox {
        match self {
            PaintNode::Group(children) => children.iter().map(|c| c.bbox()).fold(BBox::default(), |acc, b| bbox_union(acc, b)),
            // path_data bbox computation delegated to render-time: the
            // renderer can inspect `path_data` / `glyph_paths` in
            // [`FontMetricsSnapshot`] and build the real box from SVG
            // geometry.  Layout keeps the paint tree backend-free.
            PaintNode::GlyphRun { .. } => BBox::default(),
            PaintNode::Rect { x, y, width, height, .. } => BBox { x: *x, y: *y, width: *width, height: *height },
            PaintNode::VectorPath { bbox, .. } => *bbox,
        }
    }
}

fn bbox_union(a: BBox, b: BBox) -> BBox {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    BBox { x, y, width: (ax2.max(bx2)) - x, height: (ay2.max(by2)) - y }
}

/// Full layout result — single source of truth for all backends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormulaLayoutResult {
    pub metrics: FormulaMetrics,
    pub paint: PaintNode,
    /// Non-fatal diagnostics emitted during layout (unsupported commands,
    /// style demotion fallbacks, etc.).
    pub diagnostics: Vec<Diagnostic>,
    /// Font state captured at layout time.  Renderers consume this snapshot
    /// even when the original font source is gone.
    pub fonts: FontMetricsSnapshot,
}

/// Layout-time snapshot of all font faces referenced by [`FormulaLayoutResult::paint`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontMetricsSnapshot {
    pub faces: Vec<FontFaceSnapshot>,
    /// SVG `d` outline paths keyed by `(font_id, glyph_id)`.
    /// Populated at layout time so renderers can draw glyph outlines
    /// without re-querying the font system.
    pub glyph_paths: Vec<(String, u16, String)>,
}

/// A single font face captured at layout time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontFaceSnapshot {
    /// Opaque identifier matched against [`PaintNode::GlyphRun::font_id`].
    pub font_id: String,
    /// Typographic family name for display/debug.
    pub family: String,
    /// Design units per em of the face.
    pub design_units_per_em: u32,
    /// Variable font axis values at the time of layout.
    pub axes: Vec<FontAxis>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontAxis {
    /// OpenType tag, e.g. `"wght"`.
    pub tag: String,
    /// Numeric value of the axis.
    pub value: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_layout_result_is_instantiable() {
        let result = FormulaLayoutResult::default();
        assert_eq!(result.metrics.width, Fixed::ZERO);
    }

    #[test]
    fn paint_group_computes_union_bbox() {
        let a = PaintNode::Rect {
            x: Fixed::ZERO,
            y: Fixed::ZERO,
            width: Fixed::ONE,
            height: Fixed::ONE,
            stroke_width: Fixed::ZERO,
        };
        let b = PaintNode::Rect {
            x: Fixed::from_f64(2.0),
            y: Fixed::ZERO,
            width: Fixed::ONE,
            height: Fixed::ONE,
            stroke_width: Fixed::ZERO,
        };
        let group = PaintNode::Group(vec![a, b]);
        let bbox = group.bbox();
        assert_eq!(bbox.width, Fixed::from_f64(3.0));
    }
}
