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

    /// Create a state from any outer math style, not cramped.
    pub const fn from_style(style: MathStyle) -> Self {
        Self { style, cramped: false }
    }

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
///
/// `base_style` is the outer formula context math style (Display / Text /
/// Script / ScriptScript).  Imperative nodes still override `state.style`
/// in place, so subsequent noads carry the correct per-node style.
pub fn project_semantic(nodes: &[SemanticNode], base_style: MathStyle) -> MathList {
    let mut state = MathStyleState::from_style(base_style);
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
        SemanticNode::Delimited { left, right, content } => {
            // Left delimiter → Open noad with a synthetic Symbol nucleus
            // so the spacing engine can read its glyph for metrics.
            if let Some(gk) = left {
                let mut open = Noad::new(NoadKind::Open, state.style, state.cramped);
                open.nucleus = Some(Box::new(SemanticNode::Symbol {
                    glyph_key: gk.clone(),
                    role: SymbolRole::Opening,
                }));
                list.push(open);
            }

            // Content → recursive projection, preserving its internal spacing
            // classes (Atom / Bin / Rel / ...).  When the body is completely
            // empty (e.g. `\left(\right)`), TeX still emits an Inner atom so
            // the fence pair participates in spacing; we replicate that here.
            let before = list.len();
            project_one(content, state, list, pending_limits);
            if list.len() == before {
                list.push(Noad::new(NoadKind::Inner, state.style, state.cramped));
            }

            // Right delimiter → Close noad.
            if let Some(gk) = right {
                let mut close = Noad::new(NoadKind::Close, state.style, state.cramped);
                close.nucleus = Some(Box::new(SemanticNode::Symbol {
                    glyph_key: gk.clone(),
                    role: SymbolRole::Closing,
                }));
                list.push(close);
            }
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

/// Normalize a projected [`MathList`] with TeX Appendix G passes.
///
/// 1. **Boundary trim** — strip leading `Open`/`Close`/`Punct` noads
///    that have no nucleus (bare grouping atoms with no glyph of their
///    own).  Fence pairs emitted by the `Delimited` projector carry a
///    proper `Symbol` nucleus for their delimiters and are therefore
///    preserved; the trim only catches artifact atoms from future
///    sources that have neither a glyph nor a sub/superscript.
/// 2. **Bin / Rel / Op demotion** — downgrade binary, relation and large
///    operator atoms per TeX Appendix G Rules 5–7, evaluated in one
///    left-to-right pass (each noad sees its original neighbours; TeX
///    does not re-resolve).
/// 3. **Consecutive Atom merge** — fold adjacent `Atom` noads into a
///    single one; later scripts override earlier ones.
pub fn normalize_math_list(mut list: MathList) -> MathList {
    // ── Pass 1: boundary trim ────────────────────────────────────────
    trim_boundary(&mut list);

    // ── Pass 2: Bin / Rel / Op demote per TeX Appendix G ──────────────
    let v = demote_pass(&list.0);

    // ── Pass 3: consecutive Atom merge ────────────────────────────────
    let v = merge_atoms_pass(v);

    MathList(v)
}

// ────────────────────────────────────────────────────────────────────────
// Pass 1 — boundary trim
// ────────────────────────────────────────────────────────────────────────

/// Strip leading and trailing `Open`/`Close`/`Punct` noads that have no
/// nucleus.
///
/// Open/Close atoms at the boundaries never have a useful neighbour on
/// the outer side, so their spacing contribution is zero.  Bare atoms of
/// those kinds that carry no glyph (no nucleus) are pure artifacts and
/// can be dropped safely.  Delimiter noads emitted by the
/// [`SemanticNode::Delimited`] projector always carry a synthetic
/// `Symbol` nucleus, so they survive this pass — only placeholder noads
/// from other sources are trimmed.
fn trim_boundary(list: &mut MathList) {
    // Trim leading.
    while let Some(first) = list.0.first() {
        let can_drop = first.nucleus.is_none()
            && matches!(first.kind, NoadKind::Open | NoadKind::Close | NoadKind::Punct);
        if can_drop {
            list.0.remove(0);
        } else {
            break;
        }
    }
    // Trim trailing.
    while let Some(last) = list.0.last() {
        let can_drop = last.nucleus.is_none()
            && matches!(last.kind, NoadKind::Open | NoadKind::Close | NoadKind::Punct);
        if can_drop {
            list.0.pop();
        } else {
            break;
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// Pass 2 — demote
// ────────────────────────────────────────────────────────────────────────

/// TeX Appendix G demote pass — one left-to-right sweep, every noad sees
/// its original neighbours (no iteration over partially demoted results).
///
/// Neighbours are read via [`Noad::effective_kind`] so structural kinds
/// (`Fence`, `Radical`, `Vcent`, `TableCell`) collapse to `Inner` before
/// the demote rules fire — they participate as an Inner neighbour exactly
/// the way TeX treats fences and radicals for spacing.
fn demote_pass(input: &[Noad]) -> Vec<Noad> {
    let n = input.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let left = (i > 0).then(|| input[i - 1].effective_kind());
        let right = (i + 1 < n).then(|| input[i + 1].effective_kind());
        let mut noad = input[i].clone();
        match noad.kind {
            NoadKind::Bin => {
                if bin_demotes(left, right) {
                    noad.kind = NoadKind::Atom;
                }
            }
            NoadKind::Rel => {
                if rel_demotes(left) {
                    noad.kind = NoadKind::Bin;
                }
            }
            NoadKind::Op => {
                if op_demotes(left, right) {
                    noad.kind = NoadKind::Atom;
                }
            }
            _ => {}
        }
        out.push(noad);
    }
    out
}

/// Rule 5 — Bin → Atom.
///
/// Demotes when the **left** neighbour is "strong" enough to absorb the
/// binary operator (or Bin is first), **or** the **right** neighbour is
/// likewise strong enough (or Bin is last).
///
/// ```text
/// Bin → Atom if:
///   first (no left)  or left ∈ {Bin, Open, Rel, Op, Punct}
///   OR
///   last  (no right) or right ∈ {Rel, Close, Punct}
/// ```
///
/// `Close` is *not* a Bin-left canceller and `Open` is *not* a Bin-right
/// canceller — `(a+b)` still keeps its Bin with thick glue on both sides.
fn bin_demotes(left: Option<NoadKind>, right: Option<NoadKind>) -> bool {
    use NoadKind::*;
    let left_strong = matches!(left, None | Some(Bin | Open | Rel | Op | Punct));
    let right_strong = matches!(right, None | Some(Rel | Close | Punct));
    left_strong || right_strong
}

/// Rule 6 — Rel → Bin when left ∈ {Open, Bin}.
fn rel_demotes(left: Option<NoadKind>) -> bool {
    matches!(left, Some(NoadKind::Open | NoadKind::Bin))
}

/// Rule 7 — Op → Atom when flanked by demoting neighbours.
///
/// Large operators are also mapped to [`NoadKind::Op`] by
/// [`NoadKind::from_role`], so this same rule covers both.
fn op_demotes(left: Option<NoadKind>, right: Option<NoadKind>) -> bool {
    use NoadKind::*;
    let left_strong = matches!(left, Some(Open | Close | Bin | Punct | Op));
    let right_strong = matches!(right, Some(Bin | Rel | Close | Punct | Op));
    left_strong || right_strong
}

// ────────────────────────────────────────────────────────────────────────
// Pass 3 — merge consecutive Atoms
// ────────────────────────────────────────────────────────────────────────

/// Fold adjacent `Atom` noads into a single Atom.
///
/// The resulting Atom keeps the **first** Atom's nucleus and style; when
/// later Atoms carry `sub` / `sup` scripts, those slots override (if set)
/// the ones already on the merged Atom.  Nucleus falls back to the later
/// Atom when the first has none.
fn merge_atoms_pass(input: Vec<Noad>) -> Vec<Noad> {
    let mut out: Vec<Noad> = Vec::with_capacity(input.len());
    for noad in input {
        if let Some(last) = out.last_mut() {
            if last.kind == NoadKind::Atom && noad.kind == NoadKind::Atom {
                // Later nucleus fills a hole but never overwrites an existing one.
                if last.nucleus.is_none() {
                    last.nucleus = noad.nucleus;
                }
                // Later scripts override slot-by-slot.
                if let Some(new_sc) = noad.scripts {
                    match last.scripts.as_mut() {
                        Some(old) => {
                            if new_sc.sub.is_some() {
                                old.sub = new_sc.sub;
                            }
                            if new_sc.sup.is_some() {
                                old.sup = new_sc.sup;
                            }
                        }
                        None => last.scripts = Some(new_sc),
                    }
                }
                continue;
            }
        }
        out.push(noad);
    }
    out
}

// ---------------------------------------------------------------------------
// From<&[SemanticNode]> for MathList
// ---------------------------------------------------------------------------

impl From<&[SemanticNode]> for MathList {
    fn from(nodes: &[SemanticNode]) -> Self {
        project_semantic(nodes, MathStyle::Display)
    }
}

impl From<&Vec<SemanticNode>> for MathList {
    fn from(nodes: &Vec<SemanticNode>) -> Self {
        project_semantic(nodes, MathStyle::Display)
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
    fn bin_stays_between_atoms() {
        // a + b → Bin between two Atoms must NOT demote.
        let nodes = vec![
            sym('a', SymbolRole::Ordinary),
            sym('+', SymbolRole::Binary),
            sym('b', SymbolRole::Ordinary),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized.0[0].kind, NoadKind::Atom);
        assert_eq!(normalized.0[1].kind, NoadKind::Bin);
        assert_eq!(normalized.0[2].kind, NoadKind::Atom);
    }

    #[test]
    fn bin_demotes_before_open() {
        // ( + → Open + Atom — Bin after Open (left ∈ Open) demotes.
        let nodes = vec![
            sym('(', SymbolRole::Opening),
            sym('+', SymbolRole::Binary),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Open);
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }

    #[test]
    fn bin_demotes_after_close() {
        // ) + → Close + Atom — Bin at the end (no right neighbour) demotes.
        let nodes = vec![
            sym(')', SymbolRole::Closing),
            sym('+', SymbolRole::Binary),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Close);
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }

    #[test]
    fn rel_demotes_after_open() {
        // ( = → Open + Bin — Rel after Open demotes to Bin.
        let nodes = vec![
            sym('(', SymbolRole::Opening),
            sym('=', SymbolRole::Relation),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Open);
        assert_eq!(normalized.0[1].kind, NoadKind::Bin);
    }

    #[test]
    fn op_demotes_before_close() {
        // ∑ ) → Atom + Close — LargeOp (Op) before Close demotes.
        let nodes = vec![
            sym('∑', SymbolRole::LargeOperator),
            sym(')', SymbolRole::Closing),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Atom);
        assert_eq!(normalized.0[1].kind, NoadKind::Close);
    }

    #[test]
    fn op_stays_at_first() {
        // ∑ + x — LargeOp is first (no left neighbour), so does NOT demote.
        // Plus, the Op has right neighbour Bin — but right_strong needs Bin,
        // so it *does* demote by the right side.  Wait — let me re-check...
        // Rule 7: Op → Atom if right ∈ {Bin, Rel, Close, Punct, Op}.
        // So ∑ + x → Op right=Bin → Op demotes to Atom → Atom + Bin + Atom.
        // Hmm, the test name says "op_stays_at_first".  Let me change the
        // scenario so Op stays: put an Atom on the right instead.
        // Actually let me just test what the demote engine *should* do and
        // pick a scenario where Op genuinely stays.
        let nodes = vec![
            sym('∑', SymbolRole::LargeOperator),
            sym('x', SymbolRole::Ordinary),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        // first (no left) → left_strong false; right=Atom → right_strong false.
        // Op does NOT demote.
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Op);
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }

    #[test]
    fn continuous_atoms_merge() {
        // a b c → three Atoms collapse to one.
        let nodes = vec![
            sym('a', SymbolRole::Ordinary),
            sym('b', SymbolRole::Ordinary),
            sym('c', SymbolRole::Ordinary),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized.0[0].kind, NoadKind::Atom);
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

    // ── Delimited → Open · content · Close expansion ──────────────────

    #[test]
    fn delimited_expands_to_open_inner_close() {
        // \left(a + b\right) → Open · Atom · Bin · Atom · Close
        let delim = SemanticNode::Delimited {
            left: Some(GlyphKey::from_char('(', None, crate::math::FontStyle::Math)),
            right: Some(GlyphKey::from_char(')', None, crate::math::FontStyle::Math)),
            content: Box::new(SemanticNode::Sequence(vec![
                sym('a', SymbolRole::Ordinary),
                sym('+', SymbolRole::Binary),
                sym('b', SymbolRole::Ordinary),
            ])),
        };
        let list = MathList::from(std::slice::from_ref(&delim));
        assert_eq!(list.len(), 5);
        assert_eq!(list.0[0].kind, NoadKind::Open);
        assert!(list.0[0].nucleus.is_some(), "Open must carry delimiter glyph");
        assert_eq!(list.0[1].kind, NoadKind::Atom);
        assert_eq!(list.0[2].kind, NoadKind::Bin);
        assert_eq!(list.0[3].kind, NoadKind::Atom);
        assert_eq!(list.0[4].kind, NoadKind::Close);
        assert!(list.0[4].nucleus.is_some(), "Close must carry delimiter glyph");
    }

    #[test]
    fn delimited_empty_content_gets_inner_placeholder() {
        // \left(\right) — empty body → Open · Inner · Close (TeX semantics).
        let delim = SemanticNode::Delimited {
            left: Some(GlyphKey::from_char('(', None, crate::math::FontStyle::Math)),
            right: Some(GlyphKey::from_char(')', None, crate::math::FontStyle::Math)),
            content: Box::new(SemanticNode::Sequence(vec![])),
        };
        let list = MathList::from(std::slice::from_ref(&delim));
        assert_eq!(list.len(), 3);
        assert_eq!(list.0[0].kind, NoadKind::Open);
        assert_eq!(list.0[1].kind, NoadKind::Inner);
        assert_eq!(list.0[2].kind, NoadKind::Close);
    }

    #[test]
    fn delimited_content_projected_recursively() {
        // Content containing SetMathStyle must propagate style.
        let delim = SemanticNode::Delimited {
            left: Some(GlyphKey::from_char('[', None, crate::math::FontStyle::Math)),
            right: Some(GlyphKey::from_char(']', None, crate::math::FontStyle::Math)),
            content: Box::new(SemanticNode::Sequence(vec![
                SemanticNode::SetMathStyle(crate::math::MathStyle::Script),
                sym('x', SymbolRole::Ordinary),
            ])),
        };
        let list = MathList::from(std::slice::from_ref(&delim));
        assert_eq!(list.len(), 3);
        assert_eq!(list.0[0].kind, NoadKind::Open);
        assert_eq!(list.0[1].kind, NoadKind::Atom);
        assert_eq!(list.0[1].style, crate::math::MathStyle::Script);
        assert_eq!(list.0[2].kind, NoadKind::Close);
    }

    // ── Boundary trim ────────────────────────────────────────────────

    #[test]
    fn boundary_trim_drops_bare_open_without_nucleus() {
        // A bare Open noad with no nucleus at the leading edge is trimmed.
        let mut list = MathList::new();
        list.push(Noad::new(NoadKind::Open, crate::math::MathStyle::Display, false));
        list.push(Noad::new(NoadKind::Atom, crate::math::MathStyle::Display, false));
        // Add a trailing Close with no nucleus too.
        list.push(Noad::new(NoadKind::Close, crate::math::MathStyle::Display, false));

        let normalized = normalize_math_list(list);
        assert_eq!(normalized.len(), 1, "bare Open+Close at boundaries must be trimmed");
        assert_eq!(normalized.0[0].kind, NoadKind::Atom);
    }

    #[test]
    fn boundary_trim_preserves_delimited_open_close() {
        // Open/Close emitted from Delimited carry a nucleus and survive trim.
        let delim = SemanticNode::Delimited {
            left: Some(GlyphKey::from_char('(', None, crate::math::FontStyle::Math)),
            right: Some(GlyphKey::from_char(')', None, crate::math::FontStyle::Math)),
            content: Box::new(SemanticNode::Sequence(vec![])),
        };
        let projected = MathList::from(std::slice::from_ref(&delim));
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized.0[0].kind, NoadKind::Open);
        assert_eq!(normalized.0[1].kind, NoadKind::Inner);
        assert_eq!(normalized.0[2].kind, NoadKind::Close);
    }

    #[test]
    fn delimited_bin_demotes_after_open() {
        // \left(+ — the Bin after Delimited's Open must demote exactly like
        // bin_demotes_before_open, but the Open here comes from Delimited.
        let delim = SemanticNode::Delimited {
            left: Some(GlyphKey::from_char('(', None, crate::math::FontStyle::Math)),
            right: None,
            content: Box::new(SemanticNode::Sequence(vec![sym('+', SymbolRole::Binary)])),
        };
        let projected = MathList::from(std::slice::from_ref(&delim));
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.0[0].kind, NoadKind::Open);
        // Bin is at position 1, left=Open → demotes to Atom.
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }
}
