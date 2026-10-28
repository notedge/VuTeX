//! Integration tests for the math parse/layout pipeline.

use vutex_layout::math::{FormulaLayout, Glyph, LayoutNode, layout_formula, layout_formula_with_diagnostics};
use vutex_text::math::builtin_font_metrics;
use vutex_types::{
    Severity,
    math::{Fixed, FontStyle, GlyphKey, MathConstant},
};

fn layout(source: &str) -> FormulaLayout {
    let (_, metrics) = layout_formula(source, false).expect("formula should lay out");
    metrics
}

/// Walk the layout tree and report every glyph with its absolute position
/// relative to the tree origin.
///
/// Mirrors `render_layout_node`: an HBox advances its children horizontally and
/// applies its `shift` on the vertical axis, a VBox advances vertically and
/// applies its `shift` on the horizontal axis.
fn glyphs(node: &LayoutNode) -> Vec<(f64, f64, Glyph)> {
    fn walk(node: &LayoutNode, x: f64, y: f64, out: &mut Vec<(f64, f64, Glyph)>) {
        match node {
            LayoutNode::HBox(hbox) => {
                let mut cursor = 0.0;
                for child in &hbox.children {
                    walk(child, x + cursor, y + hbox.shift.to_f64(), out);
                    cursor += child.width().to_f64();
                }
            }
            LayoutNode::VBox(vbox) => {
                let mut cursor = -vbox.height.to_f64();
                for child in &vbox.children {
                    walk(child, x + vbox.shift.to_f64(), y + cursor + child.height().to_f64(), out);
                    cursor += child.height().to_f64() + child.depth().to_f64();
                }
            }
            LayoutNode::Glyph(glyph) => out.push((x, y, glyph.clone())),
            LayoutNode::Rule { .. } | LayoutNode::Glue(_) | LayoutNode::Kern(_) | LayoutNode::Path(_) | LayoutNode::Penalty { .. } => {}
        }
    }

    let mut out = Vec::new();
    walk(node, 0.0, 0.0, &mut out);
    out
}

/// All glyphs carrying `char`, in tree order.
fn glyphs_of(node: &LayoutNode, char: char) -> Vec<(f64, f64, Glyph)> {
    glyphs(node).into_iter().filter(|(_, _, glyph)| glyph.char == Some(char)).collect()
}

/// Position of the only glyph carrying `char`.
fn glyph_at(node: &LayoutNode, char: char) -> (f64, f64, Glyph) {
    let mut matches = glyphs_of(node, char);
    assert_eq!(matches.len(), 1, "`{char}` must appear exactly once in {node:?}");
    matches.remove(0)
}

/// Offset and glyph of the first horizontal glyph inside `node`.
fn first_glyph(node: &LayoutNode) -> (Fixed, &Glyph) {
    match node {
        LayoutNode::Glyph(glyph) => (Fixed::ZERO, glyph),
        LayoutNode::HBox(hbox) => {
            let mut offset = Fixed::ZERO;
            for child in &hbox.children {
                if matches!(child, LayoutNode::Glyph(_)) {
                    let (inner, glyph) = first_glyph(child);
                    return (offset + inner, glyph);
                }
                offset = offset + child.width();
            }
            panic!("no glyph inside {node:?}");
        }
        other => panic!("expected glyph material, got {other:?}"),
    }
}

#[test]
fn simple_formula_has_nonzero_box() {
    let metrics = layout("x");
    assert!(metrics.width > 0.0, "width should be positive: {metrics:?}");
    assert!(metrics.height > 0.0, "height should be positive: {metrics:?}");
    assert!(metrics.baseline > 0.0, "baseline should be positive: {metrics:?}");
}

#[test]
fn multi_atom_formula_is_wider_than_single_atom() {
    let single = layout("x");
    let wider = layout("x + y");
    assert!(wider.width > single.width, "expected {} > {}", wider.width, single.width);
}

#[test]
fn nested_fraction_grows_monotonically() {
    let inner = layout("\\frac{a}{b}");
    let outer = layout("\\frac{\\frac{a}{b}}{c}");

    assert!(inner.width > 0.0, "inner fraction width: {inner:?}");
    assert!(outer.width > inner.width, "nested fraction should be wider: {} vs {}", outer.width, inner.width);
    assert!(outer.height >= inner.height, "nested fraction should not shrink in height: {} vs {}", outer.height, inner.height);
}

#[test]
fn display_style_is_larger_than_text_style() {
    let (_, inline) = layout_formula("\\frac{a}{b}", false).expect("inline layout");
    let (_, display) = layout_formula("\\frac{a}{b}", true).expect("display layout");
    assert!(display.width > inline.width, "display style should use a larger base size: {} vs {}", display.width, inline.width);
}

#[test]
fn missing_fraction_argument_is_a_parse_error() {
    let error = layout_formula("\\frac{a}", false).expect_err("missing denominator must fail");
    let message = error.to_string();
    assert!(message.contains("Parse Error"), "unexpected diagnostic: {message}");
    assert!(message.contains("missing argument"), "unexpected diagnostic: {message}");
}

#[test]
fn unbalanced_group_is_a_parse_error() {
    let error = layout_formula("{a + b", false).expect_err("unclosed group must fail");
    assert!(error.to_string().contains("Parse Error"));
}

#[test]
fn unknown_command_is_a_parse_error() {
    let error = layout_formula("\\notacommand{x}", false).expect_err("unknown command must fail");
    let message = error.to_string();
    assert!(message.contains("unknown command"), "unexpected diagnostic: {message}");
}

#[test]
fn stray_script_is_a_parse_error() {
    let error = layout_formula("^2", false).expect_err("script without base must fail");
    assert!(error.to_string().contains("without a base"));
}

#[test]
fn delimited_radical_and_superscript_lay_out() {
    let metrics = layout("\\left(\\sqrt[3]{x^2}\\right)");
    assert!(metrics.width > 0.0);
    assert!(metrics.height > 0.0);
}

