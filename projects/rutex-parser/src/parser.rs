use crate::lexer::{Token, Tokenizer};
use crate::macro_system::{ParseContext, MacroDefinition, ParseEffect};
use rutex_types::{
    MathSemanticTree, SemanticNode, Result, RuTeXError, 
    GlyphKey, FontStyle, SymbolRole, Alignment, LineStyle,
    SpacingRule
};

pub struct Parser<'a> {
    tokenizer: Tokenizer<'a>,
    peeked: Option<Token>,
    expanded_tokens: Vec<Token>,
    context: ParseContext,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            tokenizer: Tokenizer::new(input),
            peeked: None,
            expanded_tokens: Vec::new(),
            context: ParseContext::default(),
        }
    }

    pub fn with_context(input: &'a str, context: ParseContext) -> Self {
        Self {
            tokenizer: Tokenizer::new(input),
            peeked: None,
            expanded_tokens: Vec::new(),
            context,
        }
    }

    pub fn parse(&mut self) -> Result<MathSemanticTree> {
        let root = self.parse_sequence()?;
        Ok(MathSemanticTree { root })
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        if let Some(token) = self.expanded_tokens.pop() {
            return Ok(Some(token));
        }
        if let Some(token) = self.peeked.take() {
            return Ok(Some(token));
        }
        match self.tokenizer.next() {
            Some(Ok(token)) => Ok(Some(token)),
            Some(Err(_)) => Err(RuTeXError::parse_error("Invalid token", None)),
            None => Ok(None),
        }
    }

    fn peek_token(&mut self) -> Result<Option<&Token>> {
        if self.expanded_tokens.is_empty() && self.peeked.is_none() {
            self.peeked = self.next_token()?;
        }
        
        if let Some(token) = self.expanded_tokens.last() {
            Ok(Some(token))
        } else {
            Ok(self.peeked.as_ref())
        }
    }

    fn parse_sequence(&mut self) -> Result<SemanticNode> {
        let mut nodes = Vec::new();
        while let Some(token) = self.peek_token()? {
            match token {
                Token::RBrace | Token::RBracket | Token::RParen => break,
                Token::Command(name) if name == "end" => break,
                _ => {
                    let node = self.parse_node()?;
                    nodes.push(node);
                }
            }
        }

        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(SemanticNode::Sequence(nodes))
        }
    }

    fn parse_node(&mut self) -> Result<SemanticNode> {
        let mut node = self.parse_primary()?;

        // Handle sub/superscripts
        loop {
            match self.peek_token()? {
                Some(Token::Caret) => {
                    self.next_token()?; // consume ^
                    let sup = self.parse_group()?;
                    node = match node {
                        SemanticNode::Subscript { base, sub } => {
                            SemanticNode::SubSuperscript { base, sub, sup: Box::new(sup) }
                        }
                        _ => SemanticNode::Superscript { base: Box::new(node), sup: Box::new(sup) },
                    };
                }
                Some(Token::Underscore) => {
                    self.next_token()?; // consume _
                    let sub = self.parse_group()?;
                    node = match node {
                        SemanticNode::Superscript { base, sup } => {
                            SemanticNode::SubSuperscript { base, sub: Box::new(sub), sup }
                        }
                        _ => SemanticNode::Subscript { base: Box::new(node), sub: Box::new(sub) },
                    };
                }
                _ => break,
            }
        }

        Ok(node)
    }

    fn parse_primary(&mut self) -> Result<SemanticNode> {
        let token = self.next_token()?.ok_or(RuTeXError::parse_error("Unexpected end of input", None))?;

        match token {
            Token::LBrace => {
                let node = self.parse_sequence()?;
                self.expect(Token::RBrace)?;
                Ok(node)
            }
            Token::Letter(c) => {
                let char = c.chars().next().unwrap();
                Ok(SemanticNode::Symbol {
                    glyph_key: GlyphKey::from_char(char, None, FontStyle::Math),
                    role: SymbolRole::Ordinary,
                })
            }
            Token::Number(n) => Ok(SemanticNode::Text(n)),
            Token::Command(name) => self.handle_command(&name),
            Token::Operator(op) => {
                let char = op.chars().next().unwrap();
                Ok(SemanticNode::Symbol {
                    glyph_key: GlyphKey::from_char(char, None, FontStyle::Normal),
                    role: self.infer_role(char),
                })
            }
            _ => Err(RuTeXError::parse_error(format!("Unexpected token: {:?}", token), None)),
        }
    }

    fn parse_group(&mut self) -> Result<SemanticNode> {
        match self.peek_token()? {
            Some(Token::LBrace) => {
                self.next_token()?;
                let node = self.parse_sequence()?;
                self.expect(Token::RBrace)?;
                Ok(node)
            }
            _ => self.parse_primary(),
        }
    }

    fn handle_command(&mut self, name: &str) -> Result<SemanticNode> {
        // First check if it's a macro
        if let Some(def) = self.context.get_macro(name).cloned() {
            let mut args = Vec::new();
            for _ in 0..def.args_count {
                args.push(self.parse_argument_tokens()?);
            }
            self.expand_macro(def, args);
            return self.parse_node();
        }

        // Built-in commands
        match name {
            "frac" => {
                let num = self.parse_group()?;
                let den = self.parse_group()?;
                Ok(SemanticNode::Fraction {
                    num: Box::new(num),
                    den: Box::new(den),
                    line: LineStyle::Solid,
                })
            }
            "sqrt" => {
                let degree = if let Some(Token::LBracket) = self.peek_token()? {
                    self.next_token()?;
                    let d = self.parse_sequence()?;
                    self.expect(Token::RBracket)?;
                    Some(Box::new(d))
                } else {
                    None
                };
                let radicand = self.parse_group()?;
                Ok(SemanticNode::Radical {
                    degree,
                    radicand: Box::new(radicand),
                })
            }
            "sum" | "prod" | "int" | "oint" | "bigcap" | "bigcup" => {
                let c = match name {
                    "sum" => '∑',
                    "prod" => '∏',
                    "int" => '∫',
                    "oint" => '∮',
                    "bigcap" => '⋂',
                    "bigcup" => '⋃',
                    _ => unreachable!(),
                };
                Ok(SemanticNode::Symbol {
                    glyph_key: GlyphKey::from_char(c, None, FontStyle::Normal),
                    role: SymbolRole::LargeOperator,
                })
            }
            "," | ":" | ";" | "!" | "quad" | "qquad" => {
                let spacing = match name {
                    "," => rutex_types::SpacingRule::Thin,
                    ":" => rutex_types::SpacingRule::Medium,
                    ";" => rutex_types::SpacingRule::Thick,
                    "!" => rutex_types::SpacingRule::Thin, // Negative thin, but for now just thin
                    "quad" => rutex_types::SpacingRule::Medium, // Approx
                    "qquad" => rutex_types::SpacingRule::Thick, // Approx
                    _ => rutex_types::SpacingRule::None,
                };
                Ok(SemanticNode::HorizontalBox {
                    content: vec![],
                    spacing,
                })
            }
            "displaystyle" => {
                self.context.math_style = rutex_types::MathStyle::Display;
                Ok(SemanticNode::Sequence(vec![]))
            }
            "textstyle" => {
                self.context.math_style = rutex_types::MathStyle::Text;
                Ok(SemanticNode::Sequence(vec![]))
            }
            "scriptstyle" => {
                self.context.math_style = rutex_types::MathStyle::Script;
                Ok(SemanticNode::Sequence(vec![]))
            }
            "scriptscriptstyle" => {
                self.context.math_style = rutex_types::MathStyle::ScriptScript;
                Ok(SemanticNode::Sequence(vec![]))
            }
            "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" => {
                let style = match name {
                    "mathrm" => FontStyle::Normal,
                    "mathbf" => FontStyle::Bold,
                    "mathit" => FontStyle::Italic,
                    "mathsf" => FontStyle::SansSerif,
                    "mathtt" => FontStyle::Monospace,
                    _ => FontStyle::Normal,
                };
                let node = self.parse_group()?;
                Ok(self.apply_font_style(node, style)?)
            }
            "left" => {
                let left_token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected delimiter after \\left", None))?;
                let left_char = match left_token {
                    Token::Operator(ref s) => Some(s.chars().next().unwrap()),
                    Token::Period => Some('.'),
                    Token::LBrace => Some('{'),
                    Token::RBrace => Some('}'),
                    _ => None,
                };
                
                let content = self.parse_sequence()?;
                
                self.expect_command("right")?;
                let right_token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected delimiter after \\right", None))?;
                let right_char = match right_token {
                    Token::Operator(ref s) => Some(s.chars().next().unwrap()),
                    Token::Period => Some('.'),
                    Token::LBrace => Some('{'),
                    Token::RBrace => Some('}'),
                    _ => None,
                };

                Ok(SemanticNode::Delimited {
                    left: left_char.and_then(|c| if c == '.' { None } else { Some(GlyphKey::from_char(c, None, FontStyle::Normal)) }),
                    right: right_char.and_then(|c| if c == '.' { None } else { Some(GlyphKey::from_char(c, None, FontStyle::Normal)) }),
                    content: Box::new(content),
                })
            }
            "limits" | "nolimits" => {
                // For now, just skip these
                Ok(SemanticNode::Sequence(vec![]))
            }
            "hat" | "bar" | "tilde" | "dot" | "ddot" | "vec" | "acute" | "grave" | "check" | "breve" => {
                let accent_char = match name {
                    "hat" => '^',
                    "bar" => '¯',
                    "tilde" => '~',
                    "dot" => '˙',
                    "ddot" => '¨',
                    "vec" => '→',
                    "acute" => '´',
                    "grave" => '`',
                    "check" => 'ˇ',
                    "breve" => '˘',
                    _ => ' ',
                };
                let base = self.parse_group()?;
                Ok(SemanticNode::Accent {
                    base: Box::new(base),
                    accent: GlyphKey::from_char(accent_char, None, FontStyle::Normal),
                })
            }
            "thinspace" | "negthinspace" => {
                let spacing = match name {
                    "thinspace" => SpacingRule::Thin,
                    "negthinspace" => SpacingRule::None, // Should be negative thin
                    _ => SpacingRule::Auto,
                };
                Ok(SemanticNode::HorizontalBox {
                    content: vec![],
                    spacing,
                })
            }
            "begin" => {
                let env_name = self.parse_braced_string()?;
                self.handle_environment(&env_name)
            }
            "end" => {
                Err(RuTeXError::parse_error("Unexpected \\end without \\begin", None))
            }
            "def" => {
                let macro_token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected macro name after \\def", None))?;
                let macro_name = match macro_token {
                    Token::Command(name) => name,
                    _ => return Err(RuTeXError::parse_error("Expected command after \\def", None)),
                };
                
                let mut args_count = 0;
                while let Some(t) = self.peek_token()? {
                    match t {
                        Token::Parameter(n) => {
                            let n_val = *n;
                            self.next_token()?;
                            args_count = args_count.max(n_val);
                        }
                        Token::LBrace => break,
                        _ => {
                            self.next_token()?;
                        }
                    }
                }

                let body = self.parse_argument_tokens()?;
                
                let effect = ParseEffect::DefineMacro(MacroDefinition {
                    name: macro_name,
                    args_count,
                    body,
                });
                self.context = self.context.apply_effect(effect)?;
                
                Ok(SemanticNode::Sequence(vec![]))
            }
            "newcommand" => {
                // \newcommand{\name}[args]{body}
                let name_token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected macro name after \\newcommand", None))?;
                
                let macro_name = match name_token {
                    Token::LBrace => {
                        let name = self.parse_command_name()?;
                        self.expect(Token::RBrace)?;
                        name
                    }
                    Token::Command(name) => name,
                    _ => return Err(RuTeXError::parse_error("Expected command or {\\command} after \\newcommand", None)),
                };

                let mut args_count = 0;
                if let Some(Token::LBracket) = self.peek_token()? {
                    self.next_token()?;
                    let count_str = match self.next_token()? {
                        Some(Token::Number(n)) => n,
                        _ => return Err(RuTeXError::parse_error("Expected number of arguments in []", None)),
                    };
                    args_count = count_str.parse().map_err(|_| RuTeXError::parse_error("Invalid argument count", None))?;
                    self.expect(Token::RBracket)?;
                }

                let body = self.parse_argument_tokens()?;

                let effect = ParseEffect::DefineMacro(MacroDefinition {
                    name: macro_name,
                    args_count,
                    body,
                });
                self.context = self.context.apply_effect(effect)?;

                Ok(SemanticNode::Sequence(vec![]))
            }
            _ => {
                // Check if it's a known symbol
                if let Some(c) = self.get_symbol_char(name) {
                    Ok(SemanticNode::Symbol {
                        glyph_key: GlyphKey::from_char(c, None, FontStyle::Normal),
                        role: self.infer_role(c),
                    })
                } else {
                    Err(RuTeXError::parse_error(format!("Unknown command: \\{}", name), None))
                }
            }
        }
    }

    fn get_symbol_char(&self, name: &str) -> Option<char> {
        match name {
            // Lowercase Greek
            "alpha" => Some('α'),
            "beta" => Some('β'),
            "gamma" => Some('γ'),
            "delta" => Some('δ'),
            "epsilon" => Some('ϵ'),
            "varepsilon" => Some('ε'),
            "zeta" => Some('ζ'),
            "eta" => Some('η'),
            "theta" => Some('θ'),
            "vartheta" => Some('ϑ'),
            "iota" => Some('ι'),
            "kappa" => Some('κ'),
            "lambda" => Some('λ'),
            "mu" => Some('μ'),
            "nu" => Some('ν'),
            "xi" => Some('ξ'),
            "pi" => Some('π'),
            "varpi" => Some('ϖ'),
            "rho" => Some('ρ'),
            "varrho" => Some('ϱ'),
            "sigma" => Some('σ'),
            "varsigma" => Some('ς'),
            "tau" => Some('τ'),
            "upsilon" => Some('υ'),
            "phi" => Some('ϕ'),
            "varphi" => Some('φ'),
            "chi" => Some('χ'),
            "psi" => Some('ψ'),
            "omega" => Some('ω'),

            // Uppercase Greek
            "Gamma" => Some('Γ'),
            "Delta" => Some('Δ'),
            "Theta" => Some('Θ'),
            "Lambda" => Some('Λ'),
            "Xi" => Some('Ξ'),
            "Pi" => Some('Π'),
            "Sigma" => Some('Σ'),
            "Upsilon" => Some('Υ'),
            "Phi" => Some('Φ'),
            "Psi" => Some('Ψ'),
            "Omega" => Some('Ω'),

            // Binary Operators
            "pm" => Some('±'),
            "mp" => Some('∓'),
            "times" => Some('×'),
            "div" => Some('÷'),
            "ast" => Some('∗'),
            "star" => Some('⋆'),
            "circ" => Some('∘'),
            "bullet" => Some('•'),
            "cdot" => Some('⋅'),
            "cap" => Some('∩'),
            "cup" => Some('∪'),
            "uplus" => Some('⊎'),
            "sqcap" => Some('⊓'),
            "sqcup" => Some('⊔'),
            "vee" | "lor" => Some('∨'),
            "wedge" | "land" => Some('∧'),
            "setminus" => Some('∖'),
            "wr" => Some('≀'),
            "diamond" => Some('⋄'),
            "bigtriangleup" => Some('△'),
            "bigtriangledown" => Some('▽'),
            "triangleleft" => Some('⊲'),
            "triangleright" => Some('⊳'),
            "oplus" => Some('⊕'),
            "ominus" => Some('⊖'),
            "otimes" => Some('⊗'),
            "oslash" => Some('⊘'),
            "odot" => Some('⊙'),
            "bigcirc" => Some('◯'),
            "dagger" => Some('†'),
            "ddagger" => Some('‡'),
            "amalg" => Some('⨿'),

            // Relations
            "le" | "leq" => Some('≤'),
            "ge" | "geq" => Some('≥'),
            "neq" | "ne" => Some('≠'),
            "approx" => Some('≈'),
            "equiv" => Some('≡'),
            "sim" => Some('∼'),
            "simeq" => Some('≃'),
            "cong" => Some('≅'),
            "asymp" => Some('≍'),
            "propto" => Some('∝'),
            "subset" => Some('⊂'),
            "supset" => Some('⊃'),
            "subseteq" => Some('⊆'),
            "supseteq" => Some('⊇'),
            "in" => Some('∈'),
            "ni" | "owns" => Some('∋'),
            "notin" => Some('∉'),
            "perp" => Some('⊥'),
            "parallel" => Some('∥'),
            "mid" => Some('∣'),
            "smile" => Some('⌣'),
            "frown" => Some('⌢'),
            "bowtie" => Some('⋈'),
            "prec" => Some('≺'),
            "succ" => Some('≻'),
            "preceq" => Some('≼'),
            "succeq" => Some('≽'),

            // Arrows
            "leftarrow" | "gets" => Some('←'),
            "rightarrow" | "to" => Some('→'),
            "uparrow" => Some('↑'),
            "downarrow" => Some('↓'),
            "leftrightarrow" => Some('↔'),
            "updownarrow" => Some('↕'),
            "Leftarrow" => Some('⇐'),
            "Rightarrow" => Some('⇒'),
            "Uparrow" => Some('⇑'),
            "Downarrow" => Some('⇓'),
            "Leftrightarrow" => Some('⇔'),
            "Updownarrow" => Some('⇕'),
            "nearrow" => Some('↗'),
            "searrow" => Some('↘'),
            "swarrow" => Some('↙'),
            "nwarrow" => Some('↖'),
            "leftharpoonup" => Some('↼'),
            "leftharpoondown" => Some('↽'),
            "rightharpoonup" => Some('⇀'),
            "rightharpoondown" => Some('⇁'),

            // Miscellaneous
            "infty" => Some('∞'),
            "forall" => Some('∀'),
            "exists" => Some('∃'),
            "nabla" => Some('∇'),
            "partial" => Some('∂'),
            "wp" => Some('℘'),
            "Re" => Some('ℜ'),
            "Im" => Some('ℑ'),
            "aleph" => Some('ℵ'),
            "emptyset" => Some('∅'),
            "top" => Some('⊤'),
            "bot" => Some('⊥'),
            "angle" => Some('∠'),
            "triangle" => Some('△'),
            "backslash" => Some('\\'),
            "surd" => Some('√'),
            "prime" => Some('′'),
            "flat" => Some('♭'),
            "natural" => Some('♮'),
            "sharp" => Some('♯'),
            "clubsuit" => Some('♣'),
            "diamondsuit" => Some('♢'),
            "heartsuit" => Some('♡'),
            "spadesuit" => Some('♠'),
            "neg" | "lnot" => Some('¬'),

            // Big Operators
            "sum" => Some('∑'),
            "prod" => Some('∏'),
            "int" => Some('∫'),
            "coprod" => Some('∐'),
            "bigcap" => Some('⋂'),
            "bigcup" => Some('⋃'),
            "bigsqcup" => Some('⨆'),
            "bigvee" => Some('⋁'),
            "bigwedge" => Some('⋀'),
            "bigodot" => Some('⨀'),
            "bigoplus" => Some('⨁'),
            "bigotimes" => Some('⨂'),
            "biguplus" => Some('⨄'),
            
            // Spacing/Dots
            "cdots" => Some('⋯'),
            "ldots" => Some('…'),
            "vdots" => Some('⋮'),
            "ddots" => Some('⋱'),
            
            _ => None,
        }
    }

    fn handle_environment(&mut self, name: &str) -> Result<SemanticNode> {
        self.context.environment_stack.push(name.to_string());
        
        let node = match name {
            "matrix" | "pmatrix" | "bmatrix" | "vmatrix" | "Vmatrix" | "Bmatrix" => {
                self.parse_matrix(name)?
            }
            _ => {
                let content = self.parse_sequence()?;
                self.expect_command("end")?;
                let end_name = self.parse_braced_string()?;
                if name != end_name {
                    return Err(RuTeXError::parse_error(format!("Environment mismatch: \\begin{{{}}} but \\end{{{}}}", name, end_name), None));
                }
                SemanticNode::VerticalBox {
                    content: vec![content],
                    alignment: Alignment::Center,
                }
            }
        };

        self.context.environment_stack.pop();
        Ok(node)
    }

    fn parse_matrix(&mut self, env_name: &str) -> Result<SemanticNode> {
        let mut rows = Vec::new();
        let mut current_row = Vec::new();
        
        loop {
            // Parse a cell
            let cell = self.parse_matrix_cell()?;
            current_row.push(cell);
            
            match self.peek_token()? {
                Some(Token::Ampersand) => {
                    self.next_token()?; // consume &
                }
                Some(Token::Backslash) => {
                    self.next_token()?; // consume \\
                    rows.push(std::mem::take(&mut current_row));
                }
                Some(Token::Command(name)) if name == "end" => {
                    if !current_row.is_empty() {
                        rows.push(current_row);
                    }
                    break;
                }
                _ => {
                    if !current_row.is_empty() {
                        rows.push(current_row);
                    }
                    break;
                }
            }
        }
        
        self.expect_command("end")?;
        let end_name = self.parse_braced_string()?;
        if env_name != end_name {
            return Err(RuTeXError::parse_error(format!("Environment mismatch: begin{{{}}} but end{{{}}}", env_name, end_name), None));
        }
        
        let matrix = SemanticNode::Matrix {
            rows,
            row_spacing: rutex_types::Fixed::from_f64(1.2),
            col_spacing: rutex_types::Fixed::from_f64(1.0),
            alignment: Alignment::Center,
        };

        // Handle delimiters for pmatrix, bmatrix, etc.
        let (left, right) = match env_name {
            "pmatrix" => (Some('('), Some(')')),
            "bmatrix" => (Some('['), Some(']')),
            "vmatrix" => (Some('|'), Some('|')),
            "Vmatrix" => (Some('‖'), Some('‖')), // Double vertical bar
            "Bmatrix" => (Some('{'), Some('}')),
            _ => (None, None),
        };

        if left.is_some() || right.is_some() {
            Ok(SemanticNode::Delimited {
                left: left.map(|c| GlyphKey::from_char(c, None, FontStyle::Normal)),
                right: right.map(|c| GlyphKey::from_char(c, None, FontStyle::Normal)),
                content: Box::new(matrix),
            })
        } else {
            Ok(matrix)
        }
    }

    fn parse_matrix_cell(&mut self) -> Result<SemanticNode> {
        let mut nodes = Vec::new();
        while let Some(token) = self.peek_token()? {
            match token {
                Token::Ampersand | Token::Backslash => break,
                Token::Command(name) if name == "end" => break,
                _ => {
                    nodes.push(self.parse_node()?);
                }
            }
        }
        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(SemanticNode::Sequence(nodes))
        }
    }

    fn parse_argument_tokens(&mut self) -> Result<Vec<Token>> {
        let token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected argument", None))?;

        match token {
            Token::LBrace => {
                let mut tokens = Vec::new();
                let mut brace_level = 1;
                while brace_level > 0 {
                    let t = self.next_token()?.ok_or(RuTeXError::parse_error("Unexpected end of input in macro argument", None))?;
                    match t {
                        Token::LBrace => brace_level += 1,
                        Token::RBrace => brace_level -= 1,
                        _ => {}
                    }
                    if brace_level > 0 {
                        tokens.push(t);
                    }
                }
                Ok(tokens)
            }
            _ => Ok(vec![token]),
        }
    }

    fn expand_macro(&mut self, def: crate::macro_system::MacroDefinition, args: Vec<Vec<Token>>) {
        let mut expanded = Vec::new();
        for token in def.body.iter().rev() {
            match token {
                Token::Parameter(idx) => {
                    if *idx > 0 && *idx <= args.len() {
                        for arg_token in args[idx - 1].iter().rev() {
                            expanded.push(arg_token.clone());
                        }
                    }
                }
                _ => expanded.push(token.clone()),
            }
        }
        self.expanded_tokens.extend(expanded);
    }

    fn parse_command_name(&mut self) -> Result<String> {
        let token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected command name", None))?;
        match token {
            Token::Command(name) => Ok(name),
            _ => Err(RuTeXError::parse_error(format!("Expected command, found {:?}", token), None)),
        }
    }

    fn parse_braced_string(&mut self) -> Result<String> {
        self.expect(Token::LBrace)?;
        let mut result = String::new();
        loop {
            let token = self.next_token()?.ok_or(RuTeXError::parse_error("Expected braced string", None))?;
            match token {
                Token::RBrace => break,
                Token::Letter(s) | Token::Number(s) | Token::Operator(s) => result.push_str(&s),
                _ => return Err(RuTeXError::parse_error(format!("Unexpected token in braced string: {:?}", token), None)),
            }
        }
        Ok(result)
    }

    fn expect(&mut self, expected: Token) -> Result<()> {
        let token = self.next_token()?.ok_or(RuTeXError::parse_error(format!("Expected {:?}, found end of input", expected), None))?;

        if token == expected {
            Ok(())
        } else {
            Err(RuTeXError::parse_error(format!("Expected {:?}, found {:?}", expected, token), None))
        }
    }

    fn expect_command(&mut self, expected_name: &str) -> Result<()> {
        let token = self.next_token()?.ok_or(RuTeXError::parse_error(format!("Expected command \\{}, found end of input", expected_name), None))?;

        match token {
            Token::Command(name) if name == expected_name => Ok(()),
            _ => Err(RuTeXError::parse_error(format!("Expected command \\{}, found {:?}", expected_name, token), None)),
        }
    }

    fn apply_font_style(&self, mut node: SemanticNode, style: FontStyle) -> Result<SemanticNode> {
        match &mut node {
            SemanticNode::Symbol { glyph_key, .. } => {
                glyph_key.style = style;
            }
            SemanticNode::Sequence(nodes) | SemanticNode::HorizontalBox { content: nodes, .. } | SemanticNode::VerticalBox { content: nodes, .. } => {
                for n in nodes {
                    *n = self.apply_font_style(n.clone(), style)?;
                }
            }
            SemanticNode::Fraction { num, den, .. } => {
                *num = Box::new(self.apply_font_style((**num).clone(), style)?);
                *den = Box::new(self.apply_font_style((**den).clone(), style)?);
            }
            SemanticNode::Radical { degree, radicand } => {
                if let Some(d) = degree {
                    *d = Box::new(self.apply_font_style((**d).clone(), style)?);
                }
                *radicand = Box::new(self.apply_font_style((**radicand).clone(), style)?);
            }
            SemanticNode::Subscript { base, sub } => {
                *base = Box::new(self.apply_font_style((**base).clone(), style)?);
                *sub = Box::new(self.apply_font_style((**sub).clone(), style)?);
            }
            SemanticNode::Superscript { base, sup } => {
                *base = Box::new(self.apply_font_style((**base).clone(), style)?);
                *sup = Box::new(self.apply_font_style((**sup).clone(), style)?);
            }
            SemanticNode::SubSuperscript { base, sub, sup } => {
                *base = Box::new(self.apply_font_style((**base).clone(), style)?);
                *sub = Box::new(self.apply_font_style((**sub).clone(), style)?);
                *sup = Box::new(self.apply_font_style((**sup).clone(), style)?);
            }
            SemanticNode::Delimited { left, right, content } => {
                if let Some(l) = left {
                    l.style = style;
                }
                if let Some(r) = right {
                    r.style = style;
                }
                *content = Box::new(self.apply_font_style((**content).clone(), style)?);
            }
            SemanticNode::Accent { base, accent } => {
                *base = Box::new(self.apply_font_style((**base).clone(), style)?);
                accent.style = style;
            }
            _ => {}
        }
        Ok(node)
    }

    fn infer_role(&self, c: char) -> SymbolRole {
        match c {
            '+' | '-' | '*' | '/' | '±' | '∓' | '×' | '÷' | '⋅' | '∗' | '⋆' | '∘' | '•' | '∩' | '∪' | '⊎' | '⊓' | '⊔' | '∨' | '∧' | '∖' | '≀' | '⋄' | '⊕' | '⊖' | '⊗' | '⊘' | '⊙' => SymbolRole::Binary,
            '=' | '<' | '>' | '≤' | '≥' | '≠' | '≈' | '≡' | '∼' | '≃' | '≅' | '≍' | '∝' | '⊂' | '⊃' | '⊆' | '⊇' | '∈' | '∋' | '∉' | '⊥' | '∥' | '∣' | '⌣' | '⌢' | '⋈' | '≺' | '≻' | '≼' | '≽' => SymbolRole::Relation,
            '(' | '[' | '{' | '⊲' => SymbolRole::Opening,
            ')' | ']' | '}' | '⊳' => SymbolRole::Closing,
            ',' | '.' | ':' | ';' => SymbolRole::Punctuation,
            '∑' | '∏' | '∫' | '∐' | '⋂' | '⋃' | '⨆' | '⋁' | '⋀' | '⨀' | '⨁' | '⨂' | '⨄' => SymbolRole::LargeOperator,
            _ => SymbolRole::Ordinary,
        }
    }
}
