//! TeX math subset parser producing [`MathSemanticTree`].
//!
//! This is a formula-level grammar only: it lowers a math span's source text into the
//! semantic nodes the layout engine understands. It is not a document language frontend
//! and does not own any document IR — the surrounding document stays `notedown-ir`.

use vutex_types::math::{
    Alignment, Fixed, FontStyle, GlyphKey, LineStyle, MathError, MathResult, MathSemanticTree, SemanticNode, SymbolRole,
};

/// Parse TeX math source into the semantic tree consumed by the layout engine.
pub(super) fn parse_math(source: &str) -> MathResult<MathSemanticTree> {
    let mut parser = Parser::new(source);
    let items = parser.parse_list(&[])?;
    parser.skip_whitespace();
    if let Some(c) = parser.peek() {
        return Err(parser.error(format!("unexpected character `{c}`")));
    }
    Ok(MathSemanticTree { root: sequence(items) })
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
}

impl Parser {
    fn new(source: &str) -> Self {
        Self { chars: source.chars().collect(), index: 0 }
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
                ',' | ':' | ';' | ' ' => Ok(SemanticNode::Text(" ".to_string())),
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
                    content: Box::new(SemanticNode::Fraction {
                        num: Box::new(num),
                        den: Box::new(den),
                        line: LineStyle::None,
                    }),
                })
            }
            "sqrt" => self.parse_sqrt(),
            "left" => self.parse_left(),
            "begin" => self.parse_environment(),
            "operatorname" => {
                let text = self.read_text_group()?;
                Ok(SemanticNode::Text(text))
            }
            "text" | "textrm" | "mathrm" | "mathit" | "mathbf" => {
                let text = self.read_text_group()?;
                Ok(SemanticNode::Text(text))
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigm" | "Bigm" | "biggm" | "Biggm" => self.parse_big_delimiter(),
            "hat" | "widehat" => self.parse_accent('ˆ'),
            "bar" | "overline" => self.parse_accent('¯'),
            "vec" => self.parse_accent('→'),
            "tilde" | "widetilde" => self.parse_accent('˜'),
            "dot" => self.parse_accent('˙'),
            "ddot" => self.parse_accent('¨'),
            "quad" => Ok(SemanticNode::Text("  ".to_string())),
            "qquad" => Ok(SemanticNode::Text("    ".to_string())),
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" | "limits" | "nolimits" => {
                Ok(SemanticNode::Sequence(Vec::new()))
            }
            _ => {
                if let Some((ch, role)) = named_symbol(&name) {
                    return Ok(SemanticNode::Symbol { glyph_key: GlyphKey::from_char(ch, None, FontStyle::Math), role });
                }
                if let Some(text) = named_function(&name) {
                    return Ok(SemanticNode::Text(text.to_string()));
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
        Ok(SemanticNode::Delimited {
            left: glyph,
            right: None,
            content: Box::new(SemanticNode::Sequence(Vec::new())),
        })
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
            "matrix", "pmatrix", "bmatrix", "vmatrix", "cases", "aligned",
            "smallmatrix", "gathered", "split", "Bmatrix", "Vmatrix",
        ];
        if !supported.contains(&name.as_str()) {
            return Err(self.error(format!("unsupported environment `{name}`")));
        }

        let matrix = self.parse_matrix(&name)?;
        let (left, right) = match name.as_str() {
            "pmatrix" => (Some(delimiter('(')), Some(delimiter(')'))),
            "bmatrix" => (Some(delimiter('[')), Some(delimiter(']'))),
            "Bmatrix" => (Some(delimiter('{')), Some(delimiter('}'))),
            "vmatrix" | "Vmatrix" => (Some(delimiter('|')), Some(delimiter('|'))),
            "cases" => (Some(delimiter('{')), None),
            _ => (None, None),
        };
        if left.is_none() && right.is_none() {
            return Ok(matrix);
        }
        Ok(SemanticNode::Delimited { left, right, content: Box::new(matrix) })
    }

    fn parse_matrix(&mut self, name: &str) -> MathResult<SemanticNode> {
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
        let alignment = match name {
            "split" => Alignment::Left,
            _ => Alignment::Center,
        };

        Ok(SemanticNode::Matrix {
            rows,
            row_spacing,
            col_spacing: Fixed::from_f64(0.8),
            alignment,
        })
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