/// A delimiter stretched to cover its body must still advance horizontally by
/// the advance of the glyph it paints, and nothing else.
///
/// TeX gives `\left(` no space beyond the glyph: the box is the body plus the
/// two delimiter advances. The variant the layout painted is identified by the
/// height it covers, and its expected advance is read back from the same MATH
/// construction in em, so a variant that reports the height it spans as its
/// width (`advance_measurement`) fails both this identity and the em bound.
#[test]
fn delimited_box_is_body_plus_the_delimiter_advances() {
    let system = builtin_font_metrics();
    for display in [false, true] {
        let (tree, delimited) = layout_formula("\\left(\\frac{a}{b}\\right)", display).expect("delimited layout");
        let (_, body) = layout_formula("\\frac{a}{b}", display).expect("body layout");

        let (left_x, _, left) = glyph_at(&tree, '(');
        let (right_x, _, right) = glyph_at(&tree, ')');
        let left_adv = left.metrics.width.to_f64();
        let right_adv = right.metrics.width.to_f64();
        let size = left.size.to_f64();

        assert_eq!(left_x, 0.0, "the opening delimiter starts the box");
        assert!(left_adv <= size, "a delimiter variant may not advance a whole em horizontally: {} at size {size}", left_adv);
        assert!(
            left_adv <= (left.metrics.height + left.metrics.depth).to_f64() / 2.0,
            "a vertical variant must stay narrower than half the height it spans: {} vs {}",
            left_adv,
            (left.metrics.height + left.metrics.depth).to_f64()
        );

        // The painted delimiter is whichever prepared variant covers the
        // headroom the layout asked for; its own advance is the expected one.
        let key = GlyphKey::from_char('(', Some("default".to_string()), FontStyle::Math);
        let construction = system.get_delimiter_construction(&key).expect("`(` construction");
        let painted = construction
            .variants
            .iter()
            .map(|variant| system.get_metrics(&variant.glyph).expect("variant metrics"))
            .find(|metrics| (metrics.height.to_f64() * size - left.metrics.height.to_f64()).abs() < 1e-3)
            .expect("the painted delimiter must be one of the font's prepared variants");
        assert!(
            (left_adv - painted.width.to_f64() * size).abs() < 1e-3,
            "the delimiter must advance by the variant's width {} em, not {left_adv}",
            painted.width.to_f64()
        );
        assert!((right_adv - left_adv).abs() < 1e-3, "`(` and `)` must advance by the same width: {left_adv} vs {right_adv}");

        let expected = left_adv + body.width + right_adv;
        assert!(
            (delimited.width - expected).abs() < 1e-3,
            "the box must be the body plus the delimiter advances: {} vs {expected}",
            delimited.width
        );
        assert!(
            (right_x + right_adv - expected).abs() < 1e-3,
            "the closing delimiter must end the box with no trailing space: {} vs {expected}",
            right_x + right_adv
        );
        assert!(
            (right_x - (left_adv + body.width)).abs() < 1e-3,
            "the body must start right after the opening delimiter: {} vs {}",
            right_x,
            left_adv + body.width
        );
    }
}

/// The radical index is set at scriptscript size and placed with the MATH
/// table's radical constants instead of fixed fractions of an em.
///
/// Every expected offset is derived from the embedded font: the index size
/// from `ScriptScriptPercentScaleDown`, the horizontal position from
/// `RadicalKernBeforeDegree` / `RadicalKernAfterDegree`, and the bottom raise
/// from `RadicalDegreeBottomRaisePercent` applied to the surd's ascent over the
/// radicand. That ascent comes back from `RadicalVerticalGap` plus
/// `RadicalRuleThickness` over the radicand's own height.
#[test]
fn radical_index_uses_scriptscript_size_and_the_math_constants() {
    let system = builtin_font_metrics();
    let constant = |name: MathConstant| system.get_math_constant("default", name).expect("font constant").to_f64();

    let scriptscript = constant(MathConstant::ScriptScriptPercentScaleDown);
    let kern_before = constant(MathConstant::RadicalKernBeforeDegree);
    let kern_after = constant(MathConstant::RadicalKernAfterDegree);
    let raise_percent = constant(MathConstant::RadicalDegreeBottomRaisePercent);
    let vertical_gap = constant(MathConstant::RadicalVerticalGap);
    let display_gap = constant(MathConstant::RadicalDisplayStyleVerticalGap);
    let rule_thickness = constant(MathConstant::RadicalRuleThickness);

    for display in [false, true] {
        let gap = if display { display_gap } else { vertical_gap };
        let (tree, _) = layout_formula("\\sqrt[3]{x}", display).expect("radical should lay out");
        let (index_x, index_y, index) = glyph_at(&tree, '3');
        let (surd_x, _, surd) = glyph_at(&tree, '√');
        let (_, base_y, base) = glyph_at(&tree, 'x');
        let em = surd.size.to_f64();

        let expected_index_size = scriptscript * em;
        assert!(
            (index.size.to_f64() - expected_index_size).abs() < 1e-3,
            "the index must be set at scriptscript size {expected_index_size}, not {}",
            index.size.to_f64()
        );

        let before = kern_before * em;
        let after = kern_after * em;
        assert!((index_x - before).abs() < 1e-3, "the index must start after the before-kern {before}: {index_x}");
        let expected_surd_x = before + index.metrics.width.to_f64() + after;
        assert!(
            (surd_x - expected_surd_x).abs() < 1e-3,
            "the surd must start after the negative after-kern, at {expected_surd_x}: {surd_x}"
        );
        assert!(surd_x >= -0.05 * em, "the after-kern must not push the surd off the box origin: {surd_x}");

        let radicand_top = base.metrics.height.to_f64() + (gap + rule_thickness) * em;
        let raise = raise_percent * (radicand_top - base.metrics.depth.to_f64());
        let index_bottom = index_y + index.metrics.depth.to_f64();
        assert!(
            (index_bottom + raise).abs() < 1e-3,
            "the index bottom must sit {raise} above the baseline, not {}",
            -index_bottom
        );
        assert!(base_y.abs() < 1e-6, "the radicand keeps its own baseline as the formula baseline");
    }
}

