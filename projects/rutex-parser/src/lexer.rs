use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\n\f]+")] // Skip whitespace
pub enum Token {
    #[token("{")]
    LBrace,

    #[token("}")]
    RBrace,

    #[token("[")]
    LBracket,

    #[token("]")]
    RBracket,

    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    #[token("^")]
    Caret,

    #[token("_")]
    Underscore,

    #[regex(r"#[0-9]", |lex| lex.slice()[1..].parse::<usize>().unwrap())]
    Parameter(usize),

    #[token("#")]
    Hash,

    #[token("&")]
    Ampersand,

    #[token(r"\\")]
    Backslash,

    #[regex(r"\\[a-zA-Z]+", |lex| lex.slice()[1..].to_string())]
    Command(String),

    #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().to_string())]
    Number(String),

    #[regex(r"[a-zA-Z]", |lex| lex.slice().to_string())]
    Letter(String),

    #[regex(r"[\+\-\*/=<>!|]", |lex| lex.slice().to_string())]
    Operator(String),

    #[token(",")]
    Comma,

    #[token(".")]
    Period,

    #[token(":")]
    Colon,

    #[token(";")]
    Semicolon,

    #[regex(r"[\u{00A0}-\u{D7FF}\u{F900}-\u{FDCF}\u{FDF0}-\u{FFEF}]", |lex| lex.slice().to_string())]
    Unicode(String),
}

pub struct Tokenizer<'a> {
    lexer: logos::Lexer<'a, Token>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            lexer: Token::lexer(input),
        }
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = std::result::Result<Token, ()>;

    fn next(&mut self) -> Option<Self::Item> {
        self.lexer.next()
    }
}
