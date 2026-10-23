#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod backend;
pub mod href;

pub use backend::RenderBackend;
pub use href::is_safe_href;
