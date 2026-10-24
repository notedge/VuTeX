#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod diagram;
pub mod document;
pub mod hint;
pub mod math;
pub mod plan;

pub use diagram::{DiagramScene, Diagrams};
pub use document::{layout, layout_with_diagrams};
pub use hint::LayoutHint;
pub use plan::{Graphic, LayoutPage, LayoutPlan};
