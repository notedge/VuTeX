//! Lower a laid-out math tree into absolutely positioned layout primitives.

use super::LayoutNode;
use crate::plan::{LayoutItem, Path, Rule};
use vutex_types::{Diagnostic, Severity};

/// Smallest thickness emitted for a math rule, in points.
const MIN_RULE_THICKNESS: f64 = 0.4;

/// Collect the primitives of a math layout tree placed at `(x, y)`.
///
/// `x` is the left edge of the formula and `y` its baseline. The traversal
/// mirrors `render_layout_node`: an HBox advances its children along x while
/// its `shift` offsets y, a VBox stacks its children from `-height` while its
/// `shift` offsets x, glyphs paint at their baseline, and a rule fills the
/// rectangle from `y - height` to `y + depth`.
///
/// Every visible primitive is a vector primitive whose coordinates are already
/// page-scaled in the y-down page space: a glyph contributes the outline
/// extracted from the embedded MATH font, a rule contributes a filled bar (a
/// [`LayoutItem::Rule`] when it does not descend below the baseline, else a
/// closed rectangle path), and a standalone `LayoutNode::Path` (for example a
/// stretched radical sign) contributes its own path. No text run is emitted, so
/// every backend paints the same outlines instead of asking a host font to
/// draw a glyph whose metrics came from the math font.
///
/// A glyph the font system has no outline for is skipped and reported through
/// [`collect_layout_with_diagnostics`]; its measured advance stays in the
/// surrounding box, so the formula keeps its geometry without a fake glyph.
pub fn collect_layout(node: &LayoutNode, x: f64, y: f64) -> Vec<LayoutItem> {
    collect_layout_with_diagnostics(node, x, y, &mut Vec::new())
}

