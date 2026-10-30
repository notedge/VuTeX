//! Single immutable layout result consumed by SVG / PDF / MathML / measure.
//!
//! The goal is a pipeline where every backend renders one shared
//! [`FormulaLayoutResult`] instead of each backend walking its own tree.
//! This module defines the data skeleton only — no fonts, no backends, no
//! layout engine.

use serde::{Deserialize, Serialize};

use crate::Diagnostic;

use super::{Fixed, MathSemantics};

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
/// Pure data.  Every renderer (SVG / PDF / HTML / measure) walks this tree
/// and emits its own dialect.  No font handles, no backend state.
///
/// # Coordinate space
///
/// All offsets are canvas pixels with **y growing downward**, laid out in the
/// formula box: the root origin is the box top-left and the baseline sits at
/// `y = metrics.height`.  A [`PaintNode::Group`] places its children with its
/// own translate; every child position is relative to the innermost enclosing
/// group origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaintNode {
    /// Children sharing one translate offset from the enclosing group origin.
    Group {
        /// Children rendered in sequence.
        children: Vec<PaintNode>,
        /// Horizontal offset of the group origin, canvas pixels.
        translate_x: Fixed,
        /// Vertical offset of the group origin, canvas pixels (y-down).
        translate_y: Fixed,
    },
    /// One or more glyphs rendered at per-glyph baseline anchors.
    GlyphRun {
        /// Font unique identifier (hash key used by the renderer to look up
        /// the actual face; matches [`FontFaceSnapshot::font_id`]).
        font_id: String,
        /// Glyph ids within the referenced font.
        glyph_ids: Vec<u16>,
        /// Unicode scalar per glyph when the layout key carried one.  A
        /// renderer may emit a host-font text run for these only when the
        /// face ships no outline (`path_data` entry is `None`) and the caller
        /// allows text fallback; whitespace never paints.
        chars: Vec<Option<char>>,
        /// Font family label for the text fallback run.
        family: String,
        /// Per-glyph `(x, y)` baseline anchor relative to the group origin,
        /// canvas pixels (y-down).  The outline in `path_data` is in em units
        /// with y growing upward and paints under
        /// `translate(x, y) scale(size, -size)`.
        positions: Vec<(Fixed, Fixed)>,
        /// Design size in the same units as [`Fixed`].
        size: Fixed,
        /// Per-glyph SVG `d` outline path.  Same length as `glyph_ids` /
        /// `positions`.  `None` entries mean the glyph has no outline
        /// (e.g. whitespace or a character that only has a text fallback).
        path_data: Vec<Option<String>>,
    },
    /// A filled rectangle (rule bar, fraction line, underline, ...).
    /// `(x, y)` is the top-left corner relative to the group origin, canvas
    /// pixels with y growing downward.  `baseline` is the rule's own baseline
    /// in the same group-relative space: the bar spans `baseline - height`
    /// down to `baseline + depth`, so a plan consumer can re-emit the bar as a
    /// centred stroke above the baseline or a full box when it descends.
    Rect { x: Fixed, y: Fixed, width: Fixed, height: Fixed, stroke_width: Fixed, baseline: Fixed },
    /// A raw vector path for complex outlines that cannot be represented
    /// as a [`PaintNode::GlyphRun`].
    VectorPath {
        /// SVG `d` attribute string in y-up unit space.  The renderer maps it
        /// with `translate(x, y) scale(scale, -scale)`.
        d: String,
        /// Baseline anchor relative to the group origin, canvas pixels.
        x: Fixed,
        /// Baseline anchor relative to the group origin, canvas pixels.
        y: Fixed,
        /// Unit scale from `d` space to canvas pixels (glyph outlines use the
        /// design size, pre-scaled paths use `1`).
        scale: Fixed,
        /// Pre-computed ink box in group-relative canvas coordinates.
        bbox: BBox,
    },
}

impl Default for PaintNode {
    fn default() -> Self {
        // Empty group keeps the "paint tree is always a Group" invariant.
        PaintNode::Group { children: Vec::new(), translate_x: Fixed::ZERO, translate_y: Fixed::ZERO }
    }
}

impl PaintNode {
    /// Compute the total axis-aligned bounding box of this node and all of
    /// its descendants, in the coordinates of the enclosing group.
    ///
    /// The `GlyphRun` variant returns an empty [`BBox`] because its ink
    /// depends on parsing outline geometry, which stays a render-time
    /// concern; the paint tree must remain backend-free.
    pub fn bbox(&self) -> BBox {
        match self {
            PaintNode::Group { children, translate_x, translate_y } => children
                .iter()
                .map(|c| offset_bbox(c.bbox(), *translate_x, *translate_y))
                .reduce(|acc, b| bbox_union(acc, b))
                .unwrap_or_default(),
            PaintNode::GlyphRun { .. } => BBox::default(),
            PaintNode::Rect { x, y, width, height, .. } => BBox { x: *x, y: *y, width: *width, height: *height },
            PaintNode::VectorPath { bbox, .. } => *bbox,
        }
    }
}

fn offset_bbox(b: BBox, dx: Fixed, dy: Fixed) -> BBox {
    BBox { x: b.x + dx, y: b.y + dy, width: b.width, height: b.height }
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
    /// The structured math semantics this layout consumed.
    ///
    /// This is the IR snapshot the layout pass read, lowered to the
    /// backend-facing [`MathSemantics`] shape.  Structured backends (MathML)
    /// render it directly instead of parsing the source again, and every
    /// backend can prove its output describes the same formula as
    /// [`FormulaLayoutResult::paint`] because both live on this one result.
    pub semantics: MathSemantics,
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
            baseline: Fixed::ONE,
        };
        let b = PaintNode::Rect {
            x: Fixed::from_f64(2.0),
            y: Fixed::ZERO,
            width: Fixed::ONE,
            height: Fixed::ONE,
            stroke_width: Fixed::ZERO,
            baseline: Fixed::ONE,
        };
        let group = PaintNode::Group { children: vec![a, b], translate_x: Fixed::ZERO, translate_y: Fixed::ZERO };
        let bbox = group.bbox();
        assert_eq!(bbox.width, Fixed::from_f64(3.0));
    }

    #[test]
    fn paint_group_translate_offsets_children() {
        let a = PaintNode::Rect {
            x: Fixed::ZERO,
            y: Fixed::ZERO,
            width: Fixed::ONE,
            height: Fixed::ONE,
            stroke_width: Fixed::ZERO,
            baseline: Fixed::ONE,
        };
        let group = PaintNode::Group {
            children: vec![PaintNode::Group {
                children: vec![a],
                translate_x: Fixed::from_f64(4.0),
                translate_y: Fixed::from_f64(-2.0),
            }],
            translate_x: Fixed::from_f64(1.0),
            translate_y: Fixed::ZERO,
        };
        let bbox = group.bbox();
        assert_eq!(bbox.x, Fixed::from_f64(5.0));
        assert_eq!(bbox.y, Fixed::from_f64(-2.0));
    }
}
