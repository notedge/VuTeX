//! Lower a laid-out math tree into absolutely positioned layout primitives.

use super::LayoutNode;
use crate::plan::{LayoutItem, Path, Rule, TextRun};
use vutex_types::math::FontStyle;

/// Smallest stroke width emitted for a math rule, in points.
const MIN_RULE_THICKNESS: f64 = 0.4;

/// Collect the primitives of a math layout tree placed at `(x, y)`.
///
/// `x` is the left edge of the formula and `y` its baseline. The traversal
/// mirrors `render_layout_node`: an HBox advances its children along x while
/// its `shift` offsets y, a VBox stacks its children from `-height` while its
/// `shift` offsets x, glyphs paint at their baseline, and a rule paints from
/// `y - height` to `y + depth`.
///
/// When a glyph carries an outline path (extracted from the embedded MATH font
/// in `opentype.rs`) we emit a `LayoutItem::Path` instead of a text run so the
/// PDF/SVG/HTML backends can paint exact glyph curves. Glyphs without outlines
/// still fall back to `TextRun`. Standalone `LayoutNode::Path` nodes (used by
/// e.g. the radical symbol) are lowered directly to `LayoutItem::Path`.
pub fn collect_layout(node: &LayoutNode, x: f64, y: f64) -> Vec<LayoutItem> {
    let mut items = Vec::new();
    push_layout(node, x, y, &mut items);
    items
}

fn push_layout(node: &LayoutNode, x: f64, y: f64, items: &mut Vec<LayoutItem>) {
    match node {
        LayoutNode::HBox(hbox) => {
            let child_y = y + hbox.shift.to_f64();
            let mut current_x = 0.0;
            for child in &hbox.children {
                push_layout(child, x + current_x, child_y, items);
                current_x += child.width().to_f64();
            }
        }
        LayoutNode::VBox(vbox) => {
            let child_x = x + vbox.shift.to_f64();
            let mut current_y = -vbox.height.to_f64();
            for child in &vbox.children {
                push_layout(child, child_x, y + current_y + child.height().to_f64(), items);
                current_y += child.height().to_f64() + child.depth().to_f64();
            }
        }
        LayoutNode::Glyph(glyph) => {
            if let Some(path) = &glyph.path_data {
                items.push(scale_svg_path(
                    path,
                    glyph.size.to_f64(),
                    x,
                    y,
                    glyph.metrics.width.to_f64(),
                    glyph.metrics.height.to_f64(),
                    glyph.metrics.depth.to_f64(),
                ));
            }
            else if let Some(ch) = glyph.char {
                items.push(LayoutItem::TextRun(TextRun {
                    x,
                    y,
                    size: glyph.size.to_f64(),
                    width: glyph.metrics.width.to_f64(),
                    font_family: output_family(&glyph.font_family),
                    style: FontStyle::Normal,
                    text: ch.to_string(),
                    href: None,
                    title: None,
                }));
            }
        }
        LayoutNode::Rule { width, height, depth: _ } => {
            let top = y - height.to_f64();
            items.push(LayoutItem::Rule(Rule {
                x1: x,
                y1: top,
                x2: x + width.to_f64(),
                y2: top,
                thickness: height.to_f64().max(MIN_RULE_THICKNESS),
            }));
        }
        LayoutNode::Path(p) => {
            items.push(LayoutItem::Path(Path {
                d: scale_svg_path_d(&p.d, 1.0),
                x,
                y,
                width: p.width.to_f64(),
                height: p.height.to_f64(),
                depth: p.depth.to_f64(),
            }));
        }
        // Kerns, glue, and penalties paint nothing; the parent boxes already
        // advanced past their extents.
        LayoutNode::Kern(_) | LayoutNode::Glue(_) | LayoutNode::Penalty { .. } => {}
    }
}

/// Scale every coordinate in an SVG `d` string by `scale`, producing a new
/// `Path` LayoutItem anchored at `(x, y)` with the given extents.
///
/// The raw SVG outline from `opentype.rs` is normalised to a 0..1 em box;
/// multiplying by `size` converts it to page points. We emit an unscaled
/// `d` when the path already lives in page coordinates (scale == 1).
fn scale_svg_path(raw_d: &str, size: f64, x: f64, y: f64, width: f64, height: f64, depth: f64) -> LayoutItem {
    let d = if (size - 1.0).abs() < f64::EPSILON { raw_d.to_string() } else { scale_svg_path_d(raw_d, size) };
    LayoutItem::Path(Path { d, x, y, width, height, depth })
}

/// Multiply every numeric literal in an SVG `d` attribute by `scale`.
///
/// Parses only the operators we actually emit from `SvgPathBuilder`
/// (M, L, Q, C, Z); unknown operators and trailing whitespace are passed
/// through verbatim. Coordinates are separated by commas or whitespace, so
/// we tokenise on those boundaries and re-emit with the same separators.
fn scale_svg_path_d(raw: &str, scale: f64) -> String {
    if (scale - 1.0).abs() < f64::EPSILON {
        return raw.to_string();
    }

    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Operator letter — emit as-is.
        if c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }

        // Whitespace / comma — emit as-is.
        if c == ' ' || c == ',' {
            out.push(c);
            i += 1;
            continue;
        }

        // Number — parse, scale, re-emit.
        if c.is_ascii_digit() || c == '-' || c == '.' {
            let start = i;
            if c == '-' {
                i += 1;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if let Ok(v) = num_str.parse::<f64>() {
                let scaled = v * scale;
                // Keep reasonable precision, similar to SvgPathBuilder.
                let formatted = format!("{scaled:.4}");
                out.push_str(&formatted);
            }
            else {
                // Fallback: emit original verbatim.
                out.push_str(&num_str);
            }
            continue;
        }

        // Unknown char — emit as-is.
        out.push(c);
        i += 1;
    }

    out
}

/// Font family written into text primitives.
///
/// The metrics system labels glyphs from the default math font as `default`,
/// which is not a usable CSS family, so those runs are painted with `serif`.
fn output_family(family: &str) -> String {
    if family == "default" { "serif".to_string() } else { family.to_string() }
}