/// Collect the primitives of a math layout tree and report outline gaps.
///
/// Layout cannot paint a glyph it has no outline for without borrowing a host
/// font, which would break the identity between the face that supplied the
/// metrics and the face that paints the ink. Such glyphs are therefore skipped
/// and named in `diagnostics` instead of degrading to a host serif run.
pub fn collect_layout_with_diagnostics(
    node: &LayoutNode,
    x: f64,
    y: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<LayoutItem> {
    let mut items = Vec::new();
    push_layout(node, x, y, &mut items, diagnostics);
    items
}

fn push_layout(node: &LayoutNode, x: f64, y: f64, items: &mut Vec<LayoutItem>, diagnostics: &mut Vec<Diagnostic>) {
    match node {
        LayoutNode::HBox(hbox) => {
            let child_y = y + hbox.shift.to_f64();
            let mut current_x = 0.0;
            for child in &hbox.children {
                push_layout(child, x + current_x, child_y, items, diagnostics);
                current_x += child.width().to_f64();
            }
        }
        LayoutNode::VBox(vbox) => {
            let child_x = x + vbox.shift.to_f64();
            let mut current_y = -vbox.height.to_f64();
            for child in &vbox.children {
                push_layout(child, child_x, y + current_y + child.height().to_f64(), items, diagnostics);
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
            else {
                diagnostics.push(outline_missing(glyph));
            }
        }
        LayoutNode::Rule { width, height, depth } => {
            let bar_width = width.to_f64();
            let thickness = height.to_f64().max(MIN_RULE_THICKNESS);
            let drop = depth.to_f64();
            if bar_width <= 0.0 || !(bar_width.is_finite() && thickness.is_finite() && drop.is_finite()) {
                return;
            }
            if drop == 0.0 {
                // The box spans `y - height .. y`, so its centre line sits half a
                // thickness above the box bottom.
                let centre = y - thickness / 2.0;
                items.push(LayoutItem::Rule(Rule { x1: x, y1: centre, x2: x + bar_width, y2: centre, thickness }));
            }
            else {
                // A rule that descends below the baseline needs its full box, which
                // `LayoutItem::Rule` cannot express.
                items.push(LayoutItem::Path(Path {
                    d: format!(
                        "M 0,{} L {},{} L {},{} L 0,{} Z",
                        decimal(-thickness),
                        decimal(bar_width),
                        decimal(-thickness),
                        decimal(bar_width),
                        decimal(drop),
                        decimal(drop)
                    ),
                    x,
                    y,
                    width: bar_width,
                    height: thickness,
                    depth: drop,
                }));
            }
        }
        LayoutNode::Path(p) => {
            items.push(LayoutItem::Path(Path {
                d: scale_svg_path_d(&p.d, 1.0, -1.0),
                x,
                y,
                width: p.width.to_f64(),
                height: p.height.to_f64(),
                depth: p.depth.to_f64(),
            }));
        }
        // Kerns, glue, penalties, and anchors paint nothing; the parent
        // boxes already advanced past their extents.
        LayoutNode::Kern(_) | LayoutNode::Glue(_) | LayoutNode::Penalty { .. } | LayoutNode::Anchor { .. } => {}
    }
}

/// Structure diagnostic for a glyph the math font ships no outline for.
fn outline_missing(glyph: &super::Glyph) -> Diagnostic {
    let name = match glyph.char {
        Some(ch) => format!("'{ch}' (U+{:04X})", ch as u32),
        None => "without a character".to_string(),
    };
    Diagnostic {
        code: "vutex.layout.math.outline_missing".into(),
        severity: Severity::Warning,
        message: format!(
            "math glyph {name} in family `{}` has no embedded outline and was not painted; the box keeps its measured advance rather than falling back to a host font",
            glyph.font_family
        ),
    }
}

/// Four-decimal number, matching the precision outlines are scaled with.
fn decimal(value: f64) -> String {
    format!("{value:.4}")
}

/// Convert a glyph outline into a `Path` LayoutItem anchored at `(x, y)`.
///
/// Outlines from `opentype.rs` are normalised to em units with y growing
/// upward from the baseline. `DisplayPath` coordinates are page points whose
/// origin sits on the baseline with y growing downward, so x is multiplied by
/// the font size and y by its negation.
fn scale_svg_path(raw_d: &str, size: f64, x: f64, y: f64, width: f64, height: f64, depth: f64) -> LayoutItem {
    LayoutItem::Path(Path { d: scale_svg_path_d(raw_d, size, -size), x, y, width, height, depth })
}

/// Scale every coordinate in an SVG `d` attribute by `(scale_x, scale_y)`.
///
/// Parses only the operators we actually emit from `SvgPathBuilder`
/// (M, L, Q, C, Z), whose coordinates always come in x/y pairs. Unknown
/// operators and whitespace are passed through verbatim. The returned path
/// keeps the same origin as the input, so callers position it by translating.
pub(crate) fn scale_svg_path_d(raw: &str, scale_x: f64, scale_y: f64) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut is_x = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Operator letter — emit as-is and restart the coordinate pair.
        if c.is_ascii_alphabetic() {
            out.push(c);
            is_x = true;
            i += 1;
            continue;
        }

        // Whitespace / comma — emit as-is.
        if c == ' ' || c == ',' {
            out.push(c);
            i += 1;
            continue;
        }

        // Number — parse, scale along its axis, re-emit.
        if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            if c == '-' || c == '+' {
                i += 1;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if let Ok(v) = num_str.parse::<f64>() {
                let scaled = if is_x { v * scale_x } else { v * scale_y };
                // Keep reasonable precision, similar to SvgPathBuilder.
                if scaled == 0.0 {
                    out.push_str("0.0000");
                }
                else {
                    out.push_str(&format!("{scaled:.4}"));
                }
            }
            else {
                // Fallback: emit original verbatim.
                out.push_str(&num_str);
            }
            is_x = !is_x;
            continue;
        }

        // Unknown char — emit as-is.
        out.push(c);
        i += 1;
    }

    out
}

/// Vertical extent `(min_y, max_y)` of an em-unit outline whose y axis grows
/// upward from the baseline.
///
/// Used to keep a stretched outline's metrics in step with the geometry it
/// actually paints, so the layout box never hides ink below the baseline.
pub(crate) fn path_vertical_extent(raw: &str) -> (f64, f64) {
    let chars: Vec<char> = raw.chars().collect();
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut is_x = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Operator letter — restart the coordinate pair.
        if c.is_ascii_alphabetic() {
            is_x = true;
            i += 1;
            continue;
        }

        if c == ' ' || c == ',' {
            i += 1;
            continue;
        }

        if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            if c == '-' || c == '+' {
                i += 1;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if let Ok(v) = num_str.parse::<f64>() {
                if !is_x {
                    min_y = min_y.min(v);
                    max_y = max_y.max(v);
                }
            }
            is_x = !is_x;
            continue;
        }

        i += 1;
    }

    if min_y.is_finite() { (min_y, max_y) } else { (0.0, 0.0) }
}
