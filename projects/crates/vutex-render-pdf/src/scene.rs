//! Painting a resolved DaVinci display scene into PDF content stream operators.
//!
//! Scene coordinates put the origin at the top-left corner and point down, like
//! SVG, so the scene-to-page matrix flips the y axis once at the root and the
//! node transforms compose inside it. Text needs one extra flip in its text
//! matrix, otherwise glyphs would come out mirrored.
//!
//! Text width uses the scene crate's documented approximation
//! (`characters * font_size * 0.6`), which is what the host reserved space
//! with, so anchored labels land inside their boxes. Opacity reaches the page
//! as an `/ExtGState` alpha entry: a node below full opacity opens its own
//! graphics state, so its siblings stay fully opaque.
//!
//! Images paint as raster XObjects when their resource resolves: a PNG
//! contributes unfiltered 8-bit samples and a JPEG keeps its `DCTDecode`
//! stream. The image matrix flips the sample rows so the picture stays upright
//! under the scene's own y flip. An SVG source instead paints its parsed scene
//! as vector geometry, fitted into the slot in scene coordinates. A reference
//! that cannot be resolved keeps the dashed placeholder the writer already
//! used for reserved image slots.

use std::fmt::Write as _;

use davinci_scene::{
    DisplayScene, MarkerShape, NodeKind, PathCommand, PathData, ResourceRef, SceneNode, ShapeStyle, TextAnchor, TextStyle,
};
use davinci_types::{Color, Point, Rect, Transform};
use vutex_presentation::display::DisplayGraphic;

use vutex_types::Diagnostic;

use crate::render::{
    EmbeddedText, ImageResources, OpacityResources, append_warning, base_font_for, needs_embedding, num, paint_text,
    scene_font_style,
};

/// Width of one character as a fraction of the font size.
const TEXT_WIDTH: f64 = 0.6;

/// Control-point offset that turns a quarter ellipse into a cubic Bézier.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// Paint context shared by every node of one placed scene.
#[derive(Debug)]
pub(crate) struct ScenePaint<'a, 'd> {
    /// Base-14 font resources available to text, in resource order.
    pub(crate) fonts: &'a [(&'static str, usize)],
    /// Embedded font subset, when the document needed one.
    pub(crate) embedded: Option<EmbeddedText<'a>>,
    /// Raster names and parsed SVG scenes keyed by their source `href`.
    pub(crate) images: &'a ImageResources,
    /// `/ExtGState` resources selecting the alpha a node paints with.
    pub(crate) opacities: &'a OpacityResources,
    /// Non-fatal diagnostics the painter can append to.
    pub(crate) diagnostics: Option<&'d mut Vec<Diagnostic>>,
}

/// Paint one placed scene into the page content stream.
pub(crate) fn write_scene<'d>(
    out: &mut String,
    page_height: f64,
    graphic: &DisplayGraphic,
    paint: &mut ScenePaint<'_, 'd>,
) {
    write_scene_box(out, page_height, &graphic.scene, graphic.x, graphic.y, graphic.width, graphic.height, paint);
}

/// Paint an SVG scene into a page-space image slot, preserving its aspect ratio.
///
/// The scene is uniformly scaled to the smaller of the two slot ratios and
/// centred, so a square icon stays square in a wide or tall slot. Returns
/// `false` when the scene or the slot has no paintable extent, letting the
/// caller fall back to the dashed placeholder.
pub(crate) fn write_scene_slot<'d>(
    out: &mut String,
    page_height: f64,
    scene: &DisplayScene,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    paint: &mut ScenePaint<'_, 'd>,
) -> bool {
    let Some((x, y, width, height)) = fit_scene(scene, x, y, width, height)
    else {
        return false;
    };
    write_scene_box(out, page_height, scene, x, y, width, height, paint);
    true
}

/// Uniformly fit a scene's natural size into a box, returning the centred box.
fn fit_scene(scene: &DisplayScene, x: f64, y: f64, width: f64, height: f64) -> Option<(f64, f64, f64, f64)> {
    let (scene_width, scene_height) = (scene.size.width, scene.size.height);
    if !(scene_width > 0.0 && scene_height > 0.0 && width > 0.0 && height > 0.0) {
        return None;
    }
    let scale = (width / scene_width).min(height / scene_height);
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let fitted_width = scene_width * scale;
    let fitted_height = scene_height * scale;
    Some((x + (width - fitted_width) * 0.5, y + (height - fitted_height) * 0.5, fitted_width, fitted_height))
}

