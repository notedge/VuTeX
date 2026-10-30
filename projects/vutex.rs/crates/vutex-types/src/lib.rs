#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod conversion;
pub mod diagnostic;

pub use conversion::{ConversionReport, CoverageSummary, LossyItem, OpaqueItem, PreservedItem, UnsupportedItem};
pub use diagnostic::{Diagnostic, Severity};

pub mod math;
