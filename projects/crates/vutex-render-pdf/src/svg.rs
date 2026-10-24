//! Focused SVG-to-scene parsing for icon artwork.
//!
//! A scene image node or a page-level image slot may name artwork authored as
//! SVG. This module turns that document into a [`DisplayScene`], which the
//! scene painter already knows how to turn into path, fill, stroke, and alpha
//! operators, so an icon paints as vector geometry instead of a raster.
//!
//! Supported document subset:
//!
//! - `<svg>` with `width`, `height`, and `viewBox`, honouring the default
//!   centred `xMidYMid meet` fit (and `none`) so artwork keeps its aspect
//!   ratio.
//! - `<g>` and `<a>` with `transform` and `opacity`.
//! - `<path>`, `<rect>` (with `rx`/`ry`), `<circle>`, `<ellipse>`, `<line>`,
//!   `<polyline>`, and `<polygon>`.
//! - Path data `M m L l H h V v C c S s Q q T t Z z`, including implicit
//!   repeated coordinate sets and smooth-curve reflection.
//! - Presentation attributes `fill`, `stroke`, `stroke-width`,
//!   `fill-opacity`, `opacity`, and `color`, plus the same properties inside a
//!   `style` attribute.
//! - `none`, `currentColor` (black unless a `color` attribute says otherwise),
//!   `#rgb`, `#rrggbb`, and the names in [`NAMED_COLORS`].
//!
//! Deliberate omissions. Each of these makes [`parse`] return `None` so the
//! slot keeps its dashed placeholder rather than painting something wrong:
//! elliptical arcs (`A`/`a`), gradient and pattern paint (`url(#…)`),
//! `<style>` rules and CSS classes, `<use>`, `<symbol>`, masks, clip paths,
//! filters, markers, text (`<text>` needs a font shaper and a text layout
//! contract), and nested `<svg>` documents. `<title>`, `<desc>`,
//! `<metadata>`, and `<defs>` paint nothing and are skipped.
//!
//! XML parsing is deliberately small: comments, processing instructions,
//! CDATA, and a DOCTYPE are skipped, entities in attribute values are decoded,
//! and an unbalanced or unterminated document fails closed.

use davinci_scene::{DisplayScene, NodeKind, PathCommand, PathData, SceneNode, ShapeStyle};
use davinci_types::{Color, Point, Rect, Size, Transform};

/// Elements that paint nothing and whose content is skipped.
const IGNORED: [&str; 7] = ["title", "desc", "metadata", "defs", "text", "tspan", "namedview"];

/// Elements this parser cannot honour without painting something wrong.
const UNSUPPORTED: [&str; 18] = [
    "use",
    "image",
    "style",
    "script",
    "symbol",
    "marker",
    "pattern",
    "mask",
    "clippath",
    "filter",
    "lineargradient",
    "radialgradient",
    "foreignobject",
    "textpath",
    "animate",
    "animatetransform",
    "animatemotion",
    "set",
];

/// Named colours this parser understands, in their CSS spelling.
const NAMED_COLORS: [(&str, Color); 18] = [
    ("black", Color::rgb(0, 0, 0)),
    ("silver", Color::rgb(192, 192, 192)),
    ("gray", Color::rgb(128, 128, 128)),
    ("grey", Color::rgb(128, 128, 128)),
    ("white", Color::rgb(255, 255, 255)),
    ("maroon", Color::rgb(128, 0, 0)),
    ("red", Color::rgb(255, 0, 0)),
    ("purple", Color::rgb(128, 0, 128)),
    ("fuchsia", Color::rgb(255, 0, 255)),
    ("magenta", Color::rgb(255, 0, 255)),
    ("green", Color::rgb(0, 128, 0)),
    ("lime", Color::rgb(0, 255, 0)),
    ("olive", Color::rgb(128, 128, 0)),
    ("yellow", Color::rgb(255, 255, 0)),
    ("navy", Color::rgb(0, 0, 128)),
    ("blue", Color::rgb(0, 0, 255)),
    ("teal", Color::rgb(0, 128, 128)),
    ("aqua", Color::rgb(0, 255, 255)),
];

