//! Painting a resolved DaVinci display scene into PDF content stream operators.
//!
//! Scene coordinates put the origin at the top-left corner and point down, like
//! SVG, so the scene-to-page matrix flips the y axis once at the root and the
//! node transforms compose inside it. Text needs one extra flip in its text
//! matrix, otherwise glyphs would come out mirrored.
//!
//! Text width uses the scene crate's documented approximation
//! (`characters * font_size * 0.6`), which is what the host reserved space
//! with, so anchored labels land inside their boxes. Shape and group opacity is
//! not painted yet: only fully opaque scenes render identically.
//!
//! Images carry a resource reference rather than decoded pixels, so they keep
//! the dashed placeholder the writer already used for reserved image slots.

use std::fmt::Write as _;

use davinci_scene::{DisplayScene, MarkerShape, NodeKind, PathCommand, PathData, SceneNode, ShapeStyle, TextAnchor, TextStyle};
use davinci_types::{Color, Point, Rect, Transform};
use vutex_presentation::display::DisplayGraphic;

use crate::render::{EmbeddedText, base_font_for, needs_embedding, num, paint_text, scene_font_style};

/// Width of one character as a fraction of the font size.
const TEXT_WIDTH: f64 = 0.6;

/// Control-point offset that turns a quarter ellipse into a cubic Bézier.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// Paint one placed scene into the page content stream.
pub(crate) fn write_scene(
    out: &mut String,
    page_height: f64,
    graphic: &DisplayGraphic,
    fonts: &[(&'static str, usize)],
    embedded: Option<EmbeddedText<'_>>,
) {
    let scene = &graphic.scene;
    if !(scene.size.width > 0.0 && scene.size.height > 0.0) {
        return;
    }
    let sx = graphic.width / scene.size.width;
    let sy = graphic.height / scene.size.height;
    if !(sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0) {
        return;
    }
    out.push_str("q\n");
    // Clip to the placed box in page space before switching to scene space.
    let _ = write!(
        out,
        "{} {} {} {} re\nW n\n",
        num(graphic.x),
        num(page_height - (graphic.y + graphic.height)),
        num(graphic.width),
        num(graphic.height)
    );
    let _ = write!(out, "{} 0 0 {} {} {} cm\n", num(sx), num(-sy), num(graphic.x), num(page_height - graphic.y));
    for node in &scene.nodes {
        write_node(out, node, fonts, embedded);
    }
    out.push_str("Q\n");
}

/// Visit every text in the scene tree, with its style and content.
pub(crate) fn visit_texts(scene: &DisplayScene, visit: &mut impl FnMut(&TextStyle, &str)) {
    fn walk(nodes: &[SceneNode], visit: &mut impl FnMut(&TextStyle, &str)) {
        for node in nodes {
            match &node.kind {
                NodeKind::Text { text, style, .. } => visit(style, text),
                NodeKind::Group { children, .. } => walk(children, visit),
                _ => {}
            }
        }
    }
    walk(&scene.nodes, visit);
}

fn write_node(out: &mut String, node: &SceneNode, fonts: &[(&'static str, usize)], embedded: Option<EmbeddedText<'_>>) {
    out.push_str("q\n");
    if !node.transform.is_identity() {
        write_matrix(out, node.transform);
    }
    if let Some(clip) = node.clip {
        let _ = write!(out, "{} {} {} {} re\nW n\n", num(clip.x), num(clip.y), num(clip.width), num(clip.height));
    }
    write_kind(out, &node.kind, fonts, embedded);
    out.push_str("Q\n");
}

fn write_matrix(out: &mut String, transform: Transform) {
    let _ = write!(
        out,
        "{} {} {} {} {} {} cm\n",
        num(transform.a()),
        num(transform.b()),
        num(transform.c()),
        num(transform.d()),
        num(transform.e()),
        num(transform.f()),
    );
}

fn write_kind(out: &mut String, kind: &NodeKind, fonts: &[(&'static str, usize)], embedded: Option<EmbeddedText<'_>>) {
    match kind {
        NodeKind::Group { children, .. } => {
            for child in children {
                write_node(out, child, fonts, embedded);
            }
        }
        NodeKind::Path { path, style } => {
            write_path_data(out, path);
            paint(out, style);
        }
        NodeKind::Line { from, to, style } => {
            let _ = write!(out, "{} {} m\n{} {} l\n", num(from.x), num(from.y), num(to.x), num(to.y));
            paint(out, style);
        }
        NodeKind::Rect { rect, style } => {
            write_rect(out, *rect);
            paint(out, style);
        }
        NodeKind::RoundedRect { rect, radius, style } => {
            write_rounded_rect(out, *rect, *radius);
            paint(out, style);
        }
        NodeKind::Ellipse { center, radii, style } => {
            write_ellipse(out, *center, radii.width, radii.height);
            paint(out, style);
        }
        NodeKind::Text { position, text, style } => write_text(out, *position, text, style, fonts, embedded),
        NodeKind::Image { rect, .. } => write_image_placeholder(out, *rect),
        NodeKind::Marker { at, shape, size, style } => {
            write_marker(out, *at, *shape, *size);
            paint(out, style);
        }
    }
}

fn write_rect(out: &mut String, rect: Rect) {
    let _ = write!(out, "{} {} {} {} re\n", num(rect.x), num(rect.y), num(rect.width), num(rect.height));
}

fn write_rounded_rect(out: &mut String, rect: Rect, radius: f64) {
    let radius = radius.max(0.0).min(rect.width.abs() * 0.5).min(rect.height.abs() * 0.5);
    if radius <= 0.0 {
        write_rect(out, rect);
        return;
    }
    let control = radius * (1.0 - KAPPA);
    let (left, top, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    let _ = write!(out, "{} {} m\n", num(left + radius), num(top));
    let _ = write!(out, "{} {} l\n", num(right - radius), num(top));
    let _ = write!(
        out,
        "{} {} {} {} {} {} c\n",
        num(right - control),
        num(top),
        num(right),
        num(top + control),
        num(right),
        num(top + radius),
    );
    let _ = write!(out, "{} {} l\n", num(right), num(bottom - radius));
    let _ = write!(
        out,
        "{} {} {} {} {} {} c\n",
        num(right),
        num(bottom - control),
        num(right - control),
        num(bottom),
        num(right - radius),
        num(bottom),
    );
    let _ = write!(out, "{} {} l\n", num(left + radius), num(bottom));
    let _ = write!(
        out,
        "{} {} {} {} {} {} c\n",
        num(left + control),
        num(bottom),
        num(left),
        num(bottom - control),
        num(left),
        num(bottom - radius),
    );
    let _ = write!(out, "{} {} l\n", num(left), num(top + radius));
    let _ = write!(
        out,
        "{} {} {} {} {} {} c\n",
        num(left),
        num(top + control),
        num(left + control),
        num(top),
        num(left + radius),
        num(top),
    );
    out.push_str("h\n");
}

fn write_ellipse(out: &mut String, center: Point, rx: f64, ry: f64) {
    let (cx, cy) = (center.x, center.y);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let _ = write!(out, "{} {} m\n", num(cx + rx), num(cy));
    let _ = write!(out, "{} {} {} {} {} {} c\n", num(cx + rx), num(cy + ky), num(cx + kx), num(cy + ry), num(cx), num(cy + ry));
    let _ = write!(out, "{} {} {} {} {} {} c\n", num(cx - kx), num(cy + ry), num(cx - rx), num(cy + ky), num(cx - rx), num(cy));
    let _ = write!(out, "{} {} {} {} {} {} c\n", num(cx - rx), num(cy - ky), num(cx - kx), num(cy - ry), num(cx), num(cy - ry));
    let _ = write!(out, "{} {} {} {} {} {} c\n", num(cx + kx), num(cy - ry), num(cx + rx), num(cy - ky), num(cx + rx), num(cy));
    out.push_str("h\n");
}

fn write_marker(out: &mut String, at: Point, shape: MarkerShape, size: f64) {
    let half = size * 0.5;
    match shape {
        MarkerShape::Circle => write_ellipse(out, at, half, half),
        MarkerShape::Triangle => {
            let _ = write!(
                out,
                "{} {} m\n{} {} l\n{} {} l\nh\n",
                num(at.x),
                num(at.y - half),
                num(at.x + half),
                num(at.y + half),
                num(at.x - half),
                num(at.y + half),
            );
        }
        MarkerShape::Diamond => {
            let _ = write!(
                out,
                "{} {} m\n{} {} l\n{} {} l\n{} {} l\nh\n",
                num(at.x),
                num(at.y - half),
                num(at.x + half),
                num(at.y),
                num(at.x),
                num(at.y + half),
                num(at.x - half),
                num(at.y),
            );
        }
    }
}

fn write_path_data(out: &mut String, path: &PathData) {
    let mut current = Point::new(0.0, 0.0);
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(point) => {
                let _ = write!(out, "{} {} m\n", num(point.x), num(point.y));
                current = point;
            }
            PathCommand::LineTo(point) => {
                let _ = write!(out, "{} {} l\n", num(point.x), num(point.y));
                current = point;
            }
            PathCommand::QuadraticTo { control, to } => {
                // PDF has no quadratic segment, so lift it to an equivalent cubic.
                let c1 = Point::new(
                    current.x + 2.0 / 3.0 * (control.x - current.x),
                    current.y + 2.0 / 3.0 * (control.y - current.y),
                );
                let c2 = Point::new(to.x + 2.0 / 3.0 * (control.x - to.x), to.y + 2.0 / 3.0 * (control.y - to.y));
                let _ = write!(out, "{} {} {} {} {} {} c\n", num(c1.x), num(c1.y), num(c2.x), num(c2.y), num(to.x), num(to.y));
                current = to;
            }
            PathCommand::CubicTo { control1, control2, to } => {
                let _ = write!(
                    out,
                    "{} {} {} {} {} {} c\n",
                    num(control1.x),
                    num(control1.y),
                    num(control2.x),
                    num(control2.y),
                    num(to.x),
                    num(to.y),
                );
                current = to;
            }
            PathCommand::Close => out.push_str("h\n"),
        }
    }
}

/// Apply a shape style to the path just emitted, then consume it.
fn paint(out: &mut String, style: &ShapeStyle) {
    let stroke = style.stroke.filter(|_| style.stroke_width > 0.0);
    if let Some(fill) = style.fill {
        write_color(out, fill, true);
    }
    if let Some(stroke) = stroke {
        write_color(out, stroke, false);
        let _ = write!(out, "{} w\n", num(style.stroke_width));
    }
    match (style.fill.is_some(), stroke.is_some()) {
        (true, true) => out.push_str("B\n"),
        (true, false) => out.push_str("f\n"),
        (false, true) => out.push_str("S\n"),
        (false, false) => out.push_str("n\n"),
    }
}

fn write_color(out: &mut String, color: Color, fill: bool) {
    let operator = if fill { "rg" } else { "RG" };
    let _ = write!(
        out,
        "{} {} {} {operator}\n",
        num(f64::from(color.red()) / 255.0),
        num(f64::from(color.green()) / 255.0),
        num(f64::from(color.blue()) / 255.0),
    );
}

fn write_text(
    out: &mut String,
    position: Point,
    text: &str,
    style: &TextStyle,
    fonts: &[(&'static str, usize)],
    embedded: Option<EmbeddedText<'_>>,
) {
    if text.is_empty() || !(style.font_size > 0.0) {
        return;
    }
    let anchor = if embedded.is_some() && needs_embedding(text) {
        embedded.map(|embedded| embedded.resource_index)
    }
    else {
        let family = style.font_family.as_deref().unwrap_or("");
        let base = base_font_for(family, scene_font_style(style.font_weight));
        fonts.iter().position(|(font, _)| *font == base).map(|index| index + 1)
    };
    let Some(resource_index) = anchor
    else {
        return;
    };
    if let Some(fill) = style.fill {
        write_color(out, fill, true);
    }
    let width = text.chars().count() as f64 * style.font_size * TEXT_WIDTH;
    let x = match style.text_anchor {
        TextAnchor::Start => position.x,
        TextAnchor::Middle => position.x - width * 0.5,
        TextAnchor::End => position.x - width,
    };
    paint_text(out, x, position.y, style.font_size, resource_index, text, embedded, true);
}

/// Reserved image slot: the scene carries a reference, not pixels.
fn write_image_placeholder(out: &mut String, rect: Rect) {
    let _ = write!(
        out,
        "0.6 G\n0.5 w\n[3 2] 0 d\n{} {} {} {} re\nS\n[] 0 d\n",
        num(rect.x),
        num(rect.y),
        num(rect.width),
        num(rect.height),
    );
}
