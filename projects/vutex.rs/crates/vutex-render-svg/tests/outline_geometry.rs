//! Geometry regressions for the real glyph-outline pipeline.
//!
//! `SvgBackend` receives em-unit outlines whose y axis grows upward from the
//! baseline and paints them on a y-down canvas. These tests read the emitted
//! SVG back and assert that the viewport really contains the painted ink, that
//! the ink never runs away from the layout box, that emphasis faces keep their
//! own outlines, and that `Vmatrix` and `vmatrix` stay structurally distinct.

use vutex_layout::math::{FormulaLayout, layout_formula, render_layout_node};
use vutex_render_svg::math::SvgBackend;

/// Representative formulas, all rendered as vector paths.
const CASES: &[(&str, &str, bool)] = &[
    ("simple", "x + y", false),
    ("fraction", r"\frac{a}{b}", true),
    ("radical", r"\sqrt{x}", false),
    ("limits", r"\sum_{i=1}^{n} x_i", true),
    ("vmatrix", r"\begin{Vmatrix}a&b\\c&d\end{Vmatrix}", true),
];

fn render(source: &str, display: bool) -> (String, FormulaLayout) {
    let (root, layout) = layout_formula(source, display).expect("layout should succeed");
    let mut backend = SvgBackend::new_deterministic(layout.width, layout.height + layout.depth);
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("render should succeed");
    (backend.finish(), layout)
}

/// Read the root `viewBox` as `(min_x, min_y, width, height)`.
fn view_box(svg: &str) -> (f64, f64, f64, f64) {
    let values: Vec<f64> = svg_attribute(svg, "viewBox")
        .expect("root viewBox")
        .split_whitespace()
        .map(|value| value.parse().expect("viewBox number"))
        .collect();
    assert_eq!(values.len(), 4, "viewBox needs four numbers: {svg}");
    (values[0], values[1], values[2], values[3])
}

/// Read the first `name="value"` attribute of the root `<svg>` tag.
fn svg_attribute<'a>(svg: &'a str, name: &str) -> Option<&'a str> {
    let tag_end = svg.find('>')?;
    attribute(&svg[..tag_end], name)
}

/// Read one attribute out of a single tag body.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

/// Read the `(x, y)` numbers of the first `prefix` transform in `text`.
fn transform_pair(text: &str, prefix: &str) -> Option<(f64, f64)> {
    let start = text.find(prefix)? + prefix.len();
    let end = start + text[start..].find(')')?;
    let (x, y) = text[start..end].split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Every `d` attribute painted by the document, in document order.
fn outline_paths(svg: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut cursor = 0;
    while let Some(index) = svg[cursor..].find("<path d=\"").map(|i| i + cursor) {
        let start = index + "<path d=\"".len();
        let end = start + svg[start..].find('"').expect("path data terminator");
        paths.push(&svg[start..end]);
        cursor = end;
    }
    paths
}

/// Every point painted by the document, in canvas coordinates.
///
/// Group translations accumulate, and each path applies its own
/// `translate(x, y) scale(sx, sy)` on top — the same order the browser uses.
fn painted_points(svg: &str) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let mut saved: Vec<(f64, f64)> = Vec::new();
    let mut offset = (0.0, 0.0);
    let mut cursor = 0;
    while let Some(open) = svg[cursor..].find('<').map(|i| i + cursor) {
        let Some(length) = svg[open..].find('>')
        else {
            break;
        };
        let tag = &svg[open + 1..open + length];
        cursor = open + length + 1;
        if tag == "g" || tag.starts_with("g ") {
            saved.push(offset);
            if let Some((x, y)) = transform_pair(tag, "translate(") {
                offset = (offset.0 + x, offset.1 + y);
            }
        }
        else if tag == "/g" {
            offset = saved.pop().unwrap_or((0.0, 0.0));
        }
        else if tag.starts_with("path ") {
            let d = attribute(tag, "d").expect("path data");
            let transform = attribute(tag, "transform").unwrap_or_default();
            let (tx, ty) = transform_pair(transform, "translate(").unwrap_or((0.0, 0.0));
            let (sx, sy) = transform_pair(transform, "scale(").unwrap_or((1.0, 1.0));
            for (px, py) in d_points(d) {
                points.push((offset.0 + tx + sx * px, offset.1 + ty + sy * py));
            }
        }
    }
    points
}

