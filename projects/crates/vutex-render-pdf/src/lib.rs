#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod font;
pub mod render;
mod scene;

pub use render::{PdfFontPolicy, render_pdf, render_pdf_with_font_policy};
