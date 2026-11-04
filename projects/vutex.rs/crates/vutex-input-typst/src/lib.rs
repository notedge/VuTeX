#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod adapter;
mod report;

pub use adapter::{TypstDocument, TypstError, TypstRender, import_path, import_source, render_svg};
pub use report::{IMPORT_FAILED, MATH_SOURCE_PRESERVED, UNTERMINATED_GROUP};