/// Coordinate pairs of an absolute path whose `d` only uses `M`/`L`/`Q`/`C`.
fn d_points(d: &str) -> Vec<(f64, f64)> {
    assert!(
        d.chars().all(|c| c.is_whitespace() || "MLQCZ,".contains(c) || c.is_ascii_digit() || c == '.' || c == '-' || c == '+'),
        "unexpected path operator in {d}"
    );
    let mut points = Vec::new();
    let mut numbers: Vec<f64> = Vec::new();
    let mut operator = 'M';
    let mut token = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            push_number(&mut token, &mut numbers);
            flush_numbers(operator, &mut numbers, &mut points);
            operator = c;
        }
        else if c.is_whitespace() || c == ',' {
            push_number(&mut token, &mut numbers);
        }
        else {
            token.push(c);
        }
    }
    push_number(&mut token, &mut numbers);
    flush_numbers(operator, &mut numbers, &mut points);
    points
}

fn push_number(token: &mut String, numbers: &mut Vec<f64>) {
    if !token.is_empty() {
        numbers.push(token.parse().expect("path number"));
        token.clear();
    }
}

fn flush_numbers(operator: char, numbers: &mut Vec<f64>, points: &mut Vec<(f64, f64)>) {
    let arity = match operator {
        'M' | 'L' | 'T' => 2,
        'Q' | 'S' => 4,
        'C' => 6,
        _ => {
            numbers.clear();
            return;
        }
    };
    for chunk in numbers.chunks_exact(arity) {
        for pair in chunk.chunks(2) {
            points.push((pair[0], pair[1]));
        }
    }
    numbers.clear();
}

/// Every `<rect>` painted by the document as `(x, top, width, height)`, in
/// canvas coordinates.
///
/// Rules reach the backend as rectangles from the node's baseline, so the group
/// translations accumulate exactly like they do for [`painted_points`].
fn painted_rects(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    let mut rects = Vec::new();
    let mut saved: Vec<(f64, f64)> = Vec::new();
    let mut offset = (0.0, 0.0);
    let mut cursor = 0;
    while let Some(open) = svg[cursor..].find('<').map(|i| i + cursor) {
        let Some(length) = svg[open..].find('>')
        else {
            break;
        };
        let tag = &svg[open + 1..open + length];
        cursor = open + length + 1;
        if tag == "g" || tag.starts_with("g ") {
            saved.push(offset);
            if let Some((x, y)) = transform_pair(tag, "translate(") {
                offset = (offset.0 + x, offset.1 + y);
            }
        }
        else if tag == "/g" {
            offset = saved.pop().unwrap_or((0.0, 0.0));
        }
        else if tag.starts_with("rect ") {
            let number = |name| attribute(tag, name).expect("rect attribute").parse::<f64>().expect("rect number");
            rects.push((offset.0 + number("x"), offset.1 + number("y"), number("width"), number("height")));
        }
    }
    rects
}

/// Count closed, straight-sided subpaths.
///
/// Bar delimiters are emitted as rectangles, while glyph outlines are cubic, so
/// a `d` without curve operators is a bar and its `M` count is its stroke count.
fn bar_strokes(svg: &str) -> usize {
    outline_paths(svg).iter().filter(|d| !d.contains('C') && !d.contains('Q')).map(|d| d.matches('M').count()).sum()
}

#[test]
fn view_box_contains_every_painted_point() {
    for (name, source, display) in CASES {
        let (svg, _) = render(source, *display);
        let (min_x, min_y, width, height) = view_box(&svg);
        let points = painted_points(&svg);
        assert!(!points.is_empty(), "{name}: nothing was painted");
        for (x, y) in points {
            assert!(
                x >= min_x - 1e-3 && x <= min_x + width + 1e-3 && y >= min_y - 1e-3 && y <= min_y + height + 1e-3,
                "{name}: point ({x}, {y}) falls outside viewBox ({min_x}, {min_y}, {width}, {height})"
            );
        }
    }
}

