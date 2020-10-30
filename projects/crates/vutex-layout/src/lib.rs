#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod hint;
pub mod plan;

pub use hint::LayoutHint;
pub use plan::{LayoutPage, LayoutPlan};
