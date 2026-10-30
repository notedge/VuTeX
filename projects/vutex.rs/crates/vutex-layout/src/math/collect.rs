//! Lower a laid-out math tree into absolutely positioned layout primitives.

use crate::plan::{LayoutItem, Path, Rule};
use vutex_types::{
    Diagnostic, Severity,
    math::{Fixed, FormulaLayoutResult, PaintNode},
};

/// Smallest thickness emitted for a math rule, in points.
const MIN_RULE_THICKNESS: f64 = 0.4;

/// Collect the positioned primitives of one laid-out formula whose box left
/// edge sits at page `x` and whose baseline sits at page `y`.
///
/// The paint tree already carries the canvas geometry the box walk used to
/// compute while painting: a [`PaintNode::Group`] translates its children, a
/// glyph outline becomes a page-scaled [`LayoutItem::Path`], a rule becomes a
/// [`LayoutItem::Rule`] centred stroke while it stays above its own baseline
/// and a closed box when it descends below it, and a standalone vector path
/// becomes its own [`LayoutItem::Path`]. No text run is emitted, so every
/// backend paints the same outlines instead of asking a host font to draw a
/// glyph whose metrics came from the math font.
///
/// A glyph the font system has no outline for is skipped and named in
/// `diagnostics`; its measured advance stays in the surrounding box, so the
/// formula keeps its geometry without a fake glyph.
pub fn collect_paint(result: &FormulaLayoutResult, x: f64, y: f64, diagnostics: &mut Vec<Diagnostic>) -> Vec<LayoutItem> {
    let mut items = Vec::new();
    // Canvas origin is the box top-left; the formula baseline sits
    // `metrics.height` below it.
    push_paint(&result.paint, x, y - result.metrics.height.to_f64(), &mut items, diagnostics);
    items
}

fn push_paint(node: &PaintNode, ox: f64, oy: f64, items: &mut Vec<LayoutItem>, diagnostics: &mut Vec<Diagnostic>) {
    match node {
        PaintNode::Group { children, translate_x, translate_y } => {
            let nx = ox + translate_x.to_f64();
            let ny = oy + translate_y.to_f64();
            for child in children {
                push_paint(child, nx, ny, items, diagnostics);
            }
        }
        PaintNode::GlyphRun { positions, size, path_data, chars, family, .. } => {
            let scale = size.to_f64();
            for (i, d_opt) in path_data.iter().enumerate() {
                let Some(d) = d_opt
                else {
                    diagnostics.push(outline_missing_entry(chars.get(i).copied().flatten(), family));
                    continue;
                };
                let (px, py) = positions.get(i).copied().unwrap_or((Fixed::ZERO, Fixed::ZERO));
                let d = scale_svg_path_d(d, scale, -scale);
                let (min_x, min_y, max_x, max_y) = path_ink_bounds(&d);
                items.push(LayoutItem::Path(Path {
                    d,
                    x: ox + px.to_f64(),
                    y: oy + py.to_f64(),
                    width: max_x - min_x.min(0.0),
                    height: (-min_y).max(0.0),
                    depth: max_y.max(0.0),
                }));
            }
        }
        PaintNode::Rect { x, y, width, height, stroke_width, baseline } => {
            let bar_width = width.to_f64();
            let bar_height = height.to_f64();
            let left = ox + x.to_f64();
            let top = oy + y.to_f64();
            if bar_width <= 0.0 || !(bar_width.is_finite() && bar_height.is_finite()) {
                return;
            }
            if stroke_width.to_f64() > 0.0 {
                let centre = top + bar_height / 2.0;
                items.push(LayoutItem::Rule(Rule {
                    x1: left,
                    y1: centre,
                    x2: left + bar_width,
                    y2: centre,
                    thickness: stroke_width.to_f64(),
                }));
                return;
            }
            // The rule's own baseline split stays exact `Fixed` arithmetic so a
            // zero-depth bar never drifts into the boxed branch through
            // floating-point accumulation.
            let above = (*baseline - *y).to_f64();
            let drop = (*y + *height - *baseline).to_f64();
            let baseline_page = oy + baseline.to_f64();
            let thickness = above.max(MIN_RULE_THICKNESS);
            if drop <= 0.0 {
                let centre = baseline_page - thickness / 2.0;
                items.push(LayoutItem::Rule(Rule { x1: left, y1: centre, x2: left + bar_width, y2: centre, thickness }));
            }
            else {
                // A rule that descends below the baseline needs its full box,
                // which `LayoutItem::Rule` cannot express.
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
                    x: left,
                    y: baseline_page,
                    width: bar_width,
                    height: thickness,
                    depth: drop,
                }));
            }
        }
        PaintNode::VectorPath { d, x, y, scale, bbox } => {
            let height_above = (*y - bbox.y).to_f64().max(0.0);
            items.push(LayoutItem::Path(Path {
                d: scale_svg_path_d(d, scale.to_f64(), -scale.to_f64()),
                x: ox + x.to_f64(),
                y: oy + y.to_f64(),
                width: bbox.width.to_f64(),
                height: height_above,
                depth: (bbox.height - (*y - bbox.y)).to_f64().max(0.0),
            }));
        }
    }
}

