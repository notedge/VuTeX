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
//! * [`normalize_math_list`] — flatten transparent groups, demote Bin / Rel /
//!   Op per Appendix G and merge consecutive text atoms in one left-to-right
//!   sweep.
//! * [`From<&[SemanticNode]>` for `MathList`] — a shallow projector that
//!   lowers a SemanticNode sequence into a MathList.
//!
//! The layout engine consumes this normalization: every math list it lays out
//! is projected and normalized first, so inter-atom spacing only ever reads an
//! already-resolved [`NoadKind`] and never re-derives a class from the source
//! [`SymbolRole`].  There is no second demotion rule anywhere downstream.

use serde::{Deserialize, Serialize};

use super::{
    operator::LimitsPolicy,
    semantic::{FontStyle, MathStyle, SemanticNode, SymbolRole},
};

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
        if self.is_table_class() { self } else { NoadKind::Inner }
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
/// and placement, and TeX's `cramped` flag (raised inside radicals, in
/// fraction denominators and in subscripts, inherited everywhere else).
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
    /// An explicit `\limits` / `\nolimits` override, if any.  Only meaningful
    /// when `kind == NoadKind::Op`.
    pub limits: Option<LimitsPolicy>,
    /// Math style that applies to this noad's own nucleus.
    pub style: MathStyle,
    /// TeX `cramped` flag for the style that governs this noad's nucleus.
    /// Raised inside radicals, in fraction denominators and in subscripts.
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
        Self { kind, nucleus: None, scripts: None, limits: None, style, cramped }
    }

    /// True when this noad should participate in Appendix G spacing glue.
    pub fn has_spacing_class(&self) -> bool {
        self.kind.is_table_class()
    }

    /// Effective spacing kind after structural resolution.
    pub fn effective_kind(&self) -> NoadKind {
        self.kind.spacing_class()
    }

    /// True when this noad's nucleus is a `SemanticNode::Text` (not a
    /// Symbol, not any other variant).
    ///
    /// Text noads come from `SemanticNode::Text { text, font_style }` while
    /// Symbol noads come from `SemanticNode::Symbol { glyph_key, role }`.
    /// We only merge adjacent Text atoms — Symbol atoms (even when both
    /// happen to be Ordinary) must remain distinct because TeX keeps a
    /// demoted `Bin→Atom` as its own spacing entity.
    pub fn is_text_atom(&self) -> bool {
        matches!(self.nucleus.as_deref(), Some(SemanticNode::Text { .. }))
    }

    /// The font face of this noad's nucleus when it is a
    /// [`SemanticNode::Text`], and `None` for every other nucleus.
    ///
    /// The face is part of a Text atom's identity: `\text{a}` and `\textbf{b}`
    /// are two atoms, not one, because the bold run must keep its own face.
    pub fn text_font_style(&self) -> Option<FontStyle> {
        match self.nucleus.as_deref() {
            Some(SemanticNode::Text { font_style, .. }) => Some(*font_style),
            _ => None,
        }
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
/// TeX's `mlist_to_hlist` tracks the current math style together with the
/// `cramped` flag.  `cramped` is not derivable from `MathStyle` alone: it is
/// raised by the structure we enter (fraction denominator, radical radicand,
/// subscript) and inherited by structures that keep the parent's crampedness.
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

    /// Push into a subscript.
    ///
    /// TeX demotes the style one script level (display/text → script,
    /// script/scriptscript → scriptscript) and sets `cramped`: scripts take
    /// the *cramped* shifts so the subscript hugs the base instead of keeping
    /// the uncramped ascender clearance.
    pub fn sub(self) -> Self {
        Self { style: demote_script(self.style), cramped: true }
    }

    /// Push into a superscript.
    ///
    /// Style demotes like [`sub`](Self::sub), but `cramped` keeps the parent's
    /// value — nothing in a superscript raises it.
    pub fn sup(self) -> Self {
        Self { style: demote_script(self.style), cramped: self.cramped }
    }

    /// Fraction numerator.
    ///
    /// TeX steps the style down one level (display → text, text → script,
    /// script → scriptscript, scriptscript stays put) and keeps the parent's
    /// `cramped` flag.
    pub fn frac_num(self) -> Self {
        Self { style: demote_text(self.style), cramped: self.cramped }
    }

    /// Fraction denominator: the same step as [`frac_num`](Self::frac_num),
    /// but always cramped, which is why fractions use the *cramped* shifts.
    pub fn frac_den(self) -> Self {
        Self { style: demote_text(self.style), cramped: true }
    }

    /// Enter a radical's radicand: style unchanged, cramped forced.
    pub fn cramp(self) -> Self {
        Self { style: self.style, cramped: true }
    }
}