/// Whether one source names an SVG document.
///
/// A source counts as SVG when the caller supplied an `image/svg+xml` mime, an
/// inline `data:` URI declares that type, or the path ends in `.svg`.
pub(crate) fn is_svg_source(source: &str, mime: Option<&str>) -> bool {
    if mime.is_some_and(|mime| mime.trim().eq_ignore_ascii_case("image/svg+xml")) {
        return true;
    }
    let source = source.trim();
    if source.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:")) {
        let Some((meta, _)) = source[5..].split_once(',')
        else {
            return false;
        };
        return meta.split(';').next().is_some_and(|kind| kind.trim().eq_ignore_ascii_case("image/svg+xml"));
    }
    let path = source.split(['?', '#']).next().unwrap_or(source);
    path.to_ascii_lowercase().ends_with(".svg")
}

/// Parse one SVG document into a display scene, or `None` when it uses a
/// construct this subset does not support.
pub(crate) fn parse(raw: &[u8]) -> Option<DisplayScene> {
    let text = std::str::from_utf8(raw).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut xml = Xml { text, at: 0 };
    let mut frames: Vec<Frame> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut root: Option<(Size, Transform)> = None;
    let mut finished: Option<DisplayScene> = None;
    let mut painted = false;
    while let Some(token) = xml.next_token()? {
        match token {
            Token::Start { name, attrs, self_closing } => {
                if !skipped.is_empty() {
                    if !self_closing {
                        skipped.push(name);
                    }
                    continue;
                }
                if name == "svg" {
                    if root.is_some() || !frames.is_empty() {
                        return None;
                    }
                    let (size, transform) = svg_box(&attrs)?;
                    root = Some((size, transform));
                    if self_closing {
                        finished = Some(build_scene(size, transform, Vec::new()));
                    }
                    else {
                        frames.push(Frame {
                            tag: name,
                            nodes: Vec::new(),
                            transform: Transform::IDENTITY,
                            opacity: 1.0,
                            style: Style::default(),
                        });
                    }
                    continue;
                }
                if IGNORED.contains(&name.as_str()) {
                    if !self_closing {
                        skipped.push(name);
                    }
                    continue;
                }
                if UNSUPPORTED.contains(&name.as_str()) {
                    return None;
                }
                let Some(parent) = frames.last()
                else {
                    return None;
                };
                let mut local_opacity = 1.0;
                let mut hidden = false;
                let style = styled(&parent.style, &attrs, &mut local_opacity, &mut hidden)?;
                let transform = match attr(&attrs, "transform") {
                    Some(value) => parse_transform(value)?,
                    None => Transform::IDENTITY,
                };
                if hidden {
                    if !self_closing {
                        frames.push(Frame { tag: name, nodes: Vec::new(), transform, opacity: local_opacity, style });
                    }
                    continue;
                }
                match name.as_str() {
                    "g" | "a" => {
                        frames.push(Frame { tag: name, nodes: Vec::new(), transform, opacity: local_opacity, style });
                    }
                    _ if is_shape(&name) => {
                        if let Some(node) = shape(&name, &attrs, &style, local_opacity, transform)? {
                            painted = true;
                            frames.last_mut()?.nodes.push(node);
                        }
                        if !self_closing {
                            frames.push(Frame { tag: name, nodes: Vec::new(), transform, opacity: local_opacity, style });
                        }
                    }
                    // Unknown elements paint nothing, so their content is
                    // skipped rather than guessed at.
                    _ => {
                        if !self_closing {
                            skipped.push(name);
                        }
                    }
                }
            }
            Token::End { name } => {
                if !skipped.is_empty() {
                    let open = skipped.pop()?;
                    if open != name {
                        return None;
                    }
                    continue;
                }
                let frame = frames.pop()?;
                if frame.tag != name {
                    return None;
                }
                if frame.tag == "svg" {
                    let (size, transform) = root.take()?;
                    finished = Some(build_scene(size, transform, frame.nodes));
                }
                else if frame.tag == "g" || frame.tag == "a" {
                    let node = SceneNode::new(NodeKind::Group { children: frame.nodes, opacity: frame.opacity })
                        .with_transform(frame.transform);
                    frames.last_mut()?.nodes.push(node);
                }
            }
        }
    }
    if !frames.is_empty() || !skipped.is_empty() {
        return None;
    }
    let scene = finished?;
    if !painted {
        return None;
    }
    Some(scene)
}