/// Axis-aligned ink bounds of an absolute page-space path `d` (y-down).
///
/// Control points are included, so the box is a superset of the outline — the
/// same safe over-approximation the renderers use for their viewports.
fn path_ink_bounds(raw: &str) -> (f64, f64, f64, f64) {
    let mut bounds = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut count = 0usize;
    visit_path_points(raw, &mut |x, y| {
        bounds = (bounds.0.min(x), bounds.1.min(y), bounds.2.max(x), bounds.3.max(y));
        count += 1;
    });
    if count == 0 { (0.0, 0.0, 0.0, 0.0) } else { bounds }
}

/// Visit every on-curve/off-curve point of an absolute SVG path.
///
/// Layout only emits absolute `M`, `L`, `Q`, `C` and `Z`.
fn visit_path_points(d: &str, visit: &mut impl FnMut(f64, f64)) {
    let mut op = 'M';
    let mut args: Vec<f64> = Vec::new();
    let mut chars = d.chars().peekable();
    loop {
        let Some(c) = chars.peek().copied()
        else {
            break;
        };
        if c.is_ascii_alphabetic() {
            emit_points(op, &args, visit);
            args.clear();
            op = c;
            chars.next();
        }
        else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let mut token = String::new();
            if c == '-' || c == '+' {
                token.push(c);
                chars.next();
            }
            while let Some(&next) = chars.peek() {
                let exponent_sign = matches!(next, '-' | '+') && matches!(token.chars().last(), Some('e' | 'E'));
                if next.is_ascii_digit() || next == '.' || next == 'e' || next == 'E' || exponent_sign {
                    token.push(next);
                    chars.next();
                }
                else {
                    break;
                }
            }
            if let Ok(value) = token.parse::<f64>() {
                args.push(value);
            }
        }
        else {
            chars.next();
        }
    }
    emit_points(op, &args, visit);
}

/// Feed one operator's coordinate pairs to `visit`.
fn emit_points(op: char, args: &[f64], visit: &mut impl FnMut(f64, f64)) {
    let arity = match op {
        'M' | 'L' | 'T' => 2,
        'Q' | 'S' => 4,
        'C' => 6,
        'A' => 7,
        _ => return,
    };
    for chunk in args.chunks_exact(arity) {
        match op {
            'M' | 'L' | 'T' => visit(chunk[0], chunk[1]),
            'Q' | 'S' => {
                visit(chunk[0], chunk[1]);
                visit(chunk[2], chunk[3]);
            }
            'C' => {
                visit(chunk[0], chunk[1]);
                visit(chunk[2], chunk[3]);
                visit(chunk[4], chunk[5]);
            }
            'A' => visit(chunk[5], chunk[6]),
            _ => {}
        }
    }
}

/// Structure diagnostic for a glyph the math font ships no outline for.
fn outline_missing_entry(ch: Option<char>, family: &str) -> Diagnostic {
    let name = match ch {
        Some(ch) => format!("'{ch}' (U+{:04X})", ch as u32),
        None => "without a character".to_string(),
    };
    Diagnostic {
        code: "vutex.layout.math.outline_missing".into(),
        severity: Severity::Warning,
        message: format!(
            "math glyph {name} in family `{family}` has no embedded outline and was not painted; the box keeps its measured advance rather than falling back to a host font"
        ),
    }
}

/// Four-decimal number, matching the precision outlines are scaled with.
fn decimal(value: f64) -> String {
    format!("{value:.4}")
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