/// One script level down: display and text both land in script style.
fn demote_script(s: MathStyle) -> MathStyle {
    match s {
        MathStyle::Display | MathStyle::Text => MathStyle::Script,
        MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
    }
}

/// One *text* level down, the step fractions use: display → text, then
/// text → script, then script → scriptscript.
fn demote_text(s: MathStyle) -> MathStyle {
    match s {
        MathStyle::Display => MathStyle::Text,
        MathStyle::Text => MathStyle::Script,
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
///   subsequent noads.  Does **not** itself emit a noad.  Each `Sequence` is
///   a style scope: the style in force at its end is restored when the group
///   closes, so `\scriptstyle` inside `{…}` cannot leak to later siblings.
/// * `SemanticNode::LimitsSpec(policy)` — sets `limits` on the next `Op` noad.
///
/// `base_style` is the outer formula context math style (Display / Text /
/// Script / ScriptScript).  Imperative nodes still override `state.style`
/// in place, so subsequent noads carry the correct per-node style.
pub fn project_semantic(nodes: &[SemanticNode], base_style: MathStyle) -> MathList {
    let mut state = MathStyleState::from_style(base_style);
    let mut list = MathList::new();
    let mut pending_limits: Option<LimitsPolicy> = None;

    for node in nodes {
        project_one(node, &mut state, &mut list, &mut pending_limits);
    }

    list
}

fn project_one(
    node: &SemanticNode,
    state: &mut MathStyleState,
    list: &mut MathList,
    pending_limits: &mut Option<LimitsPolicy>,
) {
    match node {
        SemanticNode::Sequence(children) => {
            // A braced group is a style scope; `\scriptstyle` and friends
            // inside it must not leak to the siblings after the group.
            let outer = *state;
            for c in children {
                project_one(c, state, list, pending_limits);
            }
            *state = outer;
        }
        SemanticNode::SetMathStyle(s) => {
            state.style = *s;
        }
        SemanticNode::LimitsSpec(policy) => {
            *pending_limits = Some(*policy);
        }
        SemanticNode::Symbol { role, .. } => {
            let kind = NoadKind::from_role(*role);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Text { .. } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Space { .. } => {
            // Explicit TeX math spaces (`\,`, `\:`, `\;`, `\ `, `\quad`,
            // `\qquad`) are pure kerns — they must not participate in the
            // Appendix G spacing table on either side, so we emit them as
            // plain Atoms (Atom→Atom = NoGlue).  The kern advance is still
            // carried by the nucleus for `layout_node` to lower.
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Subscript { base, sub } => {
            let kind = kind_of_node(base);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sub_state = state.sub();
            noad.scripts = Some(Scripts { sub: Some(Box::new(with_style(sub, sub_state.style))), sup: None });
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Superscript { base, sup } => {
            let kind = kind_of_node(base);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sup_state = state.sup();
            noad.scripts = Some(Scripts { sub: None, sup: Some(Box::new(with_style(sup, sup_state.style))) });
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::SubSuperscript { base, sub, sup } => {
            let kind = kind_of_node(base);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sub_state = state.sub();
            let sup_state = state.sup();
            noad.scripts = Some(Scripts {
                sub: Some(Box::new(with_style(sub, sub_state.style))),
                sup: Some(Box::new(with_style(sup, sup_state.style))),
            });
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Fraction { num, den, line } => {
            // The fraction itself is an Inner atom in the current style, but
            // TeX sets its numerator and denominator one text level down, and
            // the denominator cramped.  Stamping the derived styles into the
            // nucleus keeps the nested scripts on that state instead of the
            // outer one.
            let num_state = state.frac_num();
            let den_state = state.frac_den();
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(SemanticNode::Fraction {
                num: Box::new(with_style(num, num_state.style)),
                den: Box::new(with_style(den, den_state.style)),
                line: *line,
            }));
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Radical { .. } => {
            // A radical's argument is typeset cramped at the current style.
            let inner = state.cramp();
            let mut noad = Noad::new(NoadKind::Radical, inner.style, inner.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        SemanticNode::Delimited { left, right, content } => {
            // Left delimiter → Open noad with a synthetic Symbol nucleus
            // so the spacing engine can read its glyph for metrics.
            if let Some(gk) = left {
                let mut open = Noad::new(NoadKind::Open, state.style, state.cramped);
                open.nucleus = Some(Box::new(SemanticNode::Symbol { glyph_key: gk.clone(), role: SymbolRole::Opening }));
                push_noad(list, open, pending_limits);
            }

            // Content → recursive projection, preserving its internal spacing
            // classes (Atom / Bin / Rel / ...).  When the body is completely
            // empty (e.g. `\left(\right)`), TeX still emits an Inner atom so
            // the fence pair participates in spacing; we replicate that here.
            let before = list.len();
            project_one(content, state, list, pending_limits);
            if list.len() == before {
                push_noad(list, Noad::new(NoadKind::Inner, state.style, state.cramped), pending_limits);
            }

            // Right delimiter → Close noad.
            if let Some(gk) = right {
                let mut close = Noad::new(NoadKind::Close, state.style, state.cramped);
                close.nucleus = Some(Box::new(SemanticNode::Symbol { glyph_key: gk.clone(), role: SymbolRole::Closing }));
                push_noad(list, close, pending_limits);
            }
        }
        SemanticNode::Accent { .. } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
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
            push_noad(list, noad, pending_limits);
            let _ = content; // deeper decomposition deferred
        }
        SemanticNode::Matrix { .. } => {
            let mut noad = Noad::new(NoadKind::TableCell, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
    }
}

/// Append `noad`, binding the `\limits` / `\nolimits` policy queued before it.
///
/// `\limits` describes the atom it follows, so the queued policy is consumed
/// here whatever that atom is.  It is only stored when the atom is an
/// operator: a policy authored after an ordinary atom is reported by the
/// parser as a misuse and must not travel on to the next operator.
fn push_noad(list: &mut MathList, mut noad: Noad, pending_limits: &mut Option<LimitsPolicy>) {
    let policy = pending_limits.take();
    if noad.kind == NoadKind::Op {
        noad.limits = policy;
    }
    list.push(noad);
}

/// Extract the [`NoadKind`] a semantic node contributes to the flat math
/// list.  Mirrors [`LayoutEngine::role_of`] but only covers the projector's
/// own needs: Symbols map through `NoadKind::from_role`, everything else
/// defaults to Atom.  Fractions / radicals / delimiters are handled by
/// their own `project_one` arms and never reach here.
fn kind_of_node(node: &SemanticNode) -> NoadKind {
    match node {
        SemanticNode::Symbol { role, .. } => NoadKind::from_role(*role),
        _ => NoadKind::Atom,
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

    // ── Pass 3: consecutive **Text** Atom merge ───────────────────────
    // Only merges Text-derived atoms; Symbol atoms stay distinct (TeX
    // semantics — a demoted Bin→Atom remains its own spacing entity).
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
        let can_drop = first.nucleus.is_none() && matches!(first.kind, NoadKind::Open | NoadKind::Close | NoadKind::Punct);
        if can_drop {
            list.0.remove(0);
        }
        else {
            break;
        }
    }
    // Trim trailing.
    while let Some(last) = list.0.last() {
        let can_drop = last.nucleus.is_none() && matches!(last.kind, NoadKind::Open | NoadKind::Close | NoadKind::Punct);
        if can_drop {
            list.0.pop();
        }
        else {
            break;
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// Pass 2 — demote
// ────────────────────────────────────────────────────────────────────────

/// TeX Appendix G demote pass — one left-to-right sweep.
///
/// Each noad's **left** neighbour is read from the already-demoted `out`
/// array (so a Bin→Atom demotion immediately affects the Rel/Op that
/// follows), while the **right** neighbour is still the raw `input`.  This is
/// the single place where atom classes are resolved: downstream spacing reads
/// the resulting [`NoadKind`] and never re-applies these rules.
///
/// Neighbours are read via [`Noad::effective_kind`] so structural kinds
/// (`Fence`, `Radical`, `Vcent`, `TableCell`) collapse to `Inner` before
/// the demote rules fire — they participate as an Inner neighbour exactly
/// the way TeX treats fences and radicals for spacing.
fn demote_pass(input: &[Noad]) -> Vec<Noad> {
    let n = input.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        // left = demote result (already in out), or None at the leading edge.
        let left = out.last().map(|n: &Noad| n.effective_kind());
        // right = still the original input; TeX evaluates demotion rules
        // before any neighbour has changed.
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
// Pass 3 — merge consecutive Text atoms only
// ────────────────────────────────────────────────────────────────────────

/// Fold adjacent **Text-derived** Atom noads into a single Atom.
///
/// TeX does **not** merge Symbol atoms even if both sides happen to be
/// Ordinary — each remains a distinct atom with its own glyph and spacing
/// context.  In particular, a Bin demoted to Atom still participates in
/// spacing as a separate Ordinary atom.  But multiple consecutive `Text`
/// fragments (e.g. a word split into several `SemanticNode::Text` during
/// markdown import) should fold into a single atom so the layout engine
/// sees one "word" instead of N single-letter atoms with zero glue between
/// them.
///
/// Two Text atoms are merged only when:
/// * both are `NoadKind::Atom`,
/// * both carry a `SemanticNode::Text` nucleus (never Symbol),
/// * they carry the same `FontStyle` face (a bold run never folds into the
///   plain run before it),
/// * they share the same `MathStyle` and `cramped` flag,
/// * neither carries sub/superscripts (scripts break the run).
fn merge_atoms_pass(input: Vec<Noad>) -> Vec<Noad> {
    let mut out: Vec<Noad> = Vec::with_capacity(input.len());
    for noad in input {
        if let Some(last) = out.last_mut() {
            if last.kind == NoadKind::Atom
                && noad.kind == NoadKind::Atom
                && last.is_text_atom()
                && noad.is_text_atom()
                && last.text_font_style() == noad.text_font_style()
                && last.style == noad.style
                && last.cramped == noad.cramped
                && last.scripts.is_none()
                && noad.scripts.is_none()
            {
                last.nucleus = Some(merge_text_nodes(last.nucleus.as_ref(), noad.nucleus.as_ref()));
                continue;
            }
        }
        out.push(noad);
    }
    out
}

/// Concatenate two Text nuclei into a single `SemanticNode::Text`.
///
/// Only succeeds when **both** are `SemanticNode::Text`; otherwise returns
/// `b` verbatim as a safe fallback (the caller gate keeps Symbol/other
/// nuclei out of this branch, so we only hit the fallback in defensive
/// paths).  `font_style` is taken from `a` — the merge gate already requires
/// both fragments to share the face, so the concatenation keeps it.
fn merge_text_nodes(a: Option<&Box<SemanticNode>>, b: Option<&Box<SemanticNode>>) -> Box<SemanticNode> {
    let a_text = a.and_then(|x| match x.as_ref() {
        SemanticNode::Text { text, font_style } => Some((text.clone(), *font_style)),
        _ => None,
    });
    let b_text = b.and_then(|x| match x.as_ref() {
        SemanticNode::Text { text, .. } => Some(text.clone()),
        _ => None,
    });
    match (a_text, b_text) {
        (Some((ta, fa)), Some(tb)) => Box::new(SemanticNode::Text { text: format!("{ta}{tb}"), font_style: fa }),
        // Defensive fallback: keep b (caller already filtered).
        (_, Some(_)) => b.unwrap().clone(),
        (Some(_), None) => a.unwrap().clone(),
        (None, None) => Box::new(SemanticNode::Text { text: String::new(), font_style: crate::math::FontStyle::Math }),
    }
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
    use crate::math::{GlyphKey, LineStyle, SemanticNode, SymbolRole};

    fn sym(c: char, role: SymbolRole) -> SemanticNode {
        SemanticNode::Symbol { glyph_key: GlyphKey::from_char(c, None, crate::math::FontStyle::Math), role }
    }

    /// Style a projected nucleus/script fragment was stamped with, when the
    /// projection prefixed it with `SetMathStyle`.
    fn stamped_style(node: &SemanticNode) -> Option<MathStyle> {
        match node {
            SemanticNode::Sequence(items) => match items.first() {
                Some(SemanticNode::SetMathStyle(s)) => Some(*s),
                _ => None,
            },
            _ => None,
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
    fn superscript_inherits_cramped_from_parent() {
        // TeX style derivation: a superscript demotes one script level
        // (display/text → script) and keeps the parent's cramped flag, so the
        // superscript of an uncramped formula is not cramped.
        let sup = MathStyleState::DISPLAY.sup();
        assert_eq!(sup.style, MathStyle::Script);
        assert!(!sup.cramped, "superscript inherits the parent cramped flag");

        let cramped_sup = MathStyleState::DISPLAY.cramp().sup();
        assert_eq!(cramped_sup.style, MathStyle::Script);
        assert!(cramped_sup.cramped, "a cramped parent stays cramped in its superscript");
    }

    #[test]
    fn subscript_is_always_cramped() {
        // TeX style derivation: subscripts are cramped whatever the parent is,
        // and demote one script level like superscripts.
        let sub = MathStyleState::DISPLAY.sub();
        assert_eq!(sub.style, MathStyle::Script);
        assert!(sub.cramped, "subscript must be cramped");

        let cramped_sub = MathStyleState::DISPLAY.cramp().sub();
        assert_eq!(cramped_sub.style, MathStyle::Script);
        assert!(cramped_sub.cramped);
    }

    #[test]
    fn fraction_states_step_one_text_level() {
        // TeX fraction derivation: numerator and denominator both step one
        // text level down (display → text → script → scriptscript, then stay),
        // the numerator inherits cramped and the denominator forces it.
        let num = MathStyleState::DISPLAY.frac_num();
        assert_eq!(num.style, MathStyle::Text);
        assert!(!num.cramped);

        let den = MathStyleState::DISPLAY.frac_den();
        assert_eq!(den.style, MathStyle::Text);
        assert!(den.cramped);

        let script = MathStyleState::from_style(MathStyle::Script);
        assert_eq!(script.frac_num().style, MathStyle::ScriptScript);
        assert_eq!(script.frac_den().style, MathStyle::ScriptScript);

        let script_script = MathStyleState::from_style(MathStyle::ScriptScript);
        assert_eq!(script_script.frac_num().style, MathStyle::ScriptScript);
        assert_eq!(script_script.frac_den().style, MathStyle::ScriptScript);

        let cramped = MathStyleState::DISPLAY.cramp();
        assert!(cramped.frac_num().cramped, "numerator inherits the cramped flag");
        assert!(cramped.frac_den().cramped);
    }

    #[test]
    fn fraction_num_and_den_carry_derived_styles() {
        let frac = SemanticNode::Fraction {
            num: Box::new(sym('a', SymbolRole::Ordinary)),
            den: Box::new(sym('b', SymbolRole::Ordinary)),
            line: LineStyle::Solid,
        };
        let list = MathList::from(std::slice::from_ref(&frac));
        assert_eq!(list.len(), 1);
        let n = &list.0[0];
        assert_eq!(n.kind, NoadKind::Inner);
        // The fraction atom itself stays in the outer style, crampedness
        // travels into the numerator/denominator only.
        assert_eq!(n.style, MathStyle::Display);
        assert!(!n.cramped);
        match n.nucleus.as_deref() {
            Some(SemanticNode::Fraction { num, den, .. }) => {
                assert_eq!(stamped_style(num), Some(MathStyle::Text));
                assert_eq!(stamped_style(den), Some(MathStyle::Text));
            }
            other => panic!("expected Fraction nucleus, got {other:?}"),
        }
    }

    #[test]
    fn superscript_subscript_derivation_inside_scripts() {
        // `x^{a_b}`: the outer superscript is script style and keeps the
        // parent's uncramped flag, the nested subscript is cramped at
        // scriptscript style.
        let sup = MathStyleState::DISPLAY.sup();
        assert!(!sup.cramped);
        let nested = sup.sub();
        assert_eq!(nested.style, MathStyle::ScriptScript);
        assert!(nested.cramped, "the nested subscript is cramped");

        let nodes = vec![SemanticNode::Superscript {
            base: Box::new(sym('x', SymbolRole::Ordinary)),
            sup: Box::new(SemanticNode::Subscript {
                base: Box::new(sym('a', SymbolRole::Ordinary)),
                sub: Box::new(sym('b', SymbolRole::Ordinary)),
            }),
        }];
        let list = MathList::from(nodes.as_slice());
        let n = &list.0[0];
        assert_eq!(n.style, MathStyle::Display, "the base keeps the outer style");
        assert!(!n.cramped);
        let sup_node = n.scripts.as_ref().and_then(|s| s.sup.as_deref()).expect("superscript");
        assert_eq!(stamped_style(sup_node), Some(MathStyle::Script));
    }

    #[test]
    fn radical_argument_is_cramped() {
        let rad = SemanticNode::Radical { degree: None, radicand: Box::new(sym('x', SymbolRole::Ordinary)) };
        let list = MathList::from(std::slice::from_ref(&rad));
        assert_eq!(list.0[0].kind, NoadKind::Radical);
        assert_eq!(list.0[0].style, MathStyle::Display);
        assert!(list.0[0].cramped, "a radical's argument is cramped");
    }

    #[test]
    fn sqrt_of_fraction_inherits_cramped_inside() {
        // `\sqrt{\frac{a}{b}}`: the radical marks its argument cramped, and a
        // fraction inside it propagates that crampedness to the numerator and
        // forces it in the denominator.
        let frac = SemanticNode::Fraction {
            num: Box::new(sym('a', SymbolRole::Ordinary)),
            den: Box::new(sym('b', SymbolRole::Ordinary)),
            line: LineStyle::Solid,
        };
        let rad = SemanticNode::Radical { degree: None, radicand: Box::new(frac) };
        let list = MathList::from(std::slice::from_ref(&rad));
        assert!(list.0[0].cramped);

        let cramped = MathStyleState::DISPLAY.cramp();
        assert_eq!(cramped.frac_num().style, MathStyle::Text);
        assert!(cramped.frac_num().cramped, "numerator inside a radical is cramped");
        assert!(cramped.frac_den().cramped);
    }

    #[test]
    fn style_command_is_scoped_to_its_group() {
        // `a {\scriptstyle b} c`: the switch lives inside a braced group, so it
        // must not reach the sibling after the group.
        let nodes = vec![
            sym('a', SymbolRole::Ordinary),
            SemanticNode::Sequence(vec![SemanticNode::SetMathStyle(MathStyle::Script), sym('b', SymbolRole::Ordinary)]),
            sym('c', SymbolRole::Ordinary),
        ];
        let list = MathList::from(nodes.as_slice());
        assert_eq!(list.len(), 3);
        assert_eq!(list.0[0].style, MathStyle::Display);
        assert_eq!(list.0[1].style, MathStyle::Script);
        assert_eq!(list.0[2].style, MathStyle::Display, "style must not leak past the group");
    }

    #[test]
    fn bin_stays_between_atoms() {
        // a + b → Bin between two Atoms must NOT demote.
        let nodes = vec![sym('a', SymbolRole::Ordinary), sym('+', SymbolRole::Binary), sym('b', SymbolRole::Ordinary)];
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
        let nodes = vec![sym('(', SymbolRole::Opening), sym('+', SymbolRole::Binary)];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Open);
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }

    #[test]
    fn bin_demotes_after_close() {
        // ) + → Close + Atom — Bin at the end (no right neighbour) demotes.
        let nodes = vec![sym(')', SymbolRole::Closing), sym('+', SymbolRole::Binary)];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Close);
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }

    #[test]
    fn rel_demotes_after_open() {
        // ( = → Open + Bin — Rel after Open demotes to Bin.
        let nodes = vec![sym('(', SymbolRole::Opening), sym('=', SymbolRole::Relation)];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Open);
        assert_eq!(normalized.0[1].kind, NoadKind::Bin);
    }

    #[test]
    fn op_demotes_before_close() {
        // ∑ ) → Atom + Close — LargeOp (Op) before Close demotes.
        let nodes = vec![sym('∑', SymbolRole::LargeOperator), sym(')', SymbolRole::Closing)];
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
        let nodes = vec![sym('∑', SymbolRole::LargeOperator), sym('x', SymbolRole::Ordinary)];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        // first (no left) → left_strong false; right=Atom → right_strong false.
        // Op does NOT demote.
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized.0[0].kind, NoadKind::Op);
        assert_eq!(normalized.0[1].kind, NoadKind::Atom);
    }

    #[test]
    fn symbol_atoms_never_merge() {
        // Three Ordinary Symbol atoms must remain 3 distinct atoms — TeX
        // keeps each Symbol-derived glyph separate even when both sides
        // happen to be Ordinary.
        let nodes = vec![sym('a', SymbolRole::Ordinary), sym('b', SymbolRole::Ordinary), sym('c', SymbolRole::Ordinary)];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 3, "Symbol atoms must never merge (TeX semantics)");
        assert!(normalized.0.iter().all(|n| !n.is_text_atom()));
    }

    #[test]
    fn text_atoms_do_merge() {
        // Three consecutive Text fragments merge into one Atom whose
        // nucleus carries the concatenated string.
        use crate::math::FontStyle;
        let nodes = vec![
            SemanticNode::Text { text: "a".into(), font_style: FontStyle::Math },
            SemanticNode::Text { text: "b".into(), font_style: FontStyle::Math },
            SemanticNode::Text { text: "c".into(), font_style: FontStyle::Math },
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 1, "three Text atoms must fold into one");
        let n = &normalized.0[0];
        assert!(n.is_text_atom());
        match n.nucleus.as_deref() {
            Some(SemanticNode::Text { text, .. }) => {
                assert_eq!(text, "abc");
            }
            other => panic!("expected Text nucleus, got {other:?}"),
        }
    }

    #[test]
    fn text_atoms_with_a_different_face_stay_separate() {
        // `\text{a}\textbf{b}`: the bold run must not fold into the plain run
        // before it, otherwise the merged atom would be set entirely in the
        // first fragment's face and the bold would disappear.
        use crate::math::FontStyle;
        let nodes = vec![
            SemanticNode::Text { text: "a".into(), font_style: FontStyle::Normal },
            SemanticNode::Text { text: "b".into(), font_style: FontStyle::Bold },
        ];
        let projected = project_semantic(&nodes, MathStyle::Text);
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 2, "a face change must break the Text run");
        assert_eq!(normalized.0[0].text_font_style(), Some(FontStyle::Normal));
        assert_eq!(normalized.0[1].text_font_style(), Some(FontStyle::Bold), "the bold fragment keeps its face");
        match normalized.0[1].nucleus.as_deref() {
            Some(SemanticNode::Text { text, font_style }) => {
                assert_eq!(text, "b");
                assert_eq!(*font_style, FontStyle::Bold);
            }
            other => panic!("expected Text nucleus, got {other:?}"),
        }
    }

    #[test]
    fn same_face_runs_around_a_face_change_still_merge() {
        // `\text{a}\text{b}\textbf{c}\text{d}` folds into three atoms: the
        // normal run, the bold fragment and the trailing normal fragment.
        use crate::math::FontStyle;
        let nodes = vec![
            SemanticNode::Text { text: "a".into(), font_style: FontStyle::Normal },
            SemanticNode::Text { text: "b".into(), font_style: FontStyle::Normal },
            SemanticNode::Text { text: "c".into(), font_style: FontStyle::Bold },
            SemanticNode::Text { text: "d".into(), font_style: FontStyle::Normal },
        ];
        let projected = project_semantic(&nodes, MathStyle::Text);
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized.0[0].text_font_style(), Some(FontStyle::Normal));
        assert_eq!(normalized.0[1].text_font_style(), Some(FontStyle::Bold));
        assert_eq!(normalized.0[2].text_font_style(), Some(FontStyle::Normal));
        match normalized.0[0].nucleus.as_deref() {
            Some(SemanticNode::Text { text, .. }) => assert_eq!(text, "ab"),
            other => panic!("expected Text nucleus, got {other:?}"),
        }
    }

    #[test]
    fn mixed_symbol_text_symbol_stays_three() {
        // Symbol + Text + Symbol — only adjacent Text runs merge; the Text
        // run here is length 1 so no folding happens.  Result stays 3.
        use crate::math::FontStyle;
        let nodes = vec![
            sym('a', SymbolRole::Ordinary),
            SemanticNode::Text { text: "b".into(), font_style: FontStyle::Math },
            sym('c', SymbolRole::Ordinary),
        ];
        let projected = MathList::from(nodes.as_slice());
        let normalized = normalize_math_list(projected);
        assert_eq!(normalized.len(), 3, "Symbol/Text/Symbol mix must stay 3 (only consecutive Text merges)");
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
            SemanticNode::LimitsSpec(LimitsPolicy::Limits),
            sym('∑', SymbolRole::LargeOperator),
            sym('x', SymbolRole::Ordinary),
        ];
        let list = MathList::from(nodes.as_slice());
        assert_eq!(list.len(), 2);
        assert_eq!(list.0[0].kind, NoadKind::Op);
        assert_eq!(list.0[0].limits, Some(LimitsPolicy::Limits));
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
