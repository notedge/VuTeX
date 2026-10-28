//! TeX math subset parser producing [`MathSemanticTree`].
//!
//! This is a formula-level grammar only: it lowers a math span's source text into the
//! semantic nodes the layout engine understands. It is not a document language frontend
//! and does not own any document IR — the surrounding document stays `notedown-ir`.

use vutex_types::{
    Diagnostic, Severity,
    math::{
        Alignment, Fixed, FontStyle, GlyphKey, LineStyle, MathError, MathResult, MathSemanticTree, MathStyle, SemanticNode,
        SymbolRole,
    },
};

/// Parse TeX math source and report the math-layer warnings it raised.
///
/// A warning describes source the grammar accepts but the layout semantics can
/// only partly honour, such as a matrix environment whose body does not match
/// its declared column count. Errors stay fatal and are returned through
/// [`MathError`].
pub(super) fn parse_math_with_diagnostics(source: &str) -> MathResult<(MathSemanticTree, Vec<Diagnostic>)> {
    let mut parser = Parser::new(source);
    let items = parser.parse_list(&[])?;
    parser.skip_whitespace();
    if let Some(c) = parser.peek() {
        return Err(parser.error(format!("unexpected character `{c}`")));
    }
    let diagnostics = std::mem::take(&mut parser.diagnostics);
    Ok((MathSemanticTree { root: sequence(items) }, diagnostics))
}

/// Terminators that a list parse may stop in front of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// `}` of the enclosing group.
    CloseBrace,
    /// `]` of an optional argument such as `\sqrt[...]`.
    CloseBracket,
    /// `\right` of the enclosing `\left...\right` pair.
    Right,
    /// `&` cell separator inside a matrix.
    Cell,
    /// `\\` row separator inside a matrix.
    Row,
    /// `\end{...}` of the enclosing environment.
    EndEnvironment,
}

/// Recursive-descent parser over the formula source.
struct Parser {
    chars: Vec<char>,
    index: usize,
    /// Warnings raised while lowering source the grammar accepts but the
    /// layout semantics can only partly honour.
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(source: &str) -> Self {
        Self { chars: source.chars().collect(), index: 0, diagnostics: Vec::new() }
    }

