#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod builder;
pub mod display;
pub mod flow;

pub use builder::{build_display_presentation, build_flow_presentation};
pub use display::{DisplayItem, DisplayPage, DisplayPresentation};
pub use flow::{FlowBlock, FlowPresentation, FlowTableRow};
