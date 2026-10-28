//! Render a [`FormulaLayoutResult`] into SVG directly, walking the
//! [`PaintNode`] tree instead of the older [`LayoutNode`] tree.
//!
//! Every backend (SVG / PDF / MathML / measure) is meant to consume the same
//! immutable [`FormulaLayoutResult`] — this module is the SVG side of that
//! contract.  The older `render_layout_node` entry point (driven by the
//! [`LayoutBackend`] trait) is untouched; both paths coexist until the layout
//! engine fully migrates to the paint tree.

use std::fmt::Write;

use vutex_types::math::{FormulaLayoutResult, PaintNode};

/// Render a fully laid-out formula as an SVG document.
///
/// The viewBox is derived from [`FormulaLayoutResult::metrics`] — preferring
/// the explicit `ink_bounds` when non-degenerate, otherwise falling back to the
/// formula's advance width and vertical extent.  The paint tree is walked
/// recursively; each primitive maps to a single SVG element.
///
/// # GlyphRun handling
///
/// [`PaintNode::GlyphRun`] currently carries glyph ids and a `font_id` handle
/// but does **not** yet embed glyph outline paths.  This renderer therefore
/// emits a placeholder `<g data-glyph-run="..."/>` for each `GlyphRun` so the
/// tree structure survives a round-trip, and the node is not lost.  A future
/// change will plug the font snapshot back in and emit real outlines.
pub fn render_formula_result(result: &FormulaLayoutResult) -> Result<String, String> {
    let metrics = &result.metrics;

    // Determine viewBox from ink_bounds when it has real content, otherwise
    // derive a box from the formula's advance metrics.
    let (vb_x, vb_y, vb_w, vb_h) = if result.metrics.ink_bounds.width.to_f64() > 0.0
        || result.metrics.ink_bounds.height.to_f64() > 0.0
    {
        let ib = &result.metrics.ink_bounds;
        (
            ib.x.to_f64(),
            -(ib.y.to_f64() + ib.height.to_f64()),
            ib.width.to_f64(),
            ib.height.to_f64(),
        )
    }
    else {
        let w = metrics.width.to_f64().max(1.0);
        let h = (metrics.height.to_f64() + metrics.depth.to_f64()).max(1.0);
        (0.0, -metrics.height.to_f64(), w, h)
    };

    let mut out = String::new();
    write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.4}" height="{h:.4}" viewBox="{x:.4} {y:.4} {vw:.4} {vh:.4}">"#,
        w = vb_w,
        h = vb_h,
        x = vb_x,
        y = vb_y,
        vw = vb_w,
        vh = vb_h,
    )
    .map_err(|e| e.to_string())?;

    // Walk paint tree with baseline-at-origin, y-up math coordinates flipped
    // into SVG's y-down canvas space at emit time.
    render_paint_node(&mut out, &result.paint, 0.0, 0.0)?;

    out.push_str("</svg>");
    Ok(out)
}

