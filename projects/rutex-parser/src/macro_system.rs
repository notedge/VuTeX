use im::HashMap;
use rutex_types::{MathStyle, Result, RuTeXError};
use crate::lexer::Token;

#[derive(Clone, Debug)]
pub struct MacroDefinition {
    pub name: String,
    pub args_count: usize,
    pub body: Vec<Token>,
}

#[derive(Clone, Debug)]
pub struct ParseContext {
    pub macros: HashMap<String, MacroDefinition>,
    pub math_style: MathStyle,
    pub environment_stack: Vec<String>,
}

impl Default for ParseContext {
    fn default() -> Self {
        let macros = HashMap::new();
        
        // Add some default macros if needed
        // For now, keep it empty or add basic ones
        
        Self {
            macros,
            math_style: MathStyle::Display,
            environment_stack: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ParseEffect {
    DefineMacro(MacroDefinition),
    StyleChange(MathStyle),
    BeginEnv(String),
    EndEnv,
}

impl ParseContext {
    pub fn apply_effect(&self, effect: ParseEffect) -> Result<Self> {
        let mut new_ctx = self.clone();
        match effect {
            ParseEffect::DefineMacro(def) => {
                new_ctx.macros.insert(def.name.clone(), def);
            }
            ParseEffect::StyleChange(style) => {
                new_ctx.math_style = style;
            }
            ParseEffect::BeginEnv(env) => {
                new_ctx.environment_stack.push(env);
            }
            ParseEffect::EndEnv => {
                if new_ctx.environment_stack.pop().is_none() {
                    return Err(RuTeXError::parse_error("Unexpected \\end", None));
                }
            }
        }
        Ok(new_ctx)
    }

    pub fn get_macro(&self, name: &str) -> Option<&MacroDefinition> {
        self.macros.get(name)
    }
}
