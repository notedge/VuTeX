//! Math-list / noad normalization layer.
//!
//! TeX's math engine lowers the syntax tree into a flat list of **noads**,
//! each carrying a role (the 8 spacing classes from Appendix G), an optional
//! nucleus, sub/superscripts, and the math style that governs it.  VuTeX's
//! [`SemanticNode`] tree currently mixes syntax (grouping), semantic role,
//! style commands and layout hints in one enum, which makes the spacing,
//! demotion and style-propagation rules hard to reason about locally.
//!
//! This module introduces the **skeleton** of that normalization:
//!
//! * [`NoadKind`] — the 8 Appendix G classes plus a few structural kinds.
//! * [`Noad`] — a single normalized atom with nucleus, scripts, style and
//!   a `cramped` flag that TeX uses for radicals and denominators.
//! * [`MathList`] — a newtype over `Vec<Noad>`.
//! * [`MathStyleState`] — the recursive `(MathStyle, cramped)` pair that
//!   descends into subtrees.
//! * [`normalize_math_list`] — flatten transparent groups and apply explicit
//!   role annotations.  Bin/Rel/Op **demotion** is intentionally left as a
//!   TODO for the next phase.
//! * [`From<&[SemanticNode]>` for `MathList`] — a shallow projector that
//!   lowers a SemanticNode sequence into a MathList.
//!
//! **This module is NOT yet wired into the main layout pipeline.**  The
//! existing `SemanticNode → LayoutNode` path in `vutex-layout` is
//! untouched.  The goal here is to land the type contract, projectors and
//! a first normalizer so the subsequent P0 migration has a stable target.

use serde::{Deserialize, Serialize};

use super::semantic::{MathStyle, SemanticNode, SymbolRole};

/// TeX Appendix G spacing classes + structural extensions.
///
/// The 8 classes control which glue TeX inserts between two adjacent atoms
/// (Ordinary, Binary, Relation, Large Operator, Closing, Opening,
/// Punctuation, Inner).  [`NoadKind::Fence`], [`NoadKind::Radical`],
/// [`NoadKind::Vcent`] and [`NoadKind::TableCell`] carry additional
/// structural information that the spacing table does not need to know
/// about; they sit alongside the 8 base classes because every noad has
/// exactly one kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NoadKind {
    /// Ordinary atom (the default for most symbols and text).
    Atom,
    /// Binary operator like `+`, `-`, `\times`.
    Bin,
    /// Relation like `=`, `<`, `\subset`.
    Rel,
    /// Large operator like `\sum`, `\int`.
    Op,
    /// Closing delimiter like `)`, `]`, `\rangle`.
    Close,
    /// Opening delimiter like `(`, `[`, `\langle`.
    Open,
    /// Punctuation like `,`, `;`.
    Punct,
    /// Inner atom produced by `\left…\right`, fractions, etc.
    Inner,
    /// Structural kind: `\left…\right` fence pair.
    Fence,
    /// Structural kind: `\sqrt`, `\root n of`.
    Radical,
    /// Structural kind: `\vcenter` box.
    Vcent,
    /// Structural kind: matrix cell.
    TableCell,
}

impl NoadKind {
    /// True if this class participates in the Appendix G 9×9 spacing table.
    ///
    /// Fence / Radical / Vcent / TableCell are structural containers; the
    /// spacing engine treats them as `Inner` for the purpose of inter-atom
    /// glue, which is what [`spacing_class`] returns.
    pub fn is_table_class(self) -> bool {
        matches!(
            self,
            NoadKind::Atom
                | NoadKind::Bin
                | NoadKind::Rel
                | NoadKind::Op
                | NoadKind::Close
                | NoadKind::Open
                | NoadKind::Punct
                | NoadKind::Inner
        )
    }

    /// Collapse structural kinds to their effective spacing class.
    ///
    /// TeX treats fences, radicals and `\vcenter` boxes as Inner for glue
    /// purposes.  Table cells are flattened away before spacing is applied
    /// so they also resolve to Inner.
    pub fn spacing_class(self) -> NoadKind {
        if self.is_table_class() {
            self
        }
        else {
            NoadKind::Inner
        }
    }