    fn position(&self) -> usize {
        self.index
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.index + offset).copied()
    }

    fn bump(&mut self) {
        self.index += 1;
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.bump();
        }
    }

    fn error(&self, message: impl Into<String>) -> MathError {
        MathError::parse_error(message, Some(self.position()))
    }

    /// Record a warning for source that parses but cannot be fully honoured.
    fn warn(&mut self, code: &str, message: String) {
        self.diagnostics.push(Diagnostic { code: code.to_string(), severity: Severity::Warning, message });
    }

    /// Parse a list of atoms until one of `stops` is reached (without consuming it).
    fn parse_list(&mut self, stops: &[Stop]) -> MathResult<Vec<SemanticNode>> {
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(c) = self.peek()
            else {
                break;
            };
            if c == '}' {
                if stops.contains(&Stop::CloseBrace) {
                    break;
                }
                return Err(self.error("unexpected `}`"));
            }
            if c == ']' {
                if stops.contains(&Stop::CloseBracket) {
                    break;
                }
                return Err(self.error("unexpected `]`"));
            }
            if c == '&' {
                if stops.contains(&Stop::Cell) {
                    break;
                }
                return Err(self.error("alignment tab `&` is only allowed inside a matrix"));
            }
            if c == '\\' {
                if self.peek_at(1) == Some('\\') {
                    if stops.contains(&Stop::Row) {
                        break;
                    }
                    return Err(self.error("row separator `\\\\` is only allowed inside a matrix"));
                }
                if self.command_is("right") {
                    if stops.contains(&Stop::Right) {
                        break;
                    }
                    return Err(self.error("`\\right` without a matching `\\left`"));
                }
                if self.command_is("end") {
                    if stops.contains(&Stop::EndEnvironment) {
                        break;
                    }
                    return Err(self.error("`\\end` without a matching `\\begin`"));
                }
            }
            if c == '^' || c == '_' {
                return Err(self.error("script without a base"));
            }

            let atom = self.parse_atom()?;
            // `\limits` / `\nolimits` describes the operator in front of it, not
            // an atom of its own.  Emit the flag before the operator's scripts so
            // the layout pass applies it to that operator, and rebind any scripts
            // that follow the flag to the operator instead of to the flag.
            if let SemanticNode::LimitsSpec(flag) = atom {
                let base = items.pop().ok_or_else(|| self.error("`\\limits` must follow an operator"))?;
                // `\limits` only takes effect on a large operator.  Any other
                // atom keeps its scripts beside the base, so report the misuse
                // instead of accepting it silently.
                if !matches!(base, SemanticNode::Symbol { role: SymbolRole::LargeOperator, .. }) {
                    let command = if flag { "\\limits" } else { "\\nolimits" };
                    self.warn(
                        "vutex.layout.math.limits_misuse",
                        format!(
                            "`{command}` applies only to a large operator such as `\\sum`, so this atom keeps its scripts beside the base"
                        ),
                    );
                }
                let scripted = self.parse_scripts(base)?;
                items.push(SemanticNode::LimitsSpec(flag));
                items.push(scripted);
                continue;
            }
            items.push(self.parse_scripts(atom)?);
        }
        Ok(items)
    }

    /// Attach `^` / `_` scripts to `base`.
    fn parse_scripts(&mut self, base: SemanticNode) -> MathResult<SemanticNode> {
        let mut sub = None;
        let mut sup = None;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('^') => {
                    if sup.is_some() {
                        return Err(self.error("double superscript"));
                    }
                    self.bump();
                    sup = Some(Box::new(self.parse_argument()?));
                }
                Some('_') => {
                    if sub.is_some() {
                        return Err(self.error("double subscript"));
                    }
                    self.bump();
                    sub = Some(Box::new(self.parse_argument()?));
                }
                _ => break,
            }
        }

        Ok(match (sub, sup) {
            (None, None) => base,
            (Some(sub), None) => SemanticNode::Subscript { base: Box::new(base), sub },
            (None, Some(sup)) => SemanticNode::Superscript { base: Box::new(base), sup },
            (Some(sub), Some(sup)) => SemanticNode::SubSuperscript { base: Box::new(base), sub, sup },
        })
    }

    /// Parse one unscripted atom.
    fn parse_atom(&mut self) -> MathResult<SemanticNode> {
        self.skip_whitespace();
        let Some(c) = self.peek()
        else {
            return Err(self.error("unexpected end of formula"));
        };
        match c {
            '{' => {
                self.bump();
                let items = self.parse_list(&[Stop::CloseBrace])?;
                self.expect_char('}')?;
                Ok(sequence(items))
            }
            '\\' => self.parse_command(),
            _ => {
                self.bump();
                Ok(char_node(c))
            }
        }
    }

    /// Parse a required argument: a braced group, a command, or a single atom.
    fn parse_argument(&mut self) -> MathResult<SemanticNode> {
        self.skip_whitespace();
        let Some(c) = self.peek()
        else {
            return Err(self.error("missing argument"));
        };
        match c {
            '{' => {
                self.bump();
                let items = self.parse_list(&[Stop::CloseBrace])?;
                self.expect_char('}')?;
                Ok(sequence(items))
            }
            '}' | ']' | '&' | '^' | '_' => Err(self.error("missing argument")),
            '\\' => self.parse_command(),
            _ => {
                self.bump();
                Ok(char_node(c))
            }
        }
    }

    /// Parse a command starting at the current backslash.
    fn parse_command(&mut self) -> MathResult<SemanticNode> {
        self.bump();
        let Some(c) = self.peek()
        else {
            return Err(self.error("stray `\\`"));
        };

        if !c.is_alphabetic() {
            self.bump();
            return match c {
                '{' => Ok(char_node('{')),
                '}' => Ok(char_node('}')),
                '|' => Ok(char_node('‖')),
                // Math spaces are multiples of `mu`, `em / 18`: `\,` is thin
                // (3mu), `\:` medium (4mu), `\;` thick (5mu), and `\ ` the
                // interword space of 6mu.  They are kerns of the current em,
                // not a blank glyph of some font-dependent width.
                ',' => Ok(math_space(3.0)),
                ':' => Ok(math_space(4.0)),
                ';' => Ok(math_space(5.0)),
                ' ' => Ok(math_space(6.0)),
                '!' => Ok(SemanticNode::Sequence(Vec::new())),
                '\\' => Err(self.error("`\\\\` is only allowed inside a matrix")),
                _ => Err(self.error(format!("unknown control symbol `\\{c}`"))),
            };
        }

        let name = self.read_command_name();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.parse_argument()?;
                let den = self.parse_argument()?;
                Ok(SemanticNode::Fraction { num: Box::new(num), den: Box::new(den), line: LineStyle::Solid })
            }
            "binom" => {
                let num = self.parse_argument()?;
                let den = self.parse_argument()?;
                Ok(SemanticNode::Delimited {
                    left: Some(delimiter('(')),
                    right: Some(delimiter(')')),
                    content: Box::new(SemanticNode::Fraction { num: Box::new(num), den: Box::new(den), line: LineStyle::None }),
                })
            }
            "sqrt" => self.parse_sqrt(),
            "left" => self.parse_left(),
            "begin" => self.parse_environment(),
            "operatorname" => {
                let text = self.read_text_group()?;
                Ok(SemanticNode::Text { text, font_style: FontStyle::Normal })
            }
            "text" | "textrm" | "mathrm" => {
                let text = self.read_text_group()?;
                Ok(SemanticNode::Text { text, font_style: FontStyle::Normal })
            }
            "mathit" => {
                let text = self.read_text_group()?;
                Ok(SemanticNode::Text { text, font_style: FontStyle::Italic })
            }
            "mathbf" => {
                let text = self.read_text_group()?;
                Ok(SemanticNode::Text { text, font_style: FontStyle::Bold })
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigm" | "Bigm" | "biggm" | "Biggm" => self.parse_big_delimiter(),
            "hat" | "widehat" => self.parse_accent('ˆ'),
            "bar" | "overline" => self.parse_accent('¯'),
            "vec" => self.parse_accent('→'),
            "tilde" | "widetilde" => self.parse_accent('˜'),
            "dot" => self.parse_accent('˙'),
            "ddot" => self.parse_accent('¨'),
            // `\quad` is 1em and `\qquad` 2em, which TeX expresses as 18mu and
            // 36mu: the same table the thin, medium and thick spaces use.
            "quad" => Ok(math_space(18.0)),
            "qquad" => Ok(math_space(36.0)),
            "displaystyle" => Ok(SemanticNode::SetMathStyle(MathStyle::Display)),
            "textstyle" => Ok(SemanticNode::SetMathStyle(MathStyle::Text)),
            "scriptstyle" => Ok(SemanticNode::SetMathStyle(MathStyle::Script)),
            "scriptscriptstyle" => Ok(SemanticNode::SetMathStyle(MathStyle::ScriptScript)),
            "limits" => Ok(SemanticNode::LimitsSpec(true)),
            "nolimits" => Ok(SemanticNode::LimitsSpec(false)),
            _ => {
                if let Some((ch, role)) = named_symbol(&name) {
                    return Ok(SemanticNode::Symbol { glyph_key: GlyphKey::from_char(ch, None, FontStyle::Math), role });
                }
                if let Some(text) = named_function(&name) {
                    return Ok(SemanticNode::Text { text: text.to_string(), font_style: FontStyle::Normal });
                }
                Err(self.error(format!("unknown command `\\{name}`")))
            }
        }
    }

    fn parse_sqrt(&mut self) -> MathResult<SemanticNode> {
        self.skip_whitespace();
        let degree = if self.peek() == Some('[') {
            self.bump();
            let items = self.parse_list(&[Stop::CloseBracket])?;
            self.expect_char(']')?;
            Some(Box::new(sequence(items)))
        }
        else {
            None
        };
        let radicand = self.parse_argument()?;
        Ok(SemanticNode::Radical { degree, radicand: Box::new(radicand) })
    }

    fn parse_accent(&mut self, accent: char) -> MathResult<SemanticNode> {
        let base = self.parse_argument()?;
        Ok(SemanticNode::Accent { base: Box::new(base), accent: GlyphKey::from_char(accent, None, FontStyle::Normal) })
    }

    fn parse_left(&mut self) -> MathResult<SemanticNode> {
        let left = self.parse_delimiter()?;
        let items = self.parse_list(&[Stop::Right])?;
        self.skip_whitespace();
        self.expect_command("right")?;
        let right = self.parse_delimiter()?;
        Ok(SemanticNode::Delimited { left, right, content: Box::new(sequence(items)) })
    }

    /// Parse a `\big` / `\Big` / `\bigg` / `\Bigg` (and `-m` variants) followed by a delimiter.
    /// Lowers to `Delimited { left: Some(delimiter), right: None, content: Sequence([]) }`.
    fn parse_big_delimiter(&mut self) -> MathResult<SemanticNode> {
        let glyph = self.parse_delimiter()?;
        Ok(SemanticNode::Delimited { left: glyph, right: None, content: Box::new(SemanticNode::Sequence(Vec::new())) })
    }

    /// Read a delimiter after `\left` / `\right`; `.` means “no delimiter”.
    fn parse_delimiter(&mut self) -> MathResult<Option<GlyphKey>> {
        self.skip_whitespace();
        let Some(c) = self.peek()
        else {
            return Err(self.error("expected a delimiter"));
        };

        if c == '.' {
            self.bump();
            return Ok(None);
        }

        if c == '\\' {
            self.bump();
            let Some(next) = self.peek()
            else {
                return Err(self.error("expected a delimiter"));
            };
            if !next.is_alphabetic() {
                self.bump();
                let ch = match next {
                    '{' => '{',
                    '}' => '}',
                    '|' => '‖',
                    other => other,
                };
                return Ok(Some(GlyphKey::from_char(ch, None, FontStyle::Normal)));
            }
            let name = self.read_command_name();
            let ch = match name.as_str() {
                "langle" => '⟨',
                "rangle" => '⟩',
                "vert" => '|',
                "Vert" | "lVert" | "rVert" => '‖',
                other => return Err(self.error(format!("unsupported delimiter `\\{other}`"))),
            };
            return Ok(Some(GlyphKey::from_char(ch, None, FontStyle::Normal)));
        }

        if "()[]{}|<>/".contains(c) {
            self.bump();
            return Ok(Some(GlyphKey::from_char(c, None, FontStyle::Normal)));
        }

        Err(self.error("expected a delimiter"))
    }

    fn parse_environment(&mut self) -> MathResult<SemanticNode> {
        self.skip_whitespace();
        let name = self.read_text_group()?;
        let name = name.trim().to_string();

        let supported = [
            "matrix",
            "pmatrix",
            "bmatrix",
            "vmatrix",
            "cases",
            "aligned",
            "alignedat",
            "alignedat*",
            "align",
            "align*",
            "smallmatrix",
            "gathered",
            "split",
            "Bmatrix",
            "Vmatrix",
        ];
        if !supported.contains(&name.as_str()) {
            return Err(self.error(format!("unsupported environment `{name}`")));
        }

        let column_pairs = match name.as_str() {
            "alignedat" | "alignedat*" => Some(self.read_column_pairs(&name)?),
            _ => None,
        };

        let matrix = self.parse_matrix(&name, column_pairs)?;
        let (left, right) = match name.as_str() {
            "pmatrix" => (Some(delimiter('(')), Some(delimiter(')'))),
            "bmatrix" => (Some(delimiter('[')), Some(delimiter(']'))),
            "Bmatrix" => (Some(delimiter('{')), Some(delimiter('}'))),
            "vmatrix" => (Some(delimiter('|')), Some(delimiter('|'))),
            "Vmatrix" => (Some(delimiter('‖')), Some(delimiter('‖'))),
            "cases" => (Some(delimiter('{')), None),
            _ => (None, None),
        };
        if left.is_none() && right.is_none() {
            return Ok(matrix);
        }
        Ok(SemanticNode::Delimited { left, right, content: Box::new(matrix) })
    }

    fn parse_matrix(&mut self, name: &str, column_pairs: Option<usize>) -> MathResult<SemanticNode> {
        let mut rows = Vec::new();
        loop {
            let mut row = Vec::new();
            loop {
                let items = self.parse_list(&[Stop::Cell, Stop::Row, Stop::EndEnvironment])?;
                row.push(sequence(items));
                self.skip_whitespace();
                if self.peek() == Some('&') {
                    self.bump();
                    continue;
                }
                break;
            }
            rows.push(row);
            self.skip_whitespace();
            if self.peek() == Some('\\') && self.peek_at(1) == Some('\\') {
                self.bump();
                self.bump();
                continue;
            }
            break;
        }

        self.skip_whitespace();
        self.expect_command("end")?;
        self.skip_whitespace();
        let closing = self.read_text_group()?;
        if closing.trim() != name {
            return Err(self.error(format!("expected `\\end{{{name}}}` but found `\\end{{{}}}`", closing.trim())));
        }

        let row_spacing = match name {
            "smallmatrix" => Fixed::from_f64(0.2),
            _ => Fixed::from_f64(0.35),
        };

        // `alignedat{n}` fixes the grid to `n` right/left column pairs.  A body
        // that types fewer cells than the declaration leaves the remaining
        // columns empty, and one that types a different count is reported
        // instead of silently keeping whatever the author happened to write.
        let typed_columns = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        let declared_columns = column_pairs.map(|pairs| pairs * 2);
        if let Some(declared) = declared_columns {
            if declared != typed_columns {
                self.warn(
                    "vutex.layout.math.alignedat_columns",
                    format!(
                        "`\\begin{{{name}}}` declares {} column pairs ({} columns) but its body has {} columns, so the grid keeps {} columns",
                        declared / 2,
                        declared,
                        typed_columns,
                        declared.max(typed_columns)
                    ),
                );
            }
            for row in &mut rows {
                while row.len() < declared {
                    row.push(SemanticNode::Sequence(Vec::new()));
                }
            }
        }
        let columns = declared_columns.unwrap_or(typed_columns).max(typed_columns);
        let column_alignment = match name {
            // `cases` is a left-aligned expression/condition pair.
            "cases" => vec![Alignment::Left; columns],
            // `aligned`, `alignedat`, `split` and `align` alternate right/left
            // at every `&`, so the relation column lines up across rows.
            "aligned" | "alignedat" | "alignedat*" | "split" | "align" | "align*" => {
                (0..columns).map(|column| if column % 2 == 0 { Alignment::Right } else { Alignment::Left }).collect()
            }
            // Everything else is the generic centred matrix, `gathered`
            // included: its rows are centred over each other.
            _ => vec![Alignment::Center; columns],
        };

        // `align` and `align*` are display-level environments: their cells stay
        // at display style even when the formula around them is demoted. The
        // starred and unstarred forms share this geometry, their numbering is a
        // document-level concern.
        // `aligned`, `alignedat`, `split`, `align` and `align*` are rl grids:
        // TeX places each row as `r <relation> l` with a thick muskip on both
        // sides of the relation, and the column gap is the ordinary intercolumn
        // glue of the alignment, which these environments set to nothing.  A
        // blanket `col_spacing` would double the space on one side of the
        // relation, so these grids get zero while every other matrix keeps the
        // `\quad`-ish gutter.
        let col_spacing = if matches!(name, "aligned" | "alignedat" | "alignedat*" | "split" | "align" | "align*") {
            Fixed::ZERO
        }
        else {
            Fixed::from_f64(0.8)
        };
        let display_style = matches!(name, "align" | "align*");
        Ok(SemanticNode::Matrix { rows, row_spacing, col_spacing, display_style, column_alignment })
    }

    /// Read the `{n}` column-pair argument of `alignedat`.
    fn read_column_pairs(&mut self, name: &str) -> MathResult<usize> {
        let raw = self.read_text_group()?;
        let trimmed = raw.trim();
        match trimmed.parse::<usize>() {
            Ok(pairs) if pairs > 0 => Ok(pairs),
            _ => Err(self.error(format!("`\\begin{{{name}}}` expects a positive column-pair count but found `{trimmed}`"))),
        }
    }

    /// Read `{...}` verbatim, keeping nested braces and escaped characters.
    fn read_text_group(&mut self) -> MathResult<String> {
        self.skip_whitespace();
        if self.peek() != Some('{') {
            return Err(self.error("expected `{` after text command"));
        }
        self.bump();

        let mut depth = 1usize;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.bump();
            match c {
                '{' => {
                    depth += 1;
                    out.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(c);
                }
                '\\' => {
                    if let Some(escaped) = self.peek() {
                        out.push(escaped);
                        self.bump();
                    }
                }
                _ => out.push(c),
            }
        }
        Err(self.error("unterminated braced group"))
    }

    fn read_command_name(&mut self) -> String {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphabetic() {
                name.push(c);
                self.bump();
            }
            else {
                break;
            }
        }
        name
    }

    fn command_is(&self, name: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let mut index = self.index + 1;
        for expected in name.chars() {
            if self.chars.get(index) != Some(&expected) {
                return false;
            }
            index += 1;
        }
        !matches!(self.chars.get(index), Some(c) if c.is_alphabetic())
    }

    fn expect_command(&mut self, name: &str) -> MathResult<()> {
        if self.command_is(name) {
            self.index += 1 + name.chars().count();
            Ok(())
        }
        else {
            Err(self.error(format!("expected `\\{name}`")))
        }
    }

    fn expect_char(&mut self, expected: char) -> MathResult<()> {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.bump();
            Ok(())
        }
        else {
            Err(self.error(format!("expected `{expected}`")))
        }
    }
}