/// Wrap painted nodes in the root group that carries the `viewBox` mapping.
fn build_scene(size: Size, transform: Transform, children: Vec<SceneNode>) -> DisplayScene {
    let mut scene = DisplayScene::new(size);
    scene.push(SceneNode::new(NodeKind::Group { children, opacity: 1.0 }).with_transform(transform));
    scene
}

/// One open element, with the style its children inherit.
#[derive(Debug)]
struct Frame {
    /// Normalized element name, used for end-tag matching.
    tag: String,
    /// Children collected while the element is open.
    nodes: Vec<SceneNode>,
    /// Local transform applied to the element and its children.
    transform: Transform,
    /// Local group opacity.
    opacity: f64,
    /// Fill and stroke state inherited by children.
    style: Style,
}

/// Inherited paint state for one element subtree.
#[derive(Clone, Debug)]
struct Style {
    /// Interior paint, or `None` for no fill.
    fill: Option<Color>,
    /// Outline paint, or `None` for no outline.
    stroke: Option<Color>,
    /// Outline width in scene units.
    stroke_width: f64,
    /// Fill alpha multiplier inherited from `fill-opacity`.
    fill_opacity: f64,
    /// Colour the `currentColor` keyword resolves to.
    current: Color,
}

impl Default for Style {
    fn default() -> Self {
        // SVG's initial paint: solid black fill, no stroke, unit width.
        Self { fill: Some(Color::BLACK), stroke: None, stroke_width: 1.0, fill_opacity: 1.0, current: Color::BLACK }
    }
}

/// Whether an element name is a shape this parser builds.
fn is_shape(name: &str) -> bool {
    matches!(name, "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon")
}

/// Build one shape node, or `None` when the element paints no geometry.
///
/// The outer `Option` reports an unsupported or malformed attribute, which
/// makes the whole document fall back to the placeholder.
fn shape(
    name: &str,
    attrs: &[(String, String)],
    style: &Style,
    local_opacity: f64,
    transform: Transform,
) -> Option<Option<SceneNode>> {
    let opacity = if style.fill.is_some() { local_opacity * style.fill_opacity } else { local_opacity };
    let shape_style = ShapeStyle {
        fill: style.fill,
        stroke: style.stroke,
        stroke_width: style.stroke_width,
        opacity: opacity.clamp(0.0, 1.0),
    };
    let kind = match name {
        "path" => {
            let data = attr(attrs, "d")?;
            let path = parse_path(data)?;
            let draws = path.commands.iter().any(|command| !matches!(command, PathCommand::MoveTo(_)));
            if !draws {
                return Some(None);
            }
            NodeKind::Path { path, style: shape_style }
        }
        "rect" => {
            let x = length(attrs, "x")?;
            let y = length(attrs, "y")?;
            let width = length(attrs, "width")?;
            let height = length(attrs, "height")?;
            if !(width > 0.0 && height > 0.0) {
                return Some(None);
            }
            let rx = match attr(attrs, "rx") {
                Some(value) => parse_length(value)?,
                None => 0.0,
            };
            let ry = match attr(attrs, "ry") {
                Some(value) => parse_length(value)?,
                None => 0.0,
            };
            // A single radius applies to both axes, as SVG specifies. The
            // scene can only express circular corners, so an elliptical pair
            // collapses to the smaller radius.
            let (rx, ry) = match (rx > 0.0, ry > 0.0) {
                (true, false) => (rx, rx),
                (false, true) => (ry, ry),
                _ => (rx, ry),
            };
            if rx > 0.0 && ry > 0.0 {
                NodeKind::RoundedRect { rect: Rect::new(x, y, width, height), radius: rx.min(ry), style: shape_style }
            }
            else {
                NodeKind::Rect { rect: Rect::new(x, y, width, height), style: shape_style }
            }
        }
        "circle" => {
            let cx = length(attrs, "cx")?;
            let cy = length(attrs, "cy")?;
            let r = length(attrs, "r")?;
            if !(r > 0.0) {
                return Some(None);
            }
            NodeKind::Ellipse { center: Point::new(cx, cy), radii: Size::new(r, r), style: shape_style }
        }
        "ellipse" => {
            let cx = length(attrs, "cx")?;
            let cy = length(attrs, "cy")?;
            let rx = length(attrs, "rx")?;
            let ry = length(attrs, "ry")?;
            if !(rx > 0.0 && ry > 0.0) {
                return Some(None);
            }
            NodeKind::Ellipse { center: Point::new(cx, cy), radii: Size::new(rx, ry), style: shape_style }
        }
        "line" => {
            let from = Point::new(length(attrs, "x1")?, length(attrs, "y1")?);
            let to = Point::new(length(attrs, "x2")?, length(attrs, "y2")?);
            if from == to {
                return Some(None);
            }
            NodeKind::Line { from, to, style: shape_style }
        }
        "polyline" | "polygon" => {
            let points = attr(attrs, "points")?;
            let points = parse_points(points)?;
            if points.len() < 2 {
                return Some(None);
            }
            let mut path = PathData::new().move_to(points[0]);
            for point in &points[1..] {
                path = path.line_to(*point);
            }
            if name == "polygon" {
                path = path.close();
            }
            NodeKind::Path { path, style: shape_style }
        }
        _ => return Some(None),
    };
    Some(Some(SceneNode::new(kind).with_transform(transform)))
}