#[test]
fn painted_ink_stays_inside_the_layout_box() {
    for (name, source, display) in CASES {
        let (svg, layout) = render(source, *display);
        let (min_x, min_y, width, height) = view_box(&svg);
        let box_height = layout.height + layout.depth;
        let pad = (layout.width.max(box_height) * 0.04).max(0.5);
        assert!(min_x >= -pad - 1e-3 && min_y >= -pad - 1e-3, "{name}: ink starts above or left of the layout box:\n{svg}");
        assert!(
            min_x + width <= layout.width + pad + 1e-3 && min_y + height <= box_height + pad + 1e-3,
            "{name}: ink {min_x}..{} x {min_y}..{} runs past the {} x {box_height} layout box + {pad} pad:\n{svg}",
            min_x + width,
            min_y + height,
            layout.width
        );
    }
}

#[test]
fn simple_sum_extents_stay_in_one_em_range() {
    // Inline math lays out at a 14px em. `x + y` is a few em wide and a little
    // under one em tall; outlines painted in the wrong direction blow past this.
    let (svg, layout) = render("x + y", false);
    let em = 14.0;
    let width = layout.width / em;
    let height = (layout.height + layout.depth) / em;
    assert!((1.5..4.0).contains(&width), "{width} em wide is not plausible for `x + y`");
    assert!((0.5..1.2).contains(&height), "{height} em tall is not plausible for `x + y`");
    let (_, _, view_width, view_height) = view_box(&svg);
    assert!(
        view_width >= layout.width - 1e-9 && view_height >= layout.height + layout.depth - 1e-9,
        "viewport must cover the layout box:\n{svg}"
    );
}

#[test]
fn thin_space_paints_nothing_in_deterministic_mode() {
    let (svg, layout) = render(r"a\,b", false);
    assert!(!svg.contains("<text"), "deterministic mode must not fall back to a host font:\n{svg}");
    assert_eq!(outline_paths(&svg).len(), 2, "only the two letters carry ink:\n{svg}");
    assert!(layout.width > render("ab", false).1.width, "`\\,` still advances the formula");
}

/// Painted ink extent `(min_y, max_y)` in canvas coordinates.
fn ink_y_extent(svg: &str) -> (f64, f64) {
    let points = painted_points(svg);
    assert!(!points.is_empty(), "nothing was painted:\n{svg}");
    let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    (min_y, max_y)
}

#[test]
fn stretched_delimiters_paint_their_own_outlines() {
    // Inline math lays out at a 14px em. Parentheses wrapped around a fraction
    // paint two extra outlines, which only happens when the variant glyph keys
    // resolve to the font's own extents and paths.
    let (svg, layout) = render(r"\left(\frac{a}{b}\right)", false);
    let paths = outline_paths(&svg);
    assert_eq!(paths.len(), 4, "two letters and two delimiter outlines:\n{svg}");
    let (_, _, view_width, view_height) = view_box(&svg);
    assert!(view_width >= layout.width - 1e-9 && view_height >= layout.height + layout.depth - 1e-9, "{svg}");
}

#[test]
fn delimiters_cover_the_body_they_wrap() {
    // A stretched delimiter is sized from the taller side of the math axis, so
    // the parentheses must paint on both sides and enclose the fraction instead
    // of sticking to the one-em minimum.
    let bare = r"\frac{a}{b}";
    let (bare_svg, bare_layout) = render(bare, false);
    let (wrapped_svg, wrapped_layout) = render(r"\left(\frac{a}{b}\right)", false);
    assert_eq!(
        outline_paths(&wrapped_svg).len(),
        outline_paths(&bare_svg).len() + 2,
        "both parentheses must paint their own outline:\n{wrapped_svg}"
    );
    assert!(
        wrapped_layout.height >= bare_layout.height && wrapped_layout.depth >= bare_layout.depth,
        "delimiter {} x {} does not cover the fraction {} x {}:\n{wrapped_svg}",
        wrapped_layout.height,
        wrapped_layout.depth,
        bare_layout.height,
        bare_layout.depth
    );
}