/// Stacked limits use the MATH table's gap minima as clearances, not as
/// additions on top of one another.
///
/// TeX keeps `LowerLimitGapMin` of clearance under the operator, so the
/// subscript baseline lands at `max(depth + LowerLimitGapMin + height(sub),
/// LowerLimitBaselineDropMin)` below the operator baseline. The previous
/// `depth + max(gap, drop - depth) + height(sub)` added the limit's height to
/// the baseline drop a second time and pushed the subscript 2.9 px too low at
/// the display size.
#[test]
fn stacked_limits_keep_the_math_table_gap_minima() {
    let system = builtin_font_metrics();
    let constant = |name: MathConstant| system.get_math_constant("default", name).expect("font constant").to_f64();
    let (tree, _) = layout_formula("\\sum_{i=1}^{n}", true).expect("display limits");

    let (_, op_y, op) = glyph_at(&tree, '∑');
    let em = op.size.to_f64();
    let base_height = op.metrics.height.to_f64();
    let base_depth = op.metrics.depth.to_f64();

    // Both limits demote once, from display to script, so they are never set at
    // the operator's own size.
    let script = em * constant(MathConstant::ScriptPercentScaleDown);
    for c in ['n', 'i', '=', '1'] {
        let (_, _, glyph) = glyph_at(&tree, c);
        assert!(
            (glyph.size.to_f64() - script).abs() < 1e-3,
            "`{c}` must be set at the script size {script}, not {}",
            glyph.size.to_f64()
        );
    }

    // Upper limit: `UpperLimitGapMin` below the operator's top, or the baseline
    // rise minimum, whichever is larger.
    let (_, sup_y, sup) = glyph_at(&tree, 'n');
    let sup_bottom = sup_y + sup.metrics.depth.to_f64();
    let op_top = op_y - base_height;
    let gap_min_upper = constant(MathConstant::UpperLimitGapMin) * em;
    let rise_min = constant(MathConstant::UpperLimitBaselineRiseMin) * em;
    let expected_upper = gap_min_upper.max(rise_min - base_height - sup.metrics.depth.to_f64());
    assert!(
        (op_top - sup_bottom - expected_upper).abs() < 1e-3,
        "the upper limit must clear the operator by {expected_upper}, not {}",
        op_top - sup_bottom
    );

    // Lower limit: the gap rule and the baseline-drop rule are alternatives, so
    // the taller of them wins without being stacked on the other.
    let sub_height =
        ['i', '=', '1'].into_iter().map(|c| glyph_at(&tree, c).2.metrics.height.to_f64()).fold(f64::NEG_INFINITY, f64::max);
    let (_, sub_y, _) = glyph_at(&tree, 'i');
    let gap_min = constant(MathConstant::LowerLimitGapMin) * em;
    let drop_min = constant(MathConstant::LowerLimitBaselineDropMin) * em;
    let sub_drop = sub_y - op_y;
    let expected_drop = (base_depth + gap_min + sub_height).max(drop_min);
    assert!(
        (sub_drop - expected_drop).abs() < 1e-3,
        "the lower limit baseline must sit {expected_drop} below the operator, not {sub_drop}"
    );
    let clearance = sub_y - sub_height - (op_y + base_depth);
    assert!(
        clearance + 1e-3 >= gap_min,
        "the lower limit must keep `LowerLimitGapMin` {gap_min} below the operator, not {clearance}"
    );
    assert!(
        sub_drop < drop_min + sub_height,
        "the limit's own height must not be added on top of `LowerLimitBaselineDropMin`: {sub_drop} vs {}",
        drop_min + sub_height
    );

    // A script-style operator demotes its limits one style further, to
    // scriptscript, instead of leaving them at the operator's script size.
    // 14 px is the inline base em used by `layout_formula`.
    let (tree, _) = layout_formula("\\scriptstyle\\sum\\limits_{j}^{k}", false).expect("script limits");
    let (_, _, op) = glyph_at(&tree, '∑');
    let em = op.size.to_f64();
    assert!(
        (em - 14.0 * constant(MathConstant::ScriptPercentScaleDown)).abs() < 1e-3,
        "the operator itself is script-sized: {em}"
    );
    let scriptscript = 14.0 * constant(MathConstant::ScriptScriptPercentScaleDown);
    for c in ['j', 'k'] {
        let (_, _, glyph) = glyph_at(&tree, c);
        assert!(
            (glyph.size.to_f64() - scriptscript).abs() < 1e-3,
            "`{c}` under a script-style operator must be scriptscript {scriptscript}, not {}",
            glyph.size.to_f64()
        );
    }
}

/// Scripts beside an operator land on the `sup1`/`sup2` and `sub1`/`sub2`
/// minima measured from the operator's own baseline, so a subscript's height
/// never lifts the superscript.
///
/// A vertical box advances a child by its line height, and the previous kern
/// treated the sup-to-sub baseline distance as that line advance, adding the
/// subscript's height and the superscript's depth once more. At 16 px the
/// integral's `1` sat 22.07 px above the baseline instead of 14.61 px.
#[test]
fn beside_scripts_sit_on_the_shift_and_drop_minima() {
    let system = builtin_font_metrics();
    let constant = |name: MathConstant| system.get_math_constant("default", name).expect("font constant").to_f64();

    for (source, display, base_char, sup_char, sub_char) in
        [("\\int_0^1", true, '∫', '1', '0'), ("x_i^j", false, 'x', 'j', 'i')]
    {
        let (tree, _) = layout_formula(source, display).expect(source);
        let (base_x, base_y, base) = glyph_at(&tree, base_char);
        let (sup_x, sup_y, sup) = glyph_at(&tree, sup_char);
        let (sub_x, sub_y, sub) = glyph_at(&tree, sub_char);
        let em = base.size.to_f64();

        let script = em * constant(MathConstant::ScriptPercentScaleDown);
        for glyph in [&sup, &sub] {
            assert!(
                (glyph.size.to_f64() - script).abs() < 1e-3,
                "`{source}` scripts must be set at the script size {script}, not {}",
                glyph.size.to_f64()
            );
        }

        // `sup1` and `sup2`: the shift up minimum, or enough to keep the
        // superscript's bottom above the base's top by `SuperscriptBottomMin`.
        let sup_rise = base_y - sup_y;
        let expected_rise = (constant(MathConstant::SuperscriptShiftUp) * em)
            .max(base.metrics.height.to_f64() + constant(MathConstant::SuperscriptBottomMin) * em);
        assert!(
            (sup_rise - expected_rise).abs() < 1e-3,
            "`{source}` superscript must rise {expected_rise} above the baseline, not {sup_rise}"
        );

        // `sub1` and `sub2`: the shift down minimum, or enough to keep the
        // subscript's top below `SubscriptTopMax`.
        let sub_drop = sub_y - base_y;
        let shift_down = constant(MathConstant::SubscriptShiftDown) * em;
        let top_max = constant(MathConstant::SubscriptTopMax) * em;
        let expected_drop = shift_down.max(sub.metrics.height.to_f64() - top_max);
        assert!(
            (sub_drop - expected_drop).abs() < 1e-3,
            "`{source}` subscript must drop {expected_drop} below the baseline, not {sub_drop}"
        );

        // The scripts start after the operator's advance plus its italic
        // correction, which is the font's own request for clearance.
        let expected_x = base_x + base.metrics.width.to_f64() + base.metrics.italic_correction.to_f64();
        assert!((sup_x - expected_x).abs() < 1e-3, "`{source}` superscript starts at {sup_x}, not {expected_x}");
        assert!((sub_x - expected_x).abs() < 1e-3, "`{source}` subscript starts at {sub_x}, not {expected_x}");
    }
}

