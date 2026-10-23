pub mod lexer;
pub mod parser;
pub mod macro_system;

pub use rutex_types::{RuTeXError, Result, MathSemanticTree, SemanticNode};
pub use parser::Parser;
pub use macro_system::{ParseContext, ParseEffect};

pub fn parse(input: &str) -> Result<MathSemanticTree> {
    let mut parser = Parser::new(input);
    parser.parse()
}