/// The scene canvas and root transform an `<svg>` element asks for.
///
/// A missing `width` or `height` falls back to the matching `viewBox` extent.
/// With both an element size and a `viewBox`, the default fit scales uniformly
/// and centres the content, so artwork keeps its aspect ratio; a
/// `preserveAspectRatio="none"` stretches instead.
fn svg_box(attrs: &[(String, String)]) -> Option<(Size, Transform)> {
    let view_box = match attr(attrs, "viewbox") {
        Some(value) => Some(parse_view_box(value)?),
        None => None,
    };
    let width = match attr(attrs, "width") {
        Some(value) => Some(parse_length(value)?),
        None => None,
    };
    let height = match attr(attrs, "height") {
        Some(value) => Some(parse_length(value)?),
        None => None,
    };
    match (view_box, width, height) {
        (Some((min_x, min_y, box_width, box_height)), width, height) => {
            let width = width.unwrap_or(box_width);
            let height = height.unwrap_or(box_height);
            if !(width > 0.0 && height > 0.0) {
                return None;
            }
            let transform =
                view_box_transform(min_x, min_y, box_width, box_height, width, height, attr(attrs, "preserveaspectratio"))?;
            Some((Size::new(width, height), transform))
        }
        (None, Some(width), Some(height)) => {
            if !(width > 0.0 && height > 0.0) {
                return None;
            }
            Some((Size::new(width, height), Transform::IDENTITY))
        }
        _ => None,
    }
}

/// Map a `viewBox` onto an element box.
fn view_box_transform(
    min_x: f64,
    min_y: f64,
    box_width: f64,
    box_height: f64,
    width: f64,
    height: f64,
    aspect: Option<&str>,
) -> Option<Transform> {
    let mode = aspect.map(|value| value.trim().to_ascii_lowercase()).unwrap_or_default();
    if mode.is_empty() {
        let scale = (width / box_width).min(height / box_height);
        return Some(Transform::new(
            scale,
            0.0,
            0.0,
            scale,
            (width - box_width * scale) * 0.5 - min_x * scale,
            (height - box_height * scale) * 0.5 - min_y * scale,
        ));
    }
    if mode.starts_with("xmidymid") {
        if mode.contains("slice") {
            return None;
        }
        let scale = (width / box_width).min(height / box_height);
        return Some(Transform::new(
            scale,
            0.0,
            0.0,
            scale,
            (width - box_width * scale) * 0.5 - min_x * scale,
            (height - box_height * scale) * 0.5 - min_y * scale,
        ));
    }
    if mode == "none" {
        let sx = width / box_width;
        let sy = height / box_height;
        return Some(Transform::new(sx, 0.0, 0.0, sy, -min_x * sx, -min_y * sy));
    }
    None
}