/// Recursively emit SVG elements for one [`PaintNode`] subtree.
///
/// `(origin_x, origin_y)` is the cumulative math-space offset of the current
/// group, with y growing upward from the baseline.  SVG output flips y at the
/// leaf level so the canvas keeps y-down convention.
fn render_paint_node(out: &mut String, node: &PaintNode, origin_x: f64, origin_y: f64) -> Result<(), String> {
    match node {
        PaintNode::Group(children) => {
            for child in children {
                render_paint_node(out, child, origin_x, origin_y)?;
            }
        }
        PaintNode::Rect { x, y, width, height, stroke_width } => {
            let rx = origin_x + x.to_f64();
            let ry = origin_y + y.to_f64();
            let rw = width.to_f64();
            let rh = height.to_f64();
            // Flip y: math y-up → SVG y-down.  Rect y is the top edge in math
            // space so SVG needs `-(ry + rh)` as the SVG y and keeps rh.
            let svg_y = -(ry + rh);
            if stroke_width.to_f64() > 0.0 {
                write!(
                    out,
                    r#"<rect x="{:.4}" y="{:.4}" width="{:.4}" height="{:.4}" fill="currentColor" stroke="currentColor" stroke-width="{:.4}" />"#,
                    rx,
                    svg_y,
                    rw,
                    rh,
                    stroke_width.to_f64(),
                )
                .map_err(|e| e.to_string())?;
            }
            else {
                write!(
                    out,
                    r#"<rect x="{:.4}" y="{:.4}" width="{:.4}" height="{:.4}" fill="currentColor" />"#,
                    rx, svg_y, rw, rh,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        PaintNode::VectorPath { d, bbox: _bbox } => {
            // d is an absolute SVG path in math-space coordinates (y-up).
            // Flip y via transform and translate the whole path to its group origin.
            write!(
                out,
                r#"<g transform="translate({:.4}, {:.4}) scale(1, -1)"><path d="{}" fill="currentColor"/></g>"#,
                origin_x, origin_y, d,
            )
            .map_err(|e| e.to_string())?;
        }
        PaintNode::GlyphRun { font_id, glyph_ids, positions: _, size } => {
            // Glyph outlines are intentionally not embedded in the paint tree
            // yet — they live in the font snapshot and will be plumbed in a
            // follow-up change.  Emit a marker group so the structure survives.
            //
            // ## Outline-path query chain — current gap
            //
            // Layout produces `LayoutNode::Glyph` which carries BOTH
            // `path_data: Option<String>` (real SVG outline, populated from
            // `FontMetricsData.glyph_paths`) AND `glyph_id: Option<u16>`.
            // `lower_layout_to_paint` currently routes glyphs that have a
            // `glyph_id` into `PaintNode::GlyphRun`, discarding the sibling
            // `path_data`.  Only glyphs WITHOUT a `glyph_id` fall through to
            // `PaintNode::VectorPath`, where the outline IS preserved.
            //
            // `PaintNode::GlyphRun` itself has no `path_data` field, and
            // `FormulaLayoutResult.fonts` (`FontMetricsSnapshot`) only stores
            // face metadata (font_id, family, upm, axes) — the real
            // `glyph_paths: HashMap<GlyphKey, String>` lives inside
            // `FontMetricsData` which never leaves the layout engine.
            //
            // To render real outlines here, one of the following must happen:
            //   (a) Extend `PaintNode::GlyphRun` with per-glyph `path_data`,
            //       or a single merged outline;
            //   (b) Snapshot `glyph_paths` into `FontMetricsSnapshot` keyed by
            //       (font_id, glyph_id) so renderers can look them up;
            //   (c) Change `lower_layout_to_paint` to emit
            //       `PaintNode::VectorPath` for glyphs that have BOTH
            //       glyph_id AND path_data.
            //
            // Until then we keep the placeholder marker so the paint-tree
            // round-trip stays structurally intact and the node is not lost.
            write!(
                out,
                r#"<g data-glyph-run="{}" data-font="{}" data-size="{:.4}" data-count="{}"/>"#,
                font_id,
                font_id,
                size.to_f64(),
                glyph_ids.len(),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vutex_types::math::{Fixed, FormulaMetrics, PaintNode, formula_layout::BBox};

    fn rect_result() -> FormulaLayoutResult {
        FormulaLayoutResult {
            metrics: FormulaMetrics {
                width: Fixed::from_f64(100.0),
                height: Fixed::from_f64(20.0),
                depth: Fixed::from_f64(5.0),
                axis: Fixed::ZERO,
                ink_bounds: BBox {
                    x: Fixed::from_f64(10.0),
                    y: Fixed::ZERO,
                    width: Fixed::from_f64(80.0),
                    height: Fixed::from_f64(15.0),
                },
            },
            paint: PaintNode::Group(vec![PaintNode::Rect {
                x: Fixed::from_f64(10.0),
                y: Fixed::ZERO,
                width: Fixed::from_f64(80.0),
                height: Fixed::from_f64(15.0),
                stroke_width: Fixed::ZERO,
            }]),
            diagnostics: Vec::new(),
            fonts: vutex_types::math::formula_layout::FontMetricsSnapshot::default(),
        }
    }

    #[test]
    fn rect_paint_emits_svg_rect() {
        let svg = render_formula_result(&rect_result()).expect("render");
        assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
        assert!(svg.contains("<rect "));
        assert!(svg.contains("width="));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn vector_path_paint_emits_svg_path() {
        let mut res = rect_result();
        res.paint = PaintNode::Group(vec![PaintNode::VectorPath {
            d: "M 0 0 L 10 0 L 5 10 Z".to_string(),
            bbox: BBox {
                x: Fixed::ZERO,
                y: Fixed::ZERO,
                width: Fixed::from_f64(10.0),
                height: Fixed::from_f64(10.0),
            },
        }]);
        let svg = render_formula_result(&res).expect("render");
        assert!(svg.contains("<path d=\"M 0 0 L 10 0 L 5 10 Z\""));
        assert!(svg.contains("scale(1, -1)"));
    }

    #[test]
    fn empty_paint_still_emits_valid_svg() {
        let res = FormulaLayoutResult::default();
        let svg = render_formula_result(&res).expect("render");
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn glyph_run_emits_placeholder_group() {
        let mut res = rect_result();
        res.paint = PaintNode::Group(vec![PaintNode::GlyphRun {
            font_id: "lmr".to_string(),
            glyph_ids: vec![65, 66],
            positions: vec![(Fixed::ZERO, Fixed::ZERO), (Fixed::from_f64(10.0), Fixed::ZERO)],
            size: Fixed::from_f64(16.0),
        }]);
        let svg = render_formula_result(&res).expect("render");
        assert!(svg.contains(r#"data-glyph-run="lmr""#));
        assert!(svg.contains(r#"data-count="2""#));
    }
}