/// Paint a scene into one page-space box, scaling it on both axes.
fn write_scene_box<'d>(
    out: &mut String,
    page_height: f64,
    scene: &DisplayScene,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    paint: &mut ScenePaint<'_, 'd>,
) {
    if !(scene.size.width > 0.0 && scene.size.height > 0.0) {
        return;
    }
    let sx = width / scene.size.width;
    let sy = height / scene.size.height;
    if !(sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0) {
        return;
    }
    out.push_str("q\n");
    // Clip to the placed box in page space before switching to scene space.
    let _ = write!(out, "{} {} {} {} re\nW n\n", num(x), num(page_height - (y + height)), num(width), num(height));
    // The scene counts y down from its top edge while PDF page space counts y
    // up, so `d` is negative and the translation is the page y of the box top.
    let _ = write!(out, "{} 0 0 {} {} {} cm\n", num(sx), num(-sy), num(x), num(page_height - y));
    for node in &scene.nodes {
        write_node(out, node, paint, 1.0);
    }
    out.push_str("Q\n");
}

/// Visit every image resource in the scene tree, in paint order.
pub(crate) fn visit_images(scene: &DisplayScene, visit: &mut impl FnMut(&ResourceRef)) {
    fn walk(nodes: &[SceneNode], visit: &mut impl FnMut(&ResourceRef)) {
        for node in nodes {
            match &node.kind {
                NodeKind::Image { resource, .. } => visit(resource),
                NodeKind::Group { children, .. } => walk(children, visit),
                _ => {}
            }
        }
    }
    walk(&scene.nodes, visit);
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

/// Visit the effective alpha of every painted node that is not fully opaque.
///
/// The traversal mirrors [`write_node`], so the collected values are exactly
/// the ones the painter looks up again as `/ExtGState` resources.
pub(crate) fn visit_opacities(scene: &DisplayScene, visit: &mut impl FnMut(f64)) {
    fn walk(nodes: &[SceneNode], inherited: f64, visit: &mut impl FnMut(f64)) {
        for node in nodes {
            match &node.kind {
                NodeKind::Group { children, opacity } => walk(children, alpha_of(inherited * *opacity), visit),
                kind => {
                    let alpha = alpha_of(inherited * kind_opacity(kind));
                    if alpha < 1.0 {
                        visit(alpha);
                    }
                }
            }
        }
    }
    walk(&scene.nodes, 1.0, visit);
}

/// Effective alpha for the product of inherited and node opacity.
///
/// Non-finite values are ignored, which paints the node fully opaque, and
/// values outside `0.0..=1.0` are clamped so malformed scenes never emit an
/// invalid `/ca` entry.
fn alpha_of(value: f64) -> f64 {
    if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 }
}

/// Opacity a painted node asks for, before the inherited group alpha.
fn kind_opacity(kind: &NodeKind) -> f64 {
    match kind {
        NodeKind::Path { style, .. }
        | NodeKind::Line { style, .. }
        | NodeKind::Rect { style, .. }
        | NodeKind::RoundedRect { style, .. }
        | NodeKind::Ellipse { style, .. }
        | NodeKind::Marker { style, .. } => style.opacity,
        NodeKind::Text { style, .. } => style.opacity,
        NodeKind::Image { opacity, .. } => *opacity,
        // Groups paint through their children, which [`write_node`] recurses
        // into with the composed alpha.
        NodeKind::Group { opacity, .. } => *opacity,
    }
}

fn write_node<'d>(out: &mut String, node: &SceneNode, paint: &mut ScenePaint<'_, 'd>, inherited: f64) {
    out.push_str("q\n");
    if !node.transform.is_identity() {
        write_matrix(out, node.transform);
    }
    if let Some(clip) = node.clip {
        let _ = write!(out, "{} {} {} {} re\nW n\n", num(clip.x), num(clip.y), num(clip.width), num(clip.height));
    }
    if let NodeKind::Group { children, opacity } = &node.kind {
        // A PDF transparency group would need a Form XObject, so group opacity
        // multiplies into descendants instead: children paint one after another
        // at the composed alpha and overlap individually rather than as one
        // flattened layer.
        let alpha = alpha_of(inherited * *opacity);
        for child in children {
            write_node(out, child, paint, alpha);
        }
    }
    else {
        let alpha = alpha_of(inherited * kind_opacity(&node.kind));
        // The enclosing `q` scopes the alpha to this node, so siblings stay
        // fully opaque.
        if let Some(name) = paint.opacities.name(alpha) {
            let _ = writeln!(out, "/{name} gs");
        }
        write_kind(out, &node.kind, paint);
    }
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