/// Parse the four numbers of a `viewBox`.
fn parse_view_box(value: &str) -> Option<(f64, f64, f64, f64)> {
    let numbers = parse_numbers(value)?;
    if numbers.len() != 4 {
        return None;
    }
    let (min_x, min_y, width, height) = (numbers[0], numbers[1], numbers[2], numbers[3]);
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    Some((min_x, min_y, width, height))
}

/// Parse an SVG `transform` list into a composed matrix.
///
/// The list applies left to right like SVG does: `translate(…)
/// scale(…)` scales the local coordinates before translating them, so the
/// composed matrix is `translate × scale`.
fn parse_transform(value: &str) -> Option<Transform> {
    let mut result = Transform::IDENTITY;
    let mut rest = value.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim().to_ascii_lowercase();
        let close = rest[open..].find(')')? + open;
        let arguments = parse_numbers(&rest[open + 1..close])?;
        let function = match name.as_str() {
            "translate" => match arguments.as_slice() {
                [x] => Transform::translate(*x, 0.0),
                [x, y] => Transform::translate(*x, *y),
                _ => return None,
            },
            "scale" => match arguments.as_slice() {
                [scale] => Transform::scale(*scale, *scale),
                [x, y] => Transform::scale(*x, *y),
                _ => return None,
            },
            "rotate" => match arguments.as_slice() {
                [angle] => Transform::rotate_degrees(*angle),
                [angle, cx, cy] => Transform::translate(*cx, *cy)
                    .then(Transform::rotate_degrees(*angle))
                    .then(Transform::translate(-*cx, -*cy)),
                _ => return None,
            },
            "matrix" => match arguments.as_slice() {
                [a, b, c, d, e, f] => Transform::new(*a, *b, *c, *d, *e, *f),
                _ => return None,
            },
            // `skewX` and `skewY` have no exact scene equivalent here.
            _ => return None,
        };
        // `function.then(result)` applies `function` first, which matches the
        // left-to-right reading of an SVG transform list.
        result = function.then(result);
        rest = rest[close + 1..].trim_start();
        if let Some(tail) = rest.strip_prefix(',') {
            rest = tail.trim_start();
        }
    }
    Some(result)
}

