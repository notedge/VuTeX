//! Lower a laid-out math tree into absolutely positioned layout primitives.

use super::LayoutNode;
use crate::plan::{LayoutItem, Rule, TextRun};
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
/// Path nodes have no representation in the layout plan, so they are skipped:
/// the builtin metrics expose no glyph outlines and the radical hook is a
/// placeholder path, and faking them as text or a box would be worse than
/// omitting them.
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
            if let Some(ch) = glyph.char {
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
        // Paths, kerns, glue, and penalties paint nothing here; the parent boxes
        // already advanced past their extents.
        LayoutNode::Path(_) | LayoutNode::Kern(_) | LayoutNode::Glue(_) | LayoutNode::Penalty { .. } => {}
    }
}

/// Font family written into text primitives.
///
/// The metrics system labels glyphs from the default math font as `default`,
/// which is not a usable CSS family, so those runs are painted with `serif`.
fn output_family(family: &str) -> String {
    if family == "default" { "serif".to_string() } else { family.to_string() }
}
