use vutex_document::Document;
use vutex_types::ConversionReport;

/// Project notedown input into the canonical document IR.
pub fn project_notedown(_source: &str) -> (Document, ConversionReport) {
    (
        Document { root: vutex_document::NodeId(0) },
        ConversionReport {
            parsed: vutex_types::CoverageSummary { considered: 0, handled: 0 },
            lowered: vutex_types::CoverageSummary { considered: 0, handled: 0 },
            preserved: Vec::new(),
            unsupported: Vec::new(),
            opaque: Vec::new(),
            lossy: Vec::new(),
        },
    )
}
