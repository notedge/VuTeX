/// One laid-out page before presentation lowering.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPage {
    pub width: f64,
    pub height: f64,
}

/// Computed layout output consumed by presentation builders.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPlan {
    pub pages: Vec<LayoutPage>,
}
