//! Integration tests for the math parse/layout pipeline.

use vutex_layout::math::{FormulaLayout, Glyph, LayoutNode, layout_formula};
use vutex_types::math::Fixed;

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