fn write_kind<'d>(out: &mut String, kind: &NodeKind, painter: &mut ScenePaint<'_, 'd>) {
    match kind {
        // Groups paint through their children, which [`write_node`] recurses
        // into so it can compose group opacity.
        NodeKind::Group { .. } => {}
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
        NodeKind::Text { position, text, style } => write_text(out, *position, text, style, painter),
        NodeKind::Image { rect, resource, .. } => write_image(out, *rect, resource, painter),
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

fn write_text<'d>(
    out: &mut String,
    position: Point,
    text: &str,
    style: &TextStyle,
    painter: &mut ScenePaint<'_, 'd>,
) {
    if text.is_empty() || !(style.font_size > 0.0) {
        return;
    }
    let embedded = painter.embedded;
    let anchor = if embedded.is_some() && needs_embedding(text) {
        embedded.map(|embedded| embedded.first_index)
    }
    else {
        let family = style.font_family.as_deref().unwrap_or("");
        let base = base_font_for(family, scene_font_style(style.font_weight));
        painter.fonts.iter().position(|(font, _)| *font == base).map(|index| index + 1)
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
    paint_text(
        out,
        x,
        position.y,
        style.font_size,
        resource_index,
        text,
        embedded,
        true,
        painter.diagnostics.as_deref_mut(),
    );
}

/// Paint one resolved image, or its reserved dashed slot when it did not resolve.
///
/// A raster uses `/ImN Do`. An SVG paints its parsed scene fitted into `rect`,
/// still in scene coordinates, so the enclosing scene's root flip keeps it
/// upright. Raster placement is unchanged: PDF image space places the first
/// sample row at the top of the unit square,
/// while scene space counts y downward. The `d` entry is therefore negative and
/// the matrix puts the unit square's bottom edge at `rect.y + rect.height`, so
/// the first row lands on the scene's top edge. The scene root already flips y
/// once, so the composed page matrix keeps a positive y scale and the picture
/// stays upright.
fn write_image<'d>(
    out: &mut String,
    rect: Rect,
    resource: &ResourceRef,
    painter: &mut ScenePaint<'_, 'd>,
) {
    if !all_finite(&[rect.x, rect.y, rect.width, rect.height]) {
        return;
    }
    if !(rect.width > 0.0 && rect.height > 0.0) {
        write_image_placeholder(out, rect);
        return;
    }
    if let Some(name) = painter.images.raster(&resource.href) {
        let _ = write!(
            out,
            "q\n{} 0 0 {} {} {} cm\n/{name} Do\nQ\n",
            num(rect.width),
            num(-rect.height),
            num(rect.x),
            num(rect.y + rect.height),
        );
        return;
    }
    if let Some(scene) = painter.images.scene(&resource.href)
        && write_nested_scene(out, scene, rect, painter)
    {
        return;
    }
    write_image_placeholder(out, rect);
}

/// Paint an SVG scene inside a scene-space slot.
///
/// Both the nested scene and the enclosing scene count y downward, so this
/// transformation only scales and translates: no flip is needed here because
/// the root scene matrix already flipped the whole scene once.
fn write_nested_scene<'d>(
    out: &mut String,
    scene: &DisplayScene,
    rect: Rect,
    painter: &mut ScenePaint<'_, 'd>,
) -> bool {
    let Some((x, y, width, _)) = fit_scene(scene, rect.x, rect.y, rect.width, rect.height)
    else {
        return false;
    };
    let scale = width / scene.size.width;
    out.push_str("q\n");
    let _ = write!(out, "{} 0 0 {} {} {} cm\n", num(scale), num(scale), num(x), num(y));
    for node in &scene.nodes {
        write_node(out, node, painter, 1.0);
    }
    out.push_str("Q\n");
    true
}

/// Whether every value is a finite number.
fn all_finite(values: &[f64]) -> bool {
    values.iter().copied().all(f64::is_finite)
}

/// Reserved image slot for a reference this backend could not resolve.
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
