#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod document;
pub mod node;

pub use document::Document;
pub use node::{BlockKind, DocumentId, InlineKind, NodeId};
