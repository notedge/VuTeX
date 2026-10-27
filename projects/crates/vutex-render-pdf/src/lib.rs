#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod font;
mod image;
pub mod render;
mod resources;
mod scene;
mod svg;

pub use render::{
    PdfFontPolicy, PdfRenderOutcome, render_pdf, render_pdf_with_diagnostics, render_pdf_with_font_policy,
    render_pdf_with_font_policy_and_diagnostics, render_pdf_with_resources, render_pdf_with_resources_and_diagnostics,
};
pub use resources::PdfResources;