/// Parse path data into absolute scene commands.
///
/// Arcs (`A`/`a`) are refused: painting them as lines would silently change the
/// artwork, so the caller falls back to the dashed placeholder. Repeated
/// coordinate sets and smooth-curve reflection follow the SVG rules.
fn parse_path(data: &str) -> Option<PathData> {
    let mut reader = PathReader { bytes: data.as_bytes(), at: 0 };
    let mut path = PathData::new();
    let mut current = Point::new(0.0, 0.0);
    let mut subpath = Point::new(0.0, 0.0);
    let mut previous: Option<u8> = None;
    let mut cubic: Option<Point> = None;
    let mut quadratic: Option<Point> = None;
    let mut command: Option<u8> = None;
    let mut first = true;
    loop {
        reader.skip_separators();
        if reader.eof() {
            break;
        }
        if let Some(byte) = reader.peek_command() {
            reader.at += 1;
            if first && !matches!(byte, b'M' | b'm') {
                return None;
            }
            command = Some(byte);
        }
        let cmd = command?;
        if matches!(cmd, b'Z' | b'z') {
            path.commands.push(PathCommand::Close);
            current = subpath;
            cubic = None;
            quadratic = None;
            previous = Some(cmd);
            command = None;
            first = false;
            continue;
        }
        reader.skip_separators();
        if reader.eof() || reader.peek_command().is_some() {
            return None;
        }
        let relative = cmd.is_ascii_lowercase();
        let offset = |point: Point| if relative { current.offset(point.x, point.y) } else { point };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let point = offset(reader.point()?);
                path.commands.push(PathCommand::MoveTo(point));
                current = point;
                subpath = point;
                cubic = None;
                quadratic = None;
                command = Some(if relative { b'l' } else { b'L' });
            }
            b'L' => {
                let point = offset(reader.point()?);
                path.commands.push(PathCommand::LineTo(point));
                current = point;
                cubic = None;
                quadratic = None;
            }
            b'H' => {
                let x = reader.number()?;
                let point = Point::new(if relative { current.x + x } else { x }, current.y);
                path.commands.push(PathCommand::LineTo(point));
                current = point;
                cubic = None;
                quadratic = None;
            }
            b'V' => {
                let y = reader.number()?;
                let point = Point::new(current.x, if relative { current.y + y } else { y });
                path.commands.push(PathCommand::LineTo(point));
                current = point;
                cubic = None;
                quadratic = None;
            }
            b'C' => {
                let control1 = offset(reader.point()?);
                let control2 = offset(reader.point()?);
                let to = offset(reader.point()?);
                path.commands.push(PathCommand::CubicTo { control1, control2, to });
                current = to;
                cubic = Some(control2);
                quadratic = None;
            }
            b'S' => {
                let control1 = reflect(cubic, current, matches!(previous, Some(b'C' | b'c' | b'S' | b's')));
                let control2 = offset(reader.point()?);
                let to = offset(reader.point()?);
                path.commands.push(PathCommand::CubicTo { control1, control2, to });
                current = to;
                cubic = Some(control2);
                quadratic = None;
            }
            b'Q' => {
                let control = offset(reader.point()?);
                let to = offset(reader.point()?);
                path.commands.push(PathCommand::QuadraticTo { control, to });
                current = to;
                quadratic = Some(control);
                cubic = None;
            }
            b'T' => {
                let control = reflect(quadratic, current, matches!(previous, Some(b'Q' | b'q' | b'T' | b't')));
                let to = offset(reader.point()?);
                path.commands.push(PathCommand::QuadraticTo { control, to });
                current = to;
                quadratic = Some(control);
                cubic = None;
            }
            b'A' => return None,
            _ => return None,
        }
        previous = Some(cmd);
        first = false;
    }
    if path.commands.is_empty() {
        return None;
    }
    Some(path)
}

/// Reflection of a previous control point across the current point.
fn reflect(control: Option<Point>, current: Point, repeat: bool) -> Point {
    match control {
        Some(control) if repeat => Point::new(2.0 * current.x - control.x, 2.0 * current.y - control.y),
        _ => current,
    }
}

/// Parse an SVG `points` list into coordinate pairs.
fn parse_points(value: &str) -> Option<Vec<Point>> {
    let mut reader = PathReader { bytes: value.as_bytes(), at: 0 };
    let mut points = Vec::new();
    loop {
        reader.skip_separators();
        if reader.eof() {
            break;
        }
        let x = reader.number()?;
        let y = reader.number()?;
        points.push(Point::new(x, y));
    }
    Some(points)
}

/// Apply a base style plus one element's presentation attributes.
///
/// A `style` attribute is applied after the presentation attributes, so it
/// wins where both name the same property, as CSS does.
fn styled(base: &Style, attrs: &[(String, String)], opacity: &mut f64, hidden: &mut bool) -> Option<Style> {
    let mut style = base.clone();
    for (key, value) in attrs {
        if key == "style" {
            continue;
        }
        apply_property(&mut style, opacity, hidden, key, value)?;
    }
    if let Some(css) = attr(attrs, "style") {
        for declaration in css.split(';') {
            let Some((key, value)) = declaration.split_once(':')
            else {
                continue;
            };
            apply_property(&mut style, opacity, hidden, key.trim(), value.trim())?;
        }
    }
    Some(style)
}

/// Apply one supported CSS property.
fn apply_property(style: &mut Style, opacity: &mut f64, hidden: &mut bool, key: &str, value: &str) -> Option<()> {
    if value.trim().eq_ignore_ascii_case("inherit") {
        return Some(());
    }
    let key = key.trim().to_ascii_lowercase();
    match key.as_str() {
        "fill" => style.fill = parse_paint(value, style.current)?,
        "stroke" => style.stroke = parse_paint(value, style.current)?,
        "stroke-width" => style.stroke_width = parse_length(value)?,
        "fill-opacity" => style.fill_opacity = parse_number(value)?.clamp(0.0, 1.0),
        "opacity" => *opacity = parse_number(value)?.clamp(0.0, 1.0),
        "color" => style.current = parse_color(value)?,
        "display" => {
            if value.trim().eq_ignore_ascii_case("none") {
                *hidden = true;
            }
        }
        // Line caps, joins, dashes, and other paint details have no scene
        // field, so they are intentionally ignored.
        _ => {}
    }
    Some(())
}

