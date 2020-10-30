use vutex_layout::LayoutPlan;

use crate::display::{DisplayPage, DisplayPresentation};

/// Lower a layout plan into a display presentation.
pub fn build_display_presentation(plan: &LayoutPlan) -> DisplayPresentation {
    DisplayPresentation {
        pages: plan.pages.iter().map(|page| DisplayPage { width: page.width, height: page.height }).collect(),
    }
}
