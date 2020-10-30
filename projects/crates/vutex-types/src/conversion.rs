use serde::{Deserialize, Serialize};

/// Per-stage coverage summary for adapter conversion reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverageSummary {
    /// Number of source constructs considered.
    pub considered: u32,
    /// Number successfully handled at this stage.
    pub handled: u32,
}

/// A preserved source artifact or reversible mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreservedItem {
    pub code: String,
    pub detail: String,
}

/// A known but unsupported construct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsupportedItem {
    pub code: String,
    pub detail: String,
}

/// Content kept without full semantic lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpaqueItem {
    pub code: String,
    pub detail: String,
}

/// A lossy or approximate lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LossyItem {
    pub code: String,
    pub detail: String,
}

/// Structured conversion report shared by all language adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversionReport {
    pub parsed: CoverageSummary,
    pub lowered: CoverageSummary,
    pub preserved: Vec<PreservedItem>,
    pub unsupported: Vec<UnsupportedItem>,
    pub opaque: Vec<OpaqueItem>,
    pub lossy: Vec<LossyItem>,
}