    /// Map from [`SymbolRole`] to the base spacing kind.
    pub fn from_role(role: SymbolRole) -> Self {
        match role {
            SymbolRole::Ordinary => NoadKind::Atom,
            SymbolRole::Operator | SymbolRole::LargeOperator => NoadKind::Op,
            SymbolRole::Binary => NoadKind::Bin,
            SymbolRole::Relation => NoadKind::Rel,
            SymbolRole::Opening => NoadKind::Open,
            SymbolRole::Closing => NoadKind::Close,
            SymbolRole::Punctuation => NoadKind::Punct,
            SymbolRole::Inner => NoadKind::Inner,
        }
    }
}

/// A single normalized math atom.
///
/// Every noad has exactly one [`NoadKind`] plus an optional nucleus, an
/// optional sub/superscript pair, the math style that governs glyph metrics
/// and placement, and TeX's `cramped` flag (raised by radicals and fraction
/// denominators; raised again under every superscript regardless of origin).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Noad {
    /// The spacing / structural class for this noad.
    pub kind: NoadKind,
    /// The nucleus — a shallow reference to the original [`SemanticNode`]
    /// subtree that produced this noad, so downstream consumers still have
    /// full access to the parsed form before a deeper nucleus enum replaces
    /// it.
    pub nucleus: Option<Box<SemanticNode>>,
    /// Attached sub/superscripts, if any.
    pub scripts: Option<Scripts>,
    /// True when this noad carries an explicit `\limits` / `\nolimits`
    /// override.  Only meaningful when `kind == NoadKind::Op`.
    pub limits: Option<bool>,
    /// Math style that applies to this noad's own nucleus.
    pub style: MathStyle,
    /// TeX `cramped` flag.  Raised inside radicals and fraction denominators;
    /// also raised whenever we descend into a superscript.
    pub cramped: bool,
}

/// Sub- and/or superscript attached to a [`Noad`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scripts {
    pub sub: Option<Box<SemanticNode>>,
    pub sup: Option<Box<SemanticNode>>,
}

impl Noad {
    /// Constructor for the most common case: a plain atom of the given kind.
    pub fn new(kind: NoadKind, style: MathStyle, cramped: bool) -> Self {
        Self {
            kind,
            nucleus: None,
            scripts: None,
            limits: None,
            style,
            cramped,
        }
    }

    /// True when this noad should participate in Appendix G spacing glue.
    pub fn has_spacing_class(&self) -> bool {
        self.kind.is_table_class()
    }

    /// Effective spacing kind after structural resolution.
    pub fn effective_kind(&self) -> NoadKind {
        self.kind.spacing_class()
    }
}

/// A flat, normalized list of [`Noad`]s — TeX's equivalent of a "math list".
///
/// The newtype exists so downstream crates can depend on a named type
/// rather than `Vec<Noad>` everywhere, and so we can attach helper methods
/// and trait implementations to it without polluting `Vec`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct MathList(pub Vec<Noad>);

impl MathList {
    /// Create an empty list.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Number of noads currently in the list.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when the list contains no noads.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over the noads.
    pub fn iter(&self) -> std::slice::Iter<'_, Noad> {
        self.0.iter()
    }

    /// Append a noad to the end.
    pub fn push(&mut self, noad: Noad) {
        self.0.push(noad);
    }
}

impl FromIterator<Noad> for MathList {
    fn from_iter<T: IntoIterator<Item = Noad>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for MathList {
    type Item = Noad;
    type IntoIter = std::vec::IntoIter<Noad>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Recursive style state: `(MathStyle, cramped)`.
///
/// TeX tracks both quantities together because `cramped` depends on the
/// structure we entered (radical, fraction denominator) and on whether
/// we descend into a superscript; it is not derivable from `MathStyle`
/// alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathStyleState {
    pub style: MathStyle,
    pub cramped: bool,
}

impl MathStyleState {
    /// Default: Display, not cramped.
    pub const DISPLAY: Self = Self { style: MathStyle::Display, cramped: false };