/// Collapse a parsed list into a single node.
fn sequence(mut items: Vec<SemanticNode>) -> SemanticNode {
    match items.len() {
        0 => SemanticNode::Sequence(Vec::new()),
        1 => items.remove(0),
        _ => SemanticNode::Sequence(items),
    }
}

/// Build a delimiter glyph key for a literal character.
fn delimiter(c: char) -> GlyphKey {
    GlyphKey::from_char(c, None, FontStyle::Normal)
}

/// Build a math-space node of `mu` units, where `1mu` is `em / 18`.
fn math_space(mu: f64) -> SemanticNode {
    SemanticNode::Space { width: Fixed::from_f64(mu / 18.0) }
}

/// Build an ordinary symbol node for a literal character.
fn char_node(c: char) -> SemanticNode {
    let (role, style) = match c {
        '+' | '-' | '*' | '/' | '×' | '·' => (SymbolRole::Binary, FontStyle::Math),
        '=' | '<' | '>' => (SymbolRole::Relation, FontStyle::Math),
        '0'..='9' => (SymbolRole::Ordinary, FontStyle::Normal),
        '(' | '[' => (SymbolRole::Opening, FontStyle::Normal),
        ')' | ']' => (SymbolRole::Closing, FontStyle::Normal),
        ',' | ';' => (SymbolRole::Punctuation, FontStyle::Normal),
        _ => (SymbolRole::Ordinary, FontStyle::Math),
    };
    SemanticNode::Symbol { glyph_key: GlyphKey::from_char(c, None, style), role }
}