/// Parse a paint value: `none`, `currentColor`, a hex colour, or a name.
fn parse_paint(value: &str, current: Color) -> Option<Option<Color>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("transparent") {
        return Some(None);
    }
    if value.eq_ignore_ascii_case("currentcolor") {
        return Some(Some(current));
    }
    Some(Some(parse_color(value)?))
}

/// Parse a hex colour or one of [`NAMED_COLORS`].
fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let channel = |text: &str| u8::from_str_radix(text, 16).ok();
        return match hex.len() {
            3 => {
                let red = channel(&hex[0..1])?;
                let green = channel(&hex[1..2])?;
                let blue = channel(&hex[2..3])?;
                Some(Color::rgb(red * 17, green * 17, blue * 17))
            }
            6 => {
                let red = channel(&hex[0..2])?;
                let green = channel(&hex[2..4])?;
                let blue = channel(&hex[4..6])?;
                Some(Color::rgb(red, green, blue))
            }
            _ => None,
        };
    }
    let lower = value.to_ascii_lowercase();
    NAMED_COLORS.iter().find(|(name, _)| *name == lower).map(|(_, color)| *color)
}

/// Read one geometry length, defaulting to zero when the attribute is absent.
fn length(attrs: &[(String, String)], key: &str) -> Option<f64> {
    match attr(attrs, key) {
        Some(value) => parse_length(value),
        None => Some(0.0),
    }
}

/// Parse a length with an optional `px` unit.
fn parse_length(value: &str) -> Option<f64> {
    let value = value.trim();
    let value = value.strip_suffix("px").unwrap_or(value);
    parse_number(value)
}

/// Parse one complete finite number.
fn parse_number(value: &str) -> Option<f64> {
    let mut reader = PathReader { bytes: value.trim().as_bytes(), at: 0 };
    let number = reader.number()?;
    reader.skip_separators();
    reader.eof().then_some(number)
}

/// Parse a whitespace- or comma-separated number list.
fn parse_numbers(value: &str) -> Option<Vec<f64>> {
    let mut reader = PathReader { bytes: value.as_bytes(), at: 0 };
    let mut numbers = Vec::new();
    loop {
        reader.skip_separators();
        if reader.eof() {
            break;
        }
        numbers.push(reader.number()?);
    }
    Some(numbers)
}

/// Read an attribute by its normalized name.
fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
}

/// Tokenizer for the XML-ish document subset.
#[derive(Debug)]
struct Xml<'a> {
    /// Whole document.
    text: &'a str,
    /// Byte offset of the next scan.
    at: usize,
}

impl Xml<'_> {
    /// Next tag token, `None` inside the outer option at end of input, or the
    /// outer `None` when the document is malformed.
    fn next_token(&mut self) -> Option<Option<Token>> {
        while let Some(offset) = self.text[self.at..].find('<') {
            self.at += offset;
            let rest = &self.text[self.at..];
            let length = if let Some(end) = rest.strip_prefix("<!--").and_then(|tail| tail.find("-->")) {
                end + 7
            }
            else if let Some(end) = rest.strip_prefix("<?").and_then(|tail| tail.find("?>")) {
                end + 4
            }
            else if let Some(end) = rest.strip_prefix("<![CDATA[").and_then(|tail| tail.find("]]>")) {
                end + 12
            }
            else if rest.starts_with("<!") {
                rest.find('>')? + 1
            }
            else {
                return self.tag().map(Some);
            };
            self.at += length;
        }
        self.at = self.text.len();
        Some(None)
    }

    /// Parse one start or end tag beginning at the current `<`.
    fn tag(&mut self) -> Option<Token> {
        let rest = &self.text[self.at..];
        let end = rest.find('>')?;
        let body = rest[1..end].trim();
        self.at += end + 1;
        if let Some(name) = body.strip_prefix('/') {
            return Some(Token::End { name: normalize(name.trim()) });
        }
        let self_closing = body.ends_with('/');
        let body = body.strip_suffix('/').unwrap_or(body).trim();
        let (name, attrs) = parse_tag(body)?;
        Some(Token::Start { name, attrs, self_closing })
    }
}

