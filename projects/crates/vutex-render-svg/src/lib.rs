#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

// pub mod math; // commented out: vutex_layout::math is not available
pub mod render;

pub use render::render_svg;