/// Single-character commands that map onto a symbol with a specific role.
fn named_symbol(name: &str) -> Option<(char, SymbolRole)> {
    let symbol = match name {
        "alpha" => ('α', SymbolRole::Ordinary),
        "beta" => ('β', SymbolRole::Ordinary),
        "gamma" => ('γ', SymbolRole::Ordinary),
        "delta" => ('δ', SymbolRole::Ordinary),
        "epsilon" => ('ε', SymbolRole::Ordinary),
        "varepsilon" => ('ϵ', SymbolRole::Ordinary),
        "zeta" => ('ζ', SymbolRole::Ordinary),
        "eta" => ('η', SymbolRole::Ordinary),
        "theta" => ('θ', SymbolRole::Ordinary),
        "vartheta" => ('ϑ', SymbolRole::Ordinary),
        "iota" => ('ι', SymbolRole::Ordinary),
        "kappa" => ('κ', SymbolRole::Ordinary),
        "lambda" => ('λ', SymbolRole::Ordinary),
        "mu" => ('μ', SymbolRole::Ordinary),
        "nu" => ('ν', SymbolRole::Ordinary),
        "xi" => ('ξ', SymbolRole::Ordinary),
        "pi" => ('π', SymbolRole::Ordinary),
        "rho" => ('ρ', SymbolRole::Ordinary),
        "sigma" => ('σ', SymbolRole::Ordinary),
        "varsigma" => ('ς', SymbolRole::Ordinary),
        "tau" => ('τ', SymbolRole::Ordinary),
        "upsilon" => ('υ', SymbolRole::Ordinary),
        "phi" => ('φ', SymbolRole::Ordinary),
        "varphi" => ('ϕ', SymbolRole::Ordinary),
        "chi" => ('χ', SymbolRole::Ordinary),
        "psi" => ('ψ', SymbolRole::Ordinary),
        "omega" => ('ω', SymbolRole::Ordinary),
        "Gamma" => ('Γ', SymbolRole::Ordinary),
        "Delta" => ('Δ', SymbolRole::Ordinary),
        "Theta" => ('Θ', SymbolRole::Ordinary),
        "Lambda" => ('Λ', SymbolRole::Ordinary),
        "Xi" => ('Ξ', SymbolRole::Ordinary),
        "Pi" => ('Π', SymbolRole::Ordinary),
        "Sigma" => ('Σ', SymbolRole::Ordinary),
        "Upsilon" => ('Υ', SymbolRole::Ordinary),
        "Phi" => ('Φ', SymbolRole::Ordinary),
        "Psi" => ('Ψ', SymbolRole::Ordinary),
        "Omega" => ('Ω', SymbolRole::Ordinary),
        "sum" => ('∑', SymbolRole::LargeOperator),
        "prod" => ('∏', SymbolRole::LargeOperator),
        "coprod" => ('∐', SymbolRole::LargeOperator),
        "int" => ('∫', SymbolRole::LargeOperator),
        "iint" => ('∬', SymbolRole::LargeOperator),
        "oint" => ('∮', SymbolRole::LargeOperator),
        "pm" => ('±', SymbolRole::Binary),
        "mp" => ('∓', SymbolRole::Binary),
        "times" => ('×', SymbolRole::Binary),
        "cdot" => ('⋅', SymbolRole::Binary),
        "div" => ('÷', SymbolRole::Binary),
        "ast" => ('∗', SymbolRole::Binary),
        "star" => ('⋆', SymbolRole::Binary),
        "bullet" => ('∙', SymbolRole::Binary),
        "cup" => ('∪', SymbolRole::Binary),
        "cap" => ('∩', SymbolRole::Binary),
        "circ" => ('∘', SymbolRole::Binary),
        "le" | "leq" => ('≤', SymbolRole::Relation),
        "ge" | "geq" => ('≥', SymbolRole::Relation),
        "ne" | "neq" => ('≠', SymbolRole::Relation),
        "approx" => ('≈', SymbolRole::Relation),
        "equiv" => ('≡', SymbolRole::Relation),
        "sim" => ('∼', SymbolRole::Relation),
        "propto" => ('∝', SymbolRole::Relation),
        "to" | "rightarrow" => ('→', SymbolRole::Relation),
        "leftarrow" => ('←', SymbolRole::Relation),
        "leftrightarrow" => ('↔', SymbolRole::Relation),
        "Rightarrow" => ('⇒', SymbolRole::Relation),
        "in" => ('∈', SymbolRole::Relation),
        "notin" => ('∉', SymbolRole::Relation),
        "subset" => ('⊂', SymbolRole::Relation),
        "subseteq" => ('⊆', SymbolRole::Relation),
        "supset" => ('⊃', SymbolRole::Relation),
        "perp" => ('⊥', SymbolRole::Relation),
        "parallel" => ('∥', SymbolRole::Relation),
        "infty" => ('∞', SymbolRole::Ordinary),
        "partial" => ('∂', SymbolRole::Ordinary),
        "nabla" => ('∇', SymbolRole::Ordinary),
        "forall" => ('∀', SymbolRole::Ordinary),
        "exists" => ('∃', SymbolRole::Ordinary),
        "emptyset" => ('∅', SymbolRole::Ordinary),
        "ldots" => ('…', SymbolRole::Ordinary),
        "cdots" => ('⋯', SymbolRole::Ordinary),
        "ddots" => ('⋱', SymbolRole::Ordinary),
        "prime" => ('′', SymbolRole::Ordinary),
        "angle" => ('∠', SymbolRole::Ordinary),
        "degree" => ('°', SymbolRole::Ordinary),
        _ => return None,
    };
    Some(symbol)
}

/// Named operators that render as upright multi-character text.
fn named_function(name: &str) -> Option<&'static str> {
    let text = match name {
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "log" => "log",
        "ln" => "ln",
        "exp" => "exp",
        "max" => "max",
        "min" => "min",
        "lim" => "lim",
        "det" => "det",
        "gcd" => "gcd",
        _ => return None,
    };
    Some(text)
}
