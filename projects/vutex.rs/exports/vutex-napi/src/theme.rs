//! The zero-configuration typography baseline and the explicit legacy theme.
//!
//! Two themes exist so a host never has to tune typography to get a readable
//! page:
//!
//! * [`Theme::Baseline`] is the default. Body prose measures and paints with
//!   the bundled Noto Serif SC faces. The Latin outlines in that file descend
//!   from Source Serif, so Latin and Han share one design instead of pairing a
//!   Times clone with a CJK face built for another type system. Margins open to
//!   100 pt and the line box grows to 1.5 em, which lands the measure in the
//!   66-72 characters a reader can track without a guide.
//! * [`Theme::Traditional`] keeps the measured Tinos faces, 72 pt margins and
//!   the 1.45 line box. It exists for documents whose pagination must not move,
//!   and it is never selected implicitly.
//!
//! Both themes embed only bundled faces. No theme falls back to a host CJK
//! face, so SimSun, SimHei, and Microsoft YaHei never paint a VuTeX page.

use document_fonts::{BodyFaceKey, BodyFaceStyle, BodyFamily, BodyMetrics};
use vutex_layout::hint::Margin;
use vutex_layout::LayoutHint;

use crate::assets::{body_faces, body_metrics};

/// Theme id selected by default.
pub const BASELINE_ID: &str = "baseline";
/// Theme id for the explicit legacy selection.
pub const TRADITIONAL_ID: &str = "traditional";

/// Typography preset a render request selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Bundled Noto Serif SC for Latin and Han, 100 pt side margins, 1.5 line box.
    #[default]
    Baseline,
    /// Tinos for Latin, bundled Noto Serif SC for Han, 72 pt margins, 1.45 line box.
    Traditional,
}

impl Theme {
    /// Resolve a theme id; unknown and missing values select the baseline.
    pub fn from_option(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some(TRADITIONAL_ID) => Self::Traditional,
            _ => Self::Baseline,
        }
    }

    /// Stable id written into reports.
    pub fn id(self) -> &'static str {
        match self {
            Self::Baseline => BASELINE_ID,
            Self::Traditional => TRADITIONAL_ID,
        }
    }

    /// Body text size in points.
    pub fn base_font_size(self) -> f64 {
        11.0
    }

    /// Line box as a multiple of the body size.
    pub fn line_height(self) -> f64 {
        match self {
            Self::Baseline => 1.5,
            Self::Traditional => 1.45,
        }
    }

    /// Layout intent for this theme.
    pub fn layout_hint(self) -> LayoutHint {
        let mut hint = LayoutHint::a4();
        hint.base_font_size = self.base_font_size();
        hint.line_height = self.line_height();
        if self == Self::Baseline {
            hint.margin = Margin { top: 90.0, right: 100.0, bottom: 90.0, left: 100.0 };
        }
        hint
    }

    /// A heading run is bold when its painted size exceeds the body size by
    /// more than this threshold, in points.
    pub fn heading_weight_threshold(self) -> f64 {
        self.base_font_size() * 1.15
    }

    /// Metrics used to break lines and reserve advances.
    pub fn body_metrics(self) -> BodyMeasurements {
        match self {
            Self::Traditional => BodyMeasurements::Bundled,
            Self::Baseline => BodyMeasurements::Owned(BodyMetrics::from_faces(
                [
                    (BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular }, "Noto Serif SC Regular"),
                    (BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Bold }, "Noto Serif SC Bold"),
                    (BodyFaceKey { family: BodyFamily::Mono, style: BodyFaceStyle::Regular }, "Noto Sans Mono Regular"),
                ]
                .into_iter()
                .filter_map(|(key, label)| {
                    face_for(key).map(|face| (key, label.to_string(), face.bytes.as_ref()))
                }),
            )),
        }
    }
}

/// Bundled face that serves one key under the baseline theme.
///
/// The CJK Noto file carries the Latin outlines too, so the same file answers a
/// prose key whether the character is Latin or Han. A bold run without a bold
/// face for its request falls back to the regular file, matching the way CSS
/// synthesizes an upright bold when only one weight is installed.
fn face_for(key: BodyFaceKey) -> Option<&'static document_fonts::BodyFace> {
    body_faces()
        .iter()
        .find(|face| match (key.family, key.style) {
            (BodyFamily::Serif, BodyFaceStyle::Regular) => {
                face.family == BodyFamily::Serif
                    && face.script == document_fonts::BodyScript::Cjk
                    && face.style == BodyFaceStyle::Regular
            }
            (BodyFamily::Serif, BodyFaceStyle::Bold) => {
                face.family == BodyFamily::Serif
                    && face.script == document_fonts::BodyScript::Cjk
                    && face.style == BodyFaceStyle::Bold
            }
            (BodyFamily::Serif, BodyFaceStyle::Italic) => {
                face.family == BodyFamily::Serif
                    && face.script == document_fonts::BodyScript::Cjk
                    && face.style == BodyFaceStyle::Regular
            }
            (BodyFamily::Mono, _) => {
                face.family == BodyFamily::Mono && face.style == BodyFaceStyle::Regular
            }
        })
}

/// Borrowed or owned body metrics.
pub enum BodyMeasurements {
    /// The bundled Tinos plus Noto set.
    Bundled,
    /// A face set built from bundled files in a different order.
    Owned(BodyMetrics),
}

impl BodyMeasurements {
    /// Borrow the metrics for a layout pass.
    pub fn get(&self) -> &BodyMetrics {
        match self {
            Self::Bundled => body_metrics(),
            Self::Owned(metrics) => metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_theme_ids_select_the_baseline() {
        assert_eq!(Theme::from_option(None), Theme::Baseline);
        assert_eq!(Theme::from_option(Some("")), Theme::Baseline);
        assert_eq!(Theme::from_option(Some("bogus")), Theme::Baseline);
        assert_eq!(Theme::from_option(Some("traditional")), Theme::Traditional);
    }

    #[test]
    fn baseline_opens_the_measure_and_the_line_box() {
        let hint = Theme::Baseline.layout_hint();
        assert!(hint.content_width() < Theme::Traditional.layout_hint().content_width());
        assert!(hint.line_height > Theme::Traditional.layout_hint().line_height);
    }

    #[test]
    fn baseline_metrics_cover_latin_and_han_with_bundled_faces() {
        let metrics = Theme::Baseline.body_metrics();
        let regular = BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular };
        assert!(metrics.get().advance_em(regular, 'A').is_some());
        assert!(metrics.get().advance_em(regular, '中').is_some());
        let mono = BodyFaceKey { family: BodyFamily::Mono, style: BodyFaceStyle::Regular };
        assert!(metrics.get().advance_em(mono, 'M').is_some());
    }
}
