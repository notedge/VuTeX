use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MathErrorKind {
    ParseError {
        message: String,
        position: Option<usize>,
    },
    LayoutError(String),
    FontError {
        glyph: String,
        message: String,
    },
    BackendError(String),
    InternalError(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MathError {
    pub kind: Box<MathErrorKind>,
}

impl MathError {
    pub fn new(kind: MathErrorKind) -> Self {
        Self { kind: Box::new(kind) }
    }

    pub fn parse_error(message: impl Into<String>, position: Option<usize>) -> Self {
        Self::new(MathErrorKind::ParseError { message: message.into(), position })
    }

    pub fn layout_error(message: impl Into<String>) -> Self {
        Self::new(MathErrorKind::LayoutError(message.into()))
    }

    pub fn font_error(glyph: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(MathErrorKind::FontError { glyph: glyph.into(), message: message.into() })
    }

    pub fn backend_error(message: impl Into<String>) -> Self {
        Self::new(MathErrorKind::BackendError(message.into()))
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(MathErrorKind::InternalError(message.into()))
    }
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.kind {
            MathErrorKind::ParseError { message, position } => {
                if let Some(pos) = position {
                    write!(f, "Parse Error at position {}: {}", pos, message)
                } else {
                    write!(f, "Parse Error: {}", message)
                }
            }
            MathErrorKind::LayoutError(msg) => write!(f, "Layout Error: {}", msg),
            MathErrorKind::FontError { glyph, message } => {
                write!(f, "Font Error (Glyph: {}): {}", glyph, message)
            }
            MathErrorKind::BackendError(msg) => write!(f, "Backend Error: {}", msg),
            MathErrorKind::InternalError(msg) => write!(f, "Internal Error: {}", msg),
        }
    }
}

impl std::error::Error for MathError {}

pub type MathResult<T> = std::result::Result<T, MathError>;