/// One XML token the parser consumes.
#[derive(Debug)]
enum Token {
    /// An opening or self-closing element.
    Start {
        /// Normalized element name.
        name: String,
        /// Normalized attribute names and decoded values.
        attrs: Vec<(String, String)>,
        /// Whether the tag closed itself.
        self_closing: bool,
    },
    /// A closing element.
    End {
        /// Normalized element name.
        name: String,
    },
}

/// Split a tag body into its name and attributes.
fn parse_tag(body: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut at = 0;
    let bytes = body.as_bytes();
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    let start = at;
    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    if at == start {
        return None;
    }
    let name = normalize(&body[start..at]);
    let mut attrs = Vec::new();
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() {
            break;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'=' {
            at += 1;
        }
        let key = normalize(&body[start..at]);
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() {
            attrs.push((key, String::new()));
            break;
        }
        if bytes[at] != b'=' {
            attrs.push((key, String::new()));
            continue;
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let quote = *bytes.get(at)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        at += 1;
        let value_start = at;
        let end = body[at..].find(quote as char)? + at;
        attrs.push((key, decode_entities(&body[value_start..end])));
        at = end + 1;
    }
    Some((name, attrs))
}

/// Strip a namespace prefix and lower-case an element or attribute name.
fn normalize(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).trim().to_ascii_lowercase()
}

/// Decode the predefined and numeric entities an attribute value may carry.
fn decode_entities(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(semicolon) = tail.find(';')
        else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semicolon];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity.strip_prefix('#').and_then(|number| {
                if let Some(hex) = number.strip_prefix(['x', 'X']) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                }
                else {
                    number.parse::<u32>().ok().and_then(char::from_u32)
                }
            }),
        };
        match decoded {
            Some(ch) => out.push(ch),
            None => out.push_str(&tail[..=semicolon]),
        }
        rest = &tail[semicolon + 1..];
    }
    out.push_str(rest);
    out
}

/// Sequential reader for path data and number lists.
#[derive(Debug)]
struct PathReader<'a> {
    /// Trimmed input bytes.
    bytes: &'a [u8],
    /// Byte offset of the next scan.
    at: usize,
}

impl PathReader<'_> {
    /// Skip whitespace and comma separators.
    fn skip_separators(&mut self) {
        while let Some(&byte) = self.bytes.get(self.at) {
            if byte.is_ascii_whitespace() || byte == b',' {
                self.at += 1;
            }
            else {
                break;
            }
        }
    }

    /// Whether every byte was consumed.
    fn eof(&self) -> bool {
        self.at >= self.bytes.len()
    }

    /// The command letter at the cursor, when the cursor is not a number.
    fn peek_command(&self) -> Option<u8> {
        self.bytes.get(self.at).copied().filter(|byte| byte.is_ascii_alphabetic())
    }

    /// Read one coordinate pair.
    fn point(&mut self) -> Option<Point> {
        let x = self.number()?;
        let y = self.number()?;
        Some(Point::new(x, y))
    }

    /// Read one finite number, leaving the cursor on the next separator.
    fn number(&mut self) -> Option<f64> {
        self.skip_separators();
        let start = self.at;
        if matches!(self.bytes.get(self.at).copied(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let mut digits = false;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
            digits = true;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
                digits = true;
            }
        }
        if !digits {
            self.at = start;
            return None;
        }
        if matches!(self.bytes.get(self.at).copied(), Some(b'e' | b'E')) {
            let exponent = self.at;
            self.at += 1;
            if matches!(self.bytes.get(self.at).copied(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let mut exponent_digits = false;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
                exponent_digits = true;
            }
            if !exponent_digits {
                self.at = exponent;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at]).ok()?.parse().ok()
    }
}
