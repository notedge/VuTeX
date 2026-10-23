#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod document;
pub mod hint;
// pub mod math; // migrate-rutex.py scaffold — Result unwrap bugs (91) — re-enable after fix
pub mod plan;

pub use document::layout;
pub use hint::LayoutHint;
pub use plan::{LayoutPage, LayoutPlan};