    /// Display style inside a radical or a fraction denominator.
    pub const DISPLAY_CRAMPED: Self = Self { style: MathStyle::Display, cramped: true };

    /// Push into a subscript: style drops one level, cramped inherited.
    pub fn subscript(self) -> Self {
        Self {
            style: demote_script(self.style),
            cramped: self.cramped,
        }
    }

    /// Push into a superscript: style drops one level, **always** cramped.
    pub fn superscript(self) -> Self {
        Self {
            style: demote_script(self.style),
            cramped: true,
        }
    }

    /// Enter a radical or fraction denominator.
    pub fn cramp(self) -> Self {
        Self { style: self.style, cramped: true }
    }
}

fn demote_script(s: MathStyle) -> MathStyle {
    match s {
        MathStyle::Display | MathStyle::Text => MathStyle::Script,
        MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
    }
}

/// Shallow projection from a `SemanticNode` slice into a [`MathList`].
///
/// This is intentionally a **first pass**: we flatten `Sequence` groups,
/// honour `SetMathStyle` style switches, and project individual atoms to
/// their [`NoadKind`].  Fractions, radicals, delimiters, matrices and
/// other structural nodes still produce a single `Inner` noad whose
/// nucleus points back to the original `SemanticNode` — a deeper nucleus
/// decomposition will come later.
///
/// Transparent flattening rules:
///
/// * `SemanticNode::Sequence` — descend, append children (flat).
/// * `SemanticNode::SetMathStyle(s)` — push `s` as the current style for
///   subsequent noads.  Does **not** itself emit a noad.
/// * `SemanticNode::LimitsSpec(b)` — sets `limits` on the next `Op` noad.
pub fn project_semantic(nodes: &[SemanticNode]) -> MathList {
    let mut state = MathStyleState::DISPLAY;
    let mut list = MathList::new();
    let mut pending_limits: Option<bool> = None;

    for node in nodes {
        project_one(node, &mut state, &mut list, &mut pending_limits);
    }

    list
}

fn project_one(
    node: &SemanticNode,
    state: &mut MathStyleState,
    list: &mut MathList,
    pending_limits: &mut Option<bool>,
) {
    match node {
        SemanticNode::Sequence(children) => {
            for c in children {
                project_one(c, state, list, pending_limits);
            }
        }
        SemanticNode::SetMathStyle(s) => {
            state.style = *s;
        }
        SemanticNode::LimitsSpec(b) => {
            *pending_limits = Some(*b);
        }
        SemanticNode::Symbol { role, .. } => {
            let kind = NoadKind::from_role(*role);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            if kind == NoadKind::Op {
                if let Some(b) = pending_limits.take() {
                    noad.limits = Some(b);
                }
            }
            list.push(noad);
        }
        SemanticNode::Text { .. } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
        SemanticNode::Space { .. } => {
            // TeX treats explicit spaces as inner atoms for spacing.
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
        SemanticNode::Subscript { base, sub } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sub_state = state.subscript();
            noad.scripts = Some(Scripts {
                sub: Some(Box::new(with_style(sub, sub_state.style))),
                sup: None,
            });
            list.push(noad);
        }
        SemanticNode::Superscript { base, sup } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sup_state = state.superscript();
            noad.scripts = Some(Scripts {
                sub: None,
                sup: Some(Box::new(with_style(sup, sup_state.style))),
            });
            list.push(noad);
        }
        SemanticNode::SubSuperscript { base, sub, sup } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sub_state = state.subscript();
            let sup_state = state.superscript();
            noad.scripts = Some(Scripts {
                sub: Some(Box::new(with_style(sub, sub_state.style))),
                sup: Some(Box::new(with_style(sup, sup_state.style))),
            });
            list.push(noad);
        }
        SemanticNode::Fraction { .. } => {
            // Fraction nuclei are Inner; deeper split into num/den noads is
            // deferred to a later phase.
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
        SemanticNode::Radical { .. } => {
            let mut noad = Noad::new(NoadKind::Radical, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
        SemanticNode::Delimited { .. } => {
            let mut noad = Noad::new(NoadKind::Fence, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
        SemanticNode::Accent { .. } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
        SemanticNode::HorizontalBox { content, .. } | SemanticNode::Paragraph { content, .. } => {
            // Flattened: descend and emit children as if they were in a Sequence.
            for c in content {
                project_one(c, state, list, pending_limits);
            }
        }
        SemanticNode::VerticalBox { content, .. } => {
            // Vertical box is Inner.
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
            let _ = content; // deeper decomposition deferred
        }
        SemanticNode::Matrix { .. } => {
            let mut noad = Noad::new(NoadKind::TableCell, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            list.push(noad);
        }
    }
}

/// Wrap a node with a matching `SetMathStyle` when the target style differs
/// from the node's natural style.  In the shallow projection the sub/sup
/// nodes still live as raw `SemanticNode`s so we push the style imperatively
/// in front of them.
fn with_style(node: &SemanticNode, target: MathStyle) -> SemanticNode {
    // For the first pass we don't introspect inside the sub/sup subtree —
    // the style command at the head is a hint for the layout engine.
    SemanticNode::Sequence(vec![SemanticNode::SetMathStyle(target), node.clone()])
}

/// Normalize a projected [`MathList`].
///
/// Current passes (intentionally shallow):
///
/// 1. **Transparent group flattening** — already done during projection
///    (Sequence, HorizontalBox descend); kept here as a no-op so future
///    passes can accumulate without touching the projection path.
/// 2. **Explicit role annotation** — every noad carries a kind; this
///    pass is a stub awaiting deeper nucleus inspection.
///
/// TODO (next phase):
///
/// * Bin / Rel / Op demotion per TeX rules (e.g. Bin next to Close or
///   Punct demotes to Atom).
/// * Consecutive Atom merging.
/// * Style re-propagation from `cramped` overrides (radical/denominator).
pub fn normalize_math_list(list: MathList) -> MathList {
    // Pass 1: transparent flattening is already done in `project_one`
    // for Sequence and HorizontalBox.  We keep the function so future
    // normalizer passes can accumulate here.
    list
}

// ---------------------------------------------------------------------------
// From<&[SemanticNode]> for MathList
// ---------------------------------------------------------------------------

impl From<&[SemanticNode]> for MathList {
    fn from(nodes: &[SemanticNode]) -> Self {
        project_semantic(nodes)
    }
}

impl From<&Vec<SemanticNode>> for MathList {
    fn from(nodes: &Vec<SemanticNode>) -> Self {
        project_semantic(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{GlyphKey, SemanticNode, SymbolRole};

    fn sym(c: char, role: SymbolRole) -> SemanticNode {
        SemanticNode::Symbol {
            glyph_key: GlyphKey::from_char(c, None, crate::math::FontStyle::Math),
            role,
        }
    }

    #[test]
    fn simple_atom_projection() {
        let nodes = vec![sym('x', SymbolRole::Ordinary)];
        let list = MathList::from(nodes.as_slice());
        assert_eq!(list.len(), 1);
        assert_eq!(list.0[0].kind, NoadKind::Atom);
        assert_eq!(list.0[0].style, MathStyle::Display);
        assert!(!list.0[0].cramped);
    }

    #[test]
    fn sequence_flattens() {
        let nodes = vec![
            SemanticNode::Sequence(vec![sym('a', SymbolRole::Ordinary), sym('b', SymbolRole::Ordinary)]),
            sym('+', SymbolRole::Binary),
        ];
        let list = MathList::from(nodes.as_slice());
        // Sequence flattens so we get three noads total.
        assert_eq!(list.len(), 3);
        assert_eq!(list.0[0].kind, NoadKind::Atom);
        assert_eq!(list.0[1].kind, NoadKind::Atom);
        assert_eq!(list.0[2].kind, NoadKind::Bin);
    }

    #[test]
    fn style_command_propagates() {
        let nodes = vec![
            SemanticNode::SetMathStyle(MathStyle::Script),
            sym('x', SymbolRole::Ordinary),
            SemanticNode::SetMathStyle(MathStyle::Display),
            sym('y', SymbolRole::Ordinary),
        ];
        let list = MathList::from(nodes.as_slice());
        assert_eq!(list.len(), 2);
        assert_eq!(list.0[0].style, MathStyle::Script);
        assert_eq!(list.0[1].style, MathStyle::Display);
    }

    #[test]
    fn symbol_role_maps_to_noad_kind() {
        let cases: Vec<(SymbolRole, NoadKind)> = vec![
            (SymbolRole::Ordinary, NoadKind::Atom),
            (SymbolRole::Binary, NoadKind::Bin),
            (SymbolRole::Relation, NoadKind::Rel),
            (SymbolRole::Operator, NoadKind::Op),
            (SymbolRole::LargeOperator, NoadKind::Op),
            (SymbolRole::Opening, NoadKind::Open),
            (SymbolRole::Closing, NoadKind::Close),
            (SymbolRole::Punctuation, NoadKind::Punct),
            (SymbolRole::Inner, NoadKind::Inner),
        ];
        for (role, expected) in cases {
            assert_eq!(NoadKind::from_role(role), expected, "role {:?}", role);
        }
    }

    #[test]
    fn subscript_attaches() {
        let nodes = vec![SemanticNode::Subscript {
            base: Box::new(sym('x', SymbolRole::Ordinary)),
            sub: Box::new(sym('i', SymbolRole::Ordinary)),
        }];
        let list = MathList::from(nodes.as_slice());
        assert_eq!(list.len(), 1);
        let n = &list.0[0];
        assert!(n.scripts.is_some());
        assert!(n.scripts.as_ref().unwrap().sub.is_some());
        assert!(n.scripts.as_ref().unwrap().sup.is_none());
    }

    #[test]
    fn superscript_is_always_cramped_in_state() {
        let st = MathStyleState::DISPLAY;
        let sup = st.superscript();
        assert!(sup.cramped, "superscript must be cramped");
        assert_eq!(sup.style, MathStyle::Script);
    }

    #[test]
    fn subscript_inherits_cramped() {
        let st = MathStyleState::DISPLAY.cramp();
        let sub = st.subscript();
        assert!(sub.cramped, "subscript inherits cramped from parent");
    }

    #[test]
    fn normalize_is_identity_for_projection() {
        let nodes = vec![sym('a', SymbolRole::Ordinary), sym('b', SymbolRole::Binary)];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected.clone());
        assert_eq!(projected, normalized);
    }

    #[test]
    fn spacing_class_collapses_structural() {
        assert_eq!(NoadKind::Fence.spacing_class(), NoadKind::Inner);
        assert_eq!(NoadKind::Radical.spacing_class(), NoadKind::Inner);
        assert_eq!(NoadKind::Vcent.spacing_class(), NoadKind::Inner);
        assert_eq!(NoadKind::TableCell.spacing_class(), NoadKind::Inner);
        assert_eq!(NoadKind::Bin.spacing_class(), NoadKind::Bin);
        assert_eq!(NoadKind::Atom.spacing_class(), NoadKind::Atom);
    }

    #[test]
    fn limits_spec_attaches_to_next_op() {
        let nodes = vec![
            SemanticNode::LimitsSpec(true),
            sym('∑', SymbolRole::LargeOperator),
            sym('x', SymbolRole::Ordinary),
        ];
        let list = MathList::from(nodes.as_slice());
        assert_eq!(list.len(), 2);
        assert_eq!(list.0[0].kind, NoadKind::Op);
        assert_eq!(list.0[0].limits, Some(true));
        assert_eq!(list.0[1].kind, NoadKind::Atom);
    }
}