#[test]
fn tall_brace_is_assembled_from_stacked_parts() {
    // A brace around two nested fractions needs the piecewise assembly: no
    // single prepared variant covers that height.
    let bare = r"\frac{\frac{a}{b}}{\frac{c}{d}}";
    let (bare_svg, _) = render(bare, false);
    let (wrapped_svg, _) = render(r"\left\{\frac{\frac{a}{b}}{\frac{c}{d}}\right\}", false);
    assert!(
        outline_paths(&wrapped_svg).len() >= outline_paths(&bare_svg).len() + 2,
        "the brace must add assembled parts:\n{wrapped_svg}"
    );
    let (bare_min, bare_max) = ink_y_extent(&bare_svg);
    let (min, max) = ink_y_extent(&wrapped_svg);
    assert!(max - min >= bare_max - bare_min - 1.0, "the brace is shorter than the content it wraps:\n{wrapped_svg}");
}

#[test]
fn emphasis_faces_keep_their_own_outlines() {
    let bold_svg = render(r"\mathbf{a}", false).0;
    let italic_svg = render(r"\mathit{a}", false).0;
    let upright_svg = render(r"\mathrm{a}", false).0;
    let bold = outline_paths(&bold_svg);
    let italic = outline_paths(&italic_svg);
    let upright = outline_paths(&upright_svg);
    assert_eq!((bold.len(), italic.len(), upright.len()), (1, 1, 1), "each face paints exactly one glyph");
    assert_ne!(bold[0], upright[0], "`\\mathbf` must not reuse the upright outline");
    assert_ne!(italic[0], upright[0], "`\\mathit` must not reuse the upright outline");
    assert_ne!(bold[0], italic[0], "bold and italic must not share one outline");
    assert!(bold[0].len() > upright[0].len(), "bold is built from several offset copies");
}

/// `\hat` paints the font's circumflex next to the nucleus, never a host-font
/// run, so accent geometry stays deterministic.
#[test]
fn hat_accent_paints_the_font_circumflex() {
    let (svg, _) = render(r"\hat{x}", false);
    assert!(!svg.contains("<text"), "`\\hat` must not fall back to a host font:\n{svg}");
    assert_eq!(outline_paths(&svg).len(), 2, "the nucleus and the circumflex carry one outline each:\n{svg}");
}

/// `\bar` covers the nucleus it sits on: the font's macron is a text mark that
/// does not span a wide base, so TeX draws an overbar rule across the whole box.
#[test]
fn bar_accent_paints_a_rule_across_the_nucleus() {
    let (svg, _) = render(r"\bar{x}", false);
    assert!(!svg.contains("<text"), "`\\bar` must not fall back to a host font:\n{svg}");
    assert_eq!(outline_paths(&svg).len(), 1, "only the nucleus carries an outline:\n{svg}");

    let rects = painted_rects(&svg);
    assert_eq!(rects.len(), 1, "`\\bar` must paint exactly one overbar rule:\n{svg}");
    let (rule_x, rule_top, rule_width, rule_height) = rects[0];
    assert!(rule_width > 0.0 && rule_height > 0.0, "the overbar needs a positive size: {rects:?}\n{svg}");

    let points = painted_points(&svg);
    let ink_left = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let ink_right = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let ink_top = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!(
        rule_x <= ink_left + 1e-3 && rule_x + rule_width >= ink_right - 1e-3,
        "overbar {rule_x}..{} must cover the nucleus ink {ink_left}..{ink_right}:\n{svg}",
        rule_x + rule_width
    );
    assert!(
        rule_top + rule_height <= ink_top + 1e-3,
        "overbar bottom {} must clear the nucleus ink top {ink_top}:\n{svg}",
        rule_top + rule_height
    );
}

#[test]
fn vmatrix_and_Vmatrix_use_different_bars() {
    let upper = render(r"\begin{Vmatrix}a&b\\c&d\end{Vmatrix}", true).0;
    let lower = render(r"\begin{vmatrix}a&b\\c&d\end{vmatrix}", true).0;
    // Each matrix paints a delimiter on both sides, so `Vmatrix` (two strokes
    // per side) totals four bar subpaths and `vmatrix` totals two.
    assert_eq!(bar_strokes(&upper), 4, "`Vmatrix` needs two bar strokes per side:\n{upper}");
    assert_eq!(bar_strokes(&lower), 2, "`vmatrix` needs one bar stroke per side:\n{lower}");
}