#[test]
fn matrix_lays_out_with_two_rows() {
    let metrics = layout("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
    assert!(metrics.width > 0.0);
    assert!(metrics.height >= metrics.depth, "matrix must stay centred on the math axis: {metrics:?}");
}

#[test]
fn spacing_adds_width_around_binary() {
    // TeX inserts thick muskip around binary operators like `+`.  A sequence
    // containing a binary atom must be wider than the trivial concatenation of
    // its operands — the gap is visible in the total HBox width.
    let plain = layout("xy");
    let spaced = layout("x+y");
    // x + y should be wider than x + y (operator glyph only)
    assert!(spaced.width > plain.width, "binary spacing must add width: {} vs {}", spaced.width, plain.width);

    // And it must also be wider than the two operands concatenated
    let two_atoms = layout("ab");
    assert!(
        spaced.width > two_atoms.width,
        "binary spacing must exceed plain two-atom width: {} vs {}",
        spaced.width,
        two_atoms.width
    );
}

#[test]
fn spacing_adds_width_around_relation() {
    // Relations (`=`, `<`, etc.) take thick muskip on both sides.
    let lhs = layout("x");
    let eq = layout("=");
    let rhs = layout("y");
    let spaced = layout("x=y");
    let plain_sum = lhs.width + eq.width + rhs.width;
    assert!(spaced.width > plain_sum, "relation spacing must add width: {} vs {}", spaced.width, plain_sum);
}

#[test]
fn spacing_adds_width_around_fraction_inner() {
    // Fractions are Inner atoms — thick muskip between them and neighbours.
    let a = layout("a");
    let b = layout("\\frac{x}{y}");
    let c = layout("c");
    let spaced = layout("a \\frac{x}{y} c");
    let plain_sum = a.width + b.width + c.width;
    assert!(spaced.width > plain_sum, "inner spacing must add width: {} vs {}", spaced.width, plain_sum);
}

#[test]
fn big_delimiter_parses() {
    let metrics = layout("\\big(x\\big)");
    assert!(metrics.width > 0.0, "big delimiter formula should have positive width");
}

#[test]
fn binom_lays_out() {
    let metrics = layout("\\binom{n}{k}");
    assert!(metrics.width > 0.0, "\\binom should have positive width");
    assert!(metrics.height > 0.0, "\\binom should have positive height");
}

#[test]
fn smallmatrix_supported() {
    let metrics = layout("\\begin{smallmatrix}1&2\\\\3&4\\end{smallmatrix}");
    assert!(metrics.width > 0.0, "smallmatrix should have positive width");
    assert!(metrics.height > 0.0, "smallmatrix should have positive height");
}

#[test]
fn bmatrix_supported() {
    let metrics = layout("\\begin{Bmatrix}a&b\\\\c&d\\end{Bmatrix}");
    assert!(metrics.width > 0.0, "Bmatrix should have positive width");
    assert!(metrics.height > 0.0, "Bmatrix should have positive height");
}

#[test]
fn hat_accent_centres_on_base_top_accent_attachment() {
    let (tree, _) = layout_formula("\\hat{f}", false).expect("\\hat{f} should lay out");
    let LayoutNode::VBox(vbox) = &tree else { panic!("an accent stacks an accent box over a base box: {tree:?}") };
    assert_eq!(vbox.children.len(), 2, "accent stack should hold exactly the accent and the base");

    let (accent_offset, accent) = first_glyph(&vbox.children[0]);
    let (base_offset, base_glyph) = first_glyph(&vbox.children[1]);

    let attachment = base_glyph.metrics.top_accent_attachment;
    assert!(attachment > Fixed::ZERO, "the embedded font publishes a top accent attachment for 'f'");

    let accent_centre = accent_offset + accent.metrics.width / Fixed::from_f64(2.0);
    let attachment_point = base_offset + attachment;
    let delta = (accent_centre - attachment_point).to_f64().abs();
    assert!(
        delta < 0.01,
        "accent ink centre {accent_centre:?} must sit on the base attachment {attachment_point:?} (delta {delta})"
    );
}

/// Ink bounds `(min_x, max_x, min_y, max_y)` of an em-normalised outline.
///
/// Outlines come from the font with y growing upward from the baseline, and
/// every operator in them starts on an x coordinate.
fn ink_bounds(d: &str) -> (f64, f64, f64, f64) {
    let mut bounds = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let mut is_x = true;
    let mut token = String::new();
    for c in d.chars().chain([' ']) {
        if c.is_ascii_alphabetic() {
            token.clear();
            is_x = true;
        }
        else if c == ' ' || c == ',' {
            if let Ok(value) = token.parse::<f64>() {
                if is_x {
                    bounds.0 = bounds.0.min(value);
                    bounds.1 = bounds.1.max(value);
                }
                else {
                    bounds.2 = bounds.2.min(value);
                    bounds.3 = bounds.3.max(value);
                }
                is_x = !is_x;
            }
            token.clear();
        }
        else {
            token.push(c);
        }
    }
    bounds
}

/// Every rule in the tree as an absolute `(x, top, width, height)` rectangle.
///
/// Mirrors the same walk as [`glyphs`] and the ``Rule`` rectangle
/// `render_layout_node` paints from the node's baseline.
fn rules(node: &LayoutNode) -> Vec<(f64, f64, f64, f64)> {
    fn walk(node: &LayoutNode, x: f64, y: f64, out: &mut Vec<(f64, f64, f64, f64)>) {
        match node {
            LayoutNode::HBox(hbox) => {
                let mut cursor = 0.0;
                for child in &hbox.children {
                    walk(child, x + cursor, y + hbox.shift.to_f64(), out);
                    cursor += child.width().to_f64();
                }
            }
            LayoutNode::VBox(vbox) => {
                let mut cursor = -vbox.height.to_f64();
                for child in &vbox.children {
                    walk(child, x + vbox.shift.to_f64(), y + cursor + child.height().to_f64(), out);
                    cursor += child.height().to_f64() + child.depth().to_f64();
                }
            }
            LayoutNode::Rule { width, height, depth } => {
                let thickness = height.to_f64();
                out.push((x, y - thickness, width.to_f64(), thickness + depth.to_f64()));
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    walk(node, 0.0, 0.0, &mut out);
    out
}

#[test]
fn hat_accent_paints_font_ink_on_the_base_attachment() {
    let (tree, _) = layout_formula("\\hat{x}", false).expect("\\hat{x} should lay out");
    let LayoutNode::VBox(vbox) = &tree else { panic!("an accent stacks an accent box over a base box: {tree:?}") };
    assert_eq!(vbox.children.len(), 2, "accent stack should hold exactly the accent and the base");

    let (accent_x, accent_y, accent) = glyph_at(&tree, 'ˆ');
    let path = accent.path_data.as_deref().expect("`\\hat` must paint the font's circumflex, not a host-font run");
    let (min_x, max_x, min_y, _max_y) = ink_bounds(path);
    let size = accent.size.to_f64();

    let ink_width = (max_x - min_x) * size;
    assert!(ink_width > 0.0, "the circumflex outline {path} paints no ink ({ink_width})");

    let (base_x, base_y, base) = glyph_at(&tree, 'x');
    let ink_centre = accent_x + (min_x + max_x) / 2.0 * size;
    let attachment = base_x + base.metrics.top_accent_attachment.to_f64();
    assert!(
        (ink_centre - attachment).abs() < 0.01,
        "circumflex ink centre {ink_centre} must sit on the base attachment {attachment}"
    );

    // Canvas coordinates grow downward, so "higher" means a smaller y.
    let ink_bottom = accent_y - min_y * size;
    let base_top = base_y - base.metrics.height.to_f64();
    assert!(ink_bottom < base_top, "circumflex ink bottom {ink_bottom} must stay above the base top {base_top}");
}

#[test]
fn bar_accent_is_a_rule_covering_the_base() {
    let (tree, _) = layout_formula("\\bar{x}", false).expect("\\bar{x} should lay out");
    let bars = rules(&tree);
    assert_eq!(bars.len(), 1, "`\\bar` must paint exactly one overbar rule: {tree:?}");
    let (bar_x, bar_top, bar_width, bar_height) = bars[0];
    assert!(bar_width > 0.0 && bar_height > 0.0, "overbar needs a positive size: {bars:?}");

    let (base_x, base_y, base) = glyph_at(&tree, 'x');
    let size = base.size.to_f64();
    let path = base.path_data.as_deref().expect("the nucleus must paint its own outline");
    let (min_x, max_x, _, _) = ink_bounds(path);
    let base_left = base_x + min_x * size;
    let base_right = base_x + max_x * size;
    assert!(
        bar_x <= base_left + 1e-3 && bar_x + bar_width >= base_right - 1e-3,
        "overbar {bar_x}..{} must cover the base ink {base_left}..{base_right}",
        bar_x + bar_width
    );

    let base_top = base_y - base.metrics.height.to_f64();
    assert!(
        bar_top + bar_height <= base_top + 1e-3,
        "overbar bottom {} must clear the base top {base_top}",
        bar_top + bar_height
    );
}

/// Glyph accents clear the base ink by `FractionRuleThickness`, the MATH
/// table's `\defaultrulethickness`, and not by the much larger overbar gap.
///
/// `OverbarVerticalGap` is 0.12 em and belongs to `\bar`, so `\hat` and
/// `\tilde` floated 1.68 px above the letter at 14 px. `FractionRuleThickness`
/// is 0.039993 em, a 0.5599 px clearance.
#[test]
fn glyph_accents_clear_the_base_ink_by_the_rule_thickness() {
    let system = builtin_font_metrics();
    let thickness = system.get_math_constant("default", MathConstant::FractionRuleThickness).expect("font constant").to_f64();

    for (source, accent_char, base_char) in [("\\hat{f}", 'ˆ', 'f'), ("\\tilde{x}", '˜', 'x')] {
        let (tree, _) = layout_formula(source, false).expect(source);
        let (_, accent_y, accent) = glyph_at(&tree, accent_char);
        let (_, base_y, base) = glyph_at(&tree, base_char);

        let accent_ink = accent.path_data.as_deref().expect("the accent must paint the font outline");
        let base_ink = base.path_data.as_deref().expect("the base must paint its own outline");
        let (_, _, accent_min_y, _) = ink_bounds(accent_ink);
        let (_, _, _, base_max_y) = ink_bounds(base_ink);

        let ink_bottom = accent_y - accent_min_y * accent.size.to_f64();
        let ink_top = base_y - base_max_y * base.size.to_f64();
        let clearance = ink_top - ink_bottom;
        let expected = thickness * accent.size.to_f64();
        assert!(
            (clearance - expected).abs() < 1e-3,
            "`{source}` accent ink must clear the base ink by {expected}, not {clearance}"
        );
    }

    // `\bar` is a rule, not a glyph, and keeps the overbar gap.
    let (tree, _) = layout_formula("\\bar{x}", false).expect("\\bar{x} should lay out");
    let (_, base_y, base) = glyph_at(&tree, 'x');
    let base_top = base_y - base.metrics.height.to_f64();
    let bars = rules(&tree);
    assert_eq!(bars.len(), 1, "`\\bar` must paint exactly one overbar rule");
    let (_, bar_top, _, bar_height) = bars[0];
    let gap = base_top - (bar_top + bar_height);
    let expected = system.get_math_constant("default", MathConstant::OverbarVerticalGap).expect("font constant").to_f64()
        * base.size.to_f64();
    assert!((gap - expected).abs() < 1e-3, "`\\bar` must keep the overbar gap {expected}, not {gap}");
}

/// Every glyph size inside a tree, in pixels.
fn glyph_sizes(node: &LayoutNode) -> Vec<f64> {
    glyphs(node).into_iter().map(|(_, _, glyph)| glyph.size.to_f64()).collect()
}

/// `\displaystyle` must really switch the enclosing style.  In TeX a display
/// fraction sets its arguments in text style instead of script style and uses
/// the display shift-up constants, so the same em size yields a little under
/// twice the vertical extent of the text-style fraction.
#[test]
fn display_selection_command_expands_fraction_geometry() {
    let text = layout("\\frac{a}{b}");
    let display = layout("\\displaystyle \\frac{a}{b}");

    let text_total = text.height + text.depth;
    let display_total = display.height + display.depth;
    assert!(
        display_total >= text_total * 1.5,
        "`\\displaystyle` fraction must be at least 1.5x as tall: {display_total} vs {text_total}"
    );
    assert!(
        display.height > text.height,
        "`\\displaystyle` raises the fraction above the baseline: {} vs {}",
        display.height,
        text.height
    );
    assert!(display.width > text.width, "display arguments are set a size larger: {} vs {}", display.width, text.width);
}

/// The display/text difference also holds when the whole formula is laid out
/// by the display pipeline (larger em), which is the document-facing path.
#[test]
fn display_mode_fraction_is_taller_than_text_mode() {
    let (_, text) = layout_formula("\\frac{a}{b}", false).expect("inline layout");
    let (_, display) = layout_formula("\\frac{a}{b}", true).expect("display layout");

    let text_total = text.height + text.depth;
    let display_total = display.height + display.depth;
    assert!(
        display_total >= text_total * 1.5,
        "display-mode fraction must be at least 1.5x as tall: {display_total} vs {text_total}"
    );
}

/// `\scriptstyle` / `\scriptscriptstyle` scale the following atoms instead of
/// being accepted and ignored.
#[test]
fn style_commands_scale_following_glyphs() {
    let base = layout("x").width;
    let script = layout("\\scriptstyle x").width;
    let scriptscript = layout("\\scriptscriptstyle x").width;

    let script_ratio = script / base;
    let scriptscript_ratio = scriptscript / base;
    assert!((script_ratio - 0.7).abs() < 0.02, "`\\scriptstyle` must be 0.7 em: ratio {script_ratio}");
    assert!((scriptscript_ratio - 0.5).abs() < 0.02, "`\\scriptscriptstyle` must be 0.5 em: ratio {scriptscript_ratio}");
}

/// Fraction arguments demote one style further: display -> text, text -> script,
/// script/scriptscript -> scriptscript.  So the argument glyphs of a script-style
/// fraction are set at scriptscript size, never larger than their parent.
#[test]
fn fraction_arguments_demote_one_style_further() {
    // 14 px is the inline base em used by `layout_formula`.
    for (source, expected) in [
        ("\\frac{a}{b}", 14.0 * 0.7),
        ("\\textstyle \\frac{a}{b}", 14.0 * 0.7),
        ("\\displaystyle \\frac{a}{b}", 14.0),
        ("\\scriptstyle \\frac{a}{b}", 14.0 * 0.5),
        ("\\scriptscriptstyle \\frac{a}{b}", 14.0 * 0.5),
    ] {
        let (tree, _) = layout_formula(source, false).expect(source);
        let sizes = glyph_sizes(&tree);
        assert!(!sizes.is_empty(), "`{source}` must lay out glyphs");
        for size in sizes {
            assert!(
                (size - expected).abs() < 0.01,
                "`{source}` arguments must be set at {expected} px, found {size}"
            );
        }
    }
}

/// `\limits` must stack the scripts above and below the operator, `\nolimits`
/// must keep them beside it.  Stacking trades width for height, so the two
/// layouts must differ in both dimensions.
#[test]
fn limits_override_stacks_scripts_over_the_operator() {
    let (beside_tree, beside) = layout_formula("\\sum\\nolimits_{i=1}^{n}", false).expect("scripts beside");
    let (stacked_tree, stacked) = layout_formula("\\sum\\limits_{i=1}^{n}", false).expect("stacked limits");

    assert!(
        stacked.width + 1.0 < beside.width,
        "stacked limits must be narrower than scripts beside the operator: {} vs {}",
        stacked.width,
        beside.width
    );
    let stacked_extent = stacked.height + stacked.depth;
    let beside_extent = beside.height + beside.depth;
    assert!(
        stacked_extent > beside_extent * 1.1,
        "stacked limits must reach further than scripts beside the operator: {stacked_extent} vs {beside_extent}"
    );
    assert!(
        stacked.depth > beside.depth * 2.0,
        "stacked limits push the subscript well below the baseline: {} vs {}",
        stacked.depth,
        beside.depth
    );

    // The stacked superscript sits over the operator, horizontally centred
    // inside its span.
    let (op_x, _, op) = glyph_at(&stacked_tree, '∑');
    let op_right = op_x + op.metrics.width.to_f64();
    let (sup_x, _, sup) = glyph_at(&stacked_tree, 'n');
    let sup_centre = sup_x + sup.metrics.width.to_f64() / 2.0;
    assert!(
        sup_centre > op_x && sup_centre < op_right,
        "stacked superscript centre {sup_centre} must fall inside the operator span {op_x}..{op_right}"
    );

    // Beside the operator, the subscript starts at or after its right edge.
    let (beside_op_x, _, beside_op) = glyph_at(&beside_tree, '∑');
    let beside_op_right = beside_op_x + beside_op.metrics.width.to_f64();
    let (beside_sub_x, _, _) = glyph_at(&beside_tree, 'i');
    assert!(
        beside_sub_x >= beside_op_right - 0.01,
        "scripts beside the operator must start after it: {beside_sub_x} vs {beside_op_right}"
    );
}

/// `\int\limits` stacks, `\int\nolimits` does not — the same override works for
/// every large operator, not only `\sum`.
#[test]
fn integral_limits_follow_the_override() {
    let beside = layout("\\int\\nolimits_{0}^{1}");
    let stacked = layout("\\int\\limits_{0}^{1}");

    assert!(
        stacked.width + 0.5 < beside.width,
        "stacked integral limits must be narrower: {} vs {}",
        stacked.width,
        beside.width
    );
    assert!(
        stacked.depth > beside.depth * 2.0,
        "stacked integral limits push the subscript below the baseline: {} vs {}",
        stacked.depth,
        beside.depth
    );
    assert!(
        stacked.height + stacked.depth > (beside.height + beside.depth) * 1.1,
        "stacked integral limits must reach further: {} vs {}",
        stacked.height + stacked.depth,
        beside.height + beside.depth
    );
}

/// TeX defaults: `\sum` stacks its limits in display style, `\int` never stacks
/// unless told to.  Each default must match the explicit command.
#[test]
fn operator_default_limits_follow_tex() {
    let (_, sum_default) = layout_formula("\\sum_{i=1}^{n}", true).expect("display sum");
    let (_, sum_limits) = layout_formula("\\sum\\limits_{i=1}^{n}", true).expect("display sum with limits");
    let (_, sum_beside) = layout_formula("\\sum\\nolimits_{i=1}^{n}", true).expect("display sum without limits");
    assert!(
        (sum_default.height - sum_limits.height).abs() < 0.01 && (sum_default.width - sum_limits.width).abs() < 0.01,
        "`\\sum` must default to limits in display: {sum_default:?} vs {sum_limits:?}"
    );
    assert!(
        sum_default.width + 1.0 < sum_beside.width,
        "`\\sum` must not default to scripts beside the operator: {} vs {}",
        sum_default.width,
        sum_beside.width
    );

    let (_, int_default) = layout_formula("\\int_{0}^{1}", true).expect("display integral");
    let (_, int_nolimits) = layout_formula("\\int\\nolimits_{0}^{1}", true).expect("display integral without limits");
    let (_, int_limits) = layout_formula("\\int\\limits_{0}^{1}", true).expect("display integral with limits");
    assert!(
        (int_default.height - int_nolimits.height).abs() < 0.01 && (int_default.width - int_nolimits.width).abs() < 0.01,
        "`\\int` must default to nolimits: {int_default:?} vs {int_nolimits:?}"
    );
    assert!(
        int_default.depth * 2.0 < int_limits.depth,
        "`\\int` must not default to stacked limits: depth {} vs {}",
        int_default.depth,
        int_limits.depth
    );
}

/// `cases` is a left-aligned two-column layout: the expression and the condition
/// each start at their column's left edge, so short cells are never centred.
#[test]
fn cases_left_aligns_its_columns() {
    let (tree, _) =
        layout_formula("\\begin{cases} q & a \\\\ xyz & b \\end{cases}", false).expect("cases should lay out");

    let (expression_x, _, _) = glyph_at(&tree, 'q');
    let (wide_expression_x, _, _) = glyph_at(&tree, 'x');
    assert!(
        (expression_x - wide_expression_x).abs() < 0.01,
        "the expression column must start at one x: {expression_x} vs {wide_expression_x}"
    );

    let (condition_x, _, _) = glyph_at(&tree, 'a');
    let (wide_condition_x, _, _) = glyph_at(&tree, 'b');
    assert!(
        (condition_x - wide_condition_x).abs() < 0.01,
        "the condition column must start at one x: {condition_x} vs {wide_condition_x}"
    );
}

/// `aligned` (and `alignedat`) alternate right/left at every `&`, so the column
/// before the relation is right-aligned and every relation lands on one x.
#[test]
fn aligned_alternates_right_and_left_columns() {
    for source in [
        "\\begin{aligned} q &= a \\\\ xyz &= b \\end{aligned}",
        "\\begin{alignedat}{2} q &= a \\\\ xyz &= b \\end{alignedat}",
    ] {
        let (tree, _) = layout_formula(source, false).expect(source);

        let (short_x, _, _) = glyph_at(&tree, 'q');
        let (wide_x, _, _) = glyph_at(&tree, 'x');
        let expected_shift = layout("xyz").width - layout("q").width;
        let shift = short_x - wide_x;
        assert!(
            (shift - expected_shift).abs() < 0.02,
            "`{source}` must right-align the first column: shift {shift} vs {expected_shift}"
        );

        let relations = glyphs_of(&tree, '=');
        assert_eq!(relations.len(), 2, "`{source}` has one relation per row");
        let gap = (relations[0].0 - relations[1].0).abs();
        assert!(gap < 0.01, "`{source}` must line its relations up: gap {gap}");
    }
}

/// `gathered` puts every row on the same horizontal centre instead of aligning
/// columns.
#[test]
fn gathered_centres_each_row() {
    let (tree, _) = layout_formula("\\begin{gathered} q \\\\ xyz \\end{gathered}", false).expect("gathered");

    let (narrow_x, _, _) = glyph_at(&tree, 'q');
    let (wide_x, _, _) = glyph_at(&tree, 'x');
    let expected_shift = (layout("xyz").width - layout("q").width) / 2.0;
    let shift = narrow_x - wide_x;
    assert!(
        (shift - expected_shift).abs() < 0.02,
        "the narrow row must be centred over the wide one: shift {shift} vs {expected_shift}"
    );
}

/// `split` keeps both rows in one alignment block: the relation glyphs share one
/// x even when the left-hand sides differ in width, so the equals signs stack.
#[test]
fn split_aligns_rows_at_the_relation() {
    let (tree, _) = layout_formula("\\begin{split} q &= a \\\\ xyz &= b \\end{split}", false).expect("split");

    let relations = glyphs_of(&tree, '=');
    assert_eq!(relations.len(), 2, "each row carries a relation");
    let gap = (relations[0].0 - relations[1].0).abs();
    assert!(gap < 0.01, "the split rows must align at the relation: gap {gap}");

    let (left_x, _, _) = glyph_at(&tree, 'q');
    let (wide_x, _, _) = glyph_at(&tree, 'x');
    let expected_shift = layout("xyz").width - layout("q").width;
    let shift = left_x - wide_x;
    assert!(
        (shift - expected_shift).abs() < 0.02,
        "the split left-hand column must be right-aligned: shift {shift} vs {expected_shift}"
    );
    assert!(
        relations[0].0 > left_x + layout("q").width - 0.01,
        "the relation column must follow the right-aligned left column: {} vs {}",
        relations[0].0,
        left_x + layout("q").width
    );
}

/// Plain `matrix` keeps its centred cells: the environment semantics above must
/// not leak into the generic matrix.
#[test]
fn plain_matrix_keeps_centred_cells() {
    let (tree, _) = layout_formula("\\begin{matrix} q & a \\\\ xyz & b \\end{matrix}", false).expect("matrix");

    let (short_x, _, _) = glyph_at(&tree, 'q');
    let (wide_x, _, _) = glyph_at(&tree, 'x');
    let expected_shift = (layout("xyz").width - layout("q").width) / 2.0;
    let shift = short_x - wide_x;
    assert!((shift - expected_shift).abs() < 0.02, "matrix cells stay centred: shift {shift} vs {expected_shift}");
}

#[test]
fn alignedat_fixes_the_grid_to_the_declared_column_pairs() {
    let source = "\\begin{alignedat}{2} q &= a & r &= b \\\\ xyz &= c & s &= d \\end{alignedat}";
    let (tree, metrics, diagnostics) = layout_formula_with_diagnostics(source, false, 14.0).expect("alignedat should lay out");
    assert!(diagnostics.is_empty(), "a body that fills the declared pairs must not warn: {diagnostics:?}");

    // Two pairs fix four `rl` columns, so each row carries two relations and
    // the same pair lands on one x across rows.
    let relations = glyphs_of(&tree, '=');
    assert_eq!(relations.len(), 4, "two pairs over two rows carry four relations");
    assert!(
        (relations[0].0 - relations[2].0).abs() < 0.01,
        "the first pair's relation must line up across rows: {} vs {}",
        relations[0].0,
        relations[2].0
    );
    assert!(
        (relations[1].0 - relations[3].0).abs() < 0.01,
        "the second pair's relation must line up across rows: {} vs {}",
        relations[1].0,
        relations[3].0
    );
    assert!(
        relations[1].0 > relations[0].0 + 1.0,
        "the second pair must sit to the right of the first: {} vs {}",
        relations[1].0,
        relations[0].0
    );

    // Column x coordinates: column 1 and column 3 are left-aligned, columns 0
    // and 2 are right-aligned, so their right edges are what coincide.
    assert!((relations[0].0 - 32.5354).abs() < 0.01, "column 1 starts at 32.5354: {}", relations[0].0);
    let (b_x, _, _) = glyph_at(&tree, 'b');
    let (d_x, _, _) = glyph_at(&tree, 'd');
    assert!((b_x - 97.1664).abs() < 0.01, "column 3 starts at 97.1664: {b_x}");
    assert!((b_x - d_x).abs() < 0.01, "column 3 must start at one x: {b_x} vs {d_x}");

    let (q_x, _, _) = glyph_at(&tree, 'q');
    let (x_x, _, _) = glyph_at(&tree, 'x');
    let expected_shift = layout("xyz").width - layout("q").width;
    assert!(
        ((q_x - x_x) - expected_shift).abs() < 0.02,
        "column 0 must be right-aligned: shift {} vs {expected_shift}",
        q_x - x_x
    );
    let (r_x, _, _) = glyph_at(&tree, 'r');
    let (s_x, _, _) = glyph_at(&tree, 's');
    // Right alignment shifts the narrower cell until both right edges meet, so
    // the offset is the width difference whichever of the two is wider.
    let widths = [layout("r").width, layout("s").width];
    let expected_shift = widths[0].max(widths[1]) - widths[0].min(widths[1]);
    assert!(
        ((r_x - s_x).abs() - expected_shift).abs() < 0.02,
        "column 2 must be right-aligned: shift {} vs {expected_shift}",
        r_x - s_x
    );

    assert!((metrics.width - 104.9504).abs() < 0.01, "four columns are 104.9504 px wide: {}", metrics.width);
}

/// A body that types fewer columns than `alignedat{n}` declares pads the missing
/// columns instead of silently shrinking the grid, and says so through a warning.
#[test]
fn alignedat_pads_missing_columns_and_reports_the_mismatch() {
    let padded = "\\begin{alignedat}{2} q &= a \\\\ xyz &= b \\end{alignedat}";
    let one_pair = "\\begin{alignedat}{1} q &= a \\\\ xyz &= b \\end{alignedat}";

    let (padded_tree, padded_metrics, diagnostics) =
        layout_formula_with_diagnostics(padded, false, 14.0).expect("alignedat should lay out");
    assert_eq!(diagnostics.len(), 1, "the column mismatch must be reported once: {diagnostics:?}");
    assert_eq!(diagnostics[0].code, "vutex.layout.math.alignedat_columns");
    assert_eq!(diagnostics[0].severity, Severity::Warning);

    let (one_pair_tree, one_pair_metrics, clean) =
        layout_formula_with_diagnostics(one_pair, false, 14.0).expect("alignedat should lay out");
    assert!(clean.is_empty(), "a body that fills its pair must not warn: {clean:?}");

    // Two empty trailing columns add their inter-column spacing and nothing
    // else: `0.8 em` at the 14 px base em.
    let extra = padded_metrics.width - one_pair_metrics.width;
    assert!((extra - 22.4).abs() < 0.02, "two padded columns add 22.4 px, found {extra}");

    // Padding must not move the cells the author typed.
    for glyph in ['q', '=', 'a'] {
        let padded_x: Vec<f64> = glyphs_of(&padded_tree, glyph).into_iter().map(|(x, _, _)| x).collect();
        let declared_x: Vec<f64> = glyphs_of(&one_pair_tree, glyph).into_iter().map(|(x, _, _)| x).collect();
        assert_eq!(padded_x.len(), declared_x.len(), "`{glyph}` must keep its occurrences");
        for (left, right) in padded_x.iter().zip(&declared_x) {
            assert!((left - right).abs() < 0.01, "`{glyph}` moved from {right} to {left} when columns were padded");
        }
    }
}

/// `align` is the display-level `rl` grid: each `&` separates a column and
/// every relation lands on one x, matching what `aligned` produces.
#[test]
fn align_lays_out_the_display_rl_grid() {
    let source = "\\begin{align} q &= a \\\\ xyz &= b \\end{align}";
    let (tree, metrics, diagnostics) = layout_formula_with_diagnostics(source, false, 14.0).expect("align should lay out");
    assert!(diagnostics.is_empty(), "a plain `align` body must not warn: {diagnostics:?}");

    let relations = glyphs_of(&tree, '=');
    assert_eq!(relations.len(), 2, "one relation per row");
    assert!((relations[0].0 - 32.5354).abs() < 0.01, "the relation column starts at 32.5354: {}", relations[0].0);
    assert!(
        (relations[0].0 - relations[1].0).abs() < 0.01,
        "the relations must line up across rows: {} vs {}",
        relations[0].0,
        relations[1].0
    );

    let (q_x, _, _) = glyph_at(&tree, 'q');
    let (x_x, _, _) = glyph_at(&tree, 'x');
    let expected_shift = layout("xyz").width - layout("q").width;
    assert!(
        ((q_x - x_x) - expected_shift).abs() < 0.02,
        "column 0 must be right-aligned: shift {} vs {expected_shift}",
        q_x - x_x
    );

    let (a_x, _, _) = glyph_at(&tree, 'a');
    let (b_x, _, _) = glyph_at(&tree, 'b');
    assert!((a_x - 47.3162).abs() < 0.01, "column 1 starts at 47.3162: {a_x}");
    assert!((a_x - b_x).abs() < 0.01, "column 1 must start at one x: {a_x} vs {b_x}");

    assert!((metrics.width - 55.1002).abs() < 0.01, "two `rl` columns are 55.1002 px wide: {}", metrics.width);
}

/// `align` and `align*` share one geometry with `aligned`: the star only drops
/// equation numbering, which belongs to the document layer.
#[test]
fn align_and_aligned_share_the_grid() {
    let sources = [
        "\\begin{align} q &= a \\\\ xyz &= b \\end{align}",
        "\\begin{align*} q &= a \\\\ xyz &= b \\end{align*}",
        "\\begin{aligned} q &= a \\\\ xyz &= b \\end{aligned}",
    ];
    let mut reference: Option<(f64, Vec<f64>)> = None;
    for source in sources {
        let (tree, metrics, diagnostics) = layout_formula_with_diagnostics(source, false, 14.0).expect(source);
        assert!(diagnostics.is_empty(), "`{source}` must not warn: {diagnostics:?}");
        let xs: Vec<f64> = glyphs(&tree).into_iter().map(|(x, _, _)| x).collect();
        match &reference {
            None => reference = Some((metrics.width, xs)),
            Some((width, xs_ref)) => {
                assert!((metrics.width - width).abs() < 0.01, "`{source}` width {} vs {width}", metrics.width);
                assert_eq!(xs.len(), xs_ref.len(), "`{source}` must hold the same glyphs");
                for (left, right) in xs.iter().zip(xs_ref) {
                    assert!((left - right).abs() < 0.01, "`{source}` puts a glyph at {left} instead of {right}");
                }
            }
        }
    }
}

/// `align` is a display-level environment, so an enclosing `\scriptstyle` must
/// not demote its cells. `aligned` follows the enclosing style instead.
#[test]
fn align_ignores_an_enclosing_script_style() {
    let (align_tree, align_metrics, _) =
        layout_formula_with_diagnostics("\\scriptstyle \\begin{align} q &= a \\end{align}", false, 14.0).expect("align");
    let (aligned_tree, aligned_metrics, _) =
        layout_formula_with_diagnostics("\\scriptstyle \\begin{aligned} q &= a \\end{aligned}", false, 14.0).expect("aligned");

    let (_, _, align_q) = glyph_at(&align_tree, 'q');
    assert!((align_q.size.to_f64() - 14.0).abs() < 0.01, "`align` keeps the 14 px display size: {}", align_q.size.to_f64());
    let (_, _, aligned_q) = glyph_at(&aligned_tree, 'q');
    assert!(
        (aligned_q.size.to_f64() - 9.8).abs() < 0.01,
        "`aligned` follows `\\scriptstyle` down to 9.8 px: {}",
        aligned_q.size.to_f64()
    );

    assert!((align_metrics.width - 40.9043).abs() < 0.01, "the undemoted `align` grid is 40.9043 px: {}", align_metrics.width);
    assert!(
        (aligned_metrics.width - 28.6329).abs() < 0.01,
        "the demoted `aligned` grid is 28.6329 px: {}",
        aligned_metrics.width
    );
}

/// The starred `alignedat*` shares the grid of `alignedat`: the star only drops
/// equation numbering, which belongs to the document layer.
#[test]
fn starred_alignedat_keeps_the_unstarred_geometry() {
    let plain = "\\begin{alignedat}{2} q &= a & r &= b \\\\ xyz &= c & s &= d \\end{alignedat}";
    let starred = "\\begin{alignedat*}{2} q &= a & r &= b \\\\ xyz &= c & s &= d \\end{alignedat*}";

    let (plain_tree, plain_metrics, plain_diagnostics) =
        layout_formula_with_diagnostics(plain, false, 14.0).expect("alignedat");
    let (starred_tree, starred_metrics, starred_diagnostics) =
        layout_formula_with_diagnostics(starred, false, 14.0).expect("alignedat*");
    assert!(plain_diagnostics.is_empty() && starred_diagnostics.is_empty(), "both bodies fill their pairs");

    assert!(
        (plain_metrics.width - starred_metrics.width).abs() < 0.01,
        "`alignedat*` must share the grid: {} vs {}",
        starred_metrics.width,
        plain_metrics.width
    );
    let plain_x: Vec<f64> = glyphs(&plain_tree).into_iter().map(|(x, _, _)| x).collect();
    let starred_x: Vec<f64> = glyphs(&starred_tree).into_iter().map(|(x, _, _)| x).collect();
    assert_eq!(plain_x.len(), starred_x.len(), "`alignedat*` must hold the same glyphs");
    for (left, right) in starred_x.iter().zip(&plain_x) {
        assert!((left - right).abs() < 0.01, "`alignedat*` moved a glyph to {left} from {right}");
    }
}
