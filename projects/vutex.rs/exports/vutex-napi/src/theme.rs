//! The zero-configuration typography baseline and the explicit preset themes.
//!
//! Four themes exist so a host never has to tune typography to get a readable
//! page:
//!
//! * [`Theme::Baseline`] is the default and paints as the modern preset. Body
//!   prose measures and paints with the configured Noto Serif SC faces, margins
//!   open to 88/100/96/100 pt, the line box grows to 1.72 em, paragraphs take
//!   8 pt of air, headings are truly bold, and tables keep only three light
//!   rules with tabular figures.
//! * [`Theme::Traditional`] keeps the measured Tinos faces, the A4 reference
//!   margins and the 1.45 line box. It exists for documents whose pagination
//!   must not move, and it is never selected implicitly.
//! * [`Theme::Editorial`] is a magazine face with a larger display size, a
//!   brick accent rule under H1, warm table and code tints, and 11.5 pt body.
//! * [`Theme::Classic`] is a Chinese book face with centred headings, a two
//!   character first-line indent, no paragraph gap, and no coloured accent.
//!
//! Every theme measures and paints from the configured face set only. No theme
//! falls back to a host CJK face, so SimSun, SimHei, and Microsoft YaHei never
//! paint a VuTeX page.

use document_fonts::{BodyFaceKey, BodyFaceStyle, BodyFamily, BodyMetrics};
use vutex_layout::{
    LayoutHint,
    hint::{Align, DEFAULT_MARGIN, Margin, StyleTokens},
};

use crate::assets::{body_faces, body_metrics};

/// Theme id selected by default.
pub const BASELINE_ID: &str = "baseline";
/// Theme id for the explicit legacy selection.
pub const TRADITIONAL_ID: &str = "traditional";
/// Theme id for the explicit magazine preset.
pub const EDITORIAL_ID: &str = "editorial";
/// Theme id for the explicit book preset.
pub const CLASSIC_ID: &str = "classic";

/// Typography preset a render request selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Modern scholarly default: configured Noto Serif SC for Latin and Han.
    #[default]
    Baseline,
    /// Tinos for Latin, configured Noto Serif SC for Han, reference pagination.
    Traditional,
    /// Magazine preset with a warm tint and a brick accent rule.
    Editorial,
    /// Chinese book preset with centred headings and an indented first line.
    Classic,
}

impl Theme {
    /// Resolve a theme id; unknown and missing values select the baseline.
    pub fn from_option(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some(TRADITIONAL_ID) => Self::Traditional,
            Some(EDITORIAL_ID) => Self::Editorial,
            Some(CLASSIC_ID) => Self::Classic,
            _ => Self::Baseline,
        }
    }

    /// Stable id written into reports.
    pub fn id(self) -> &'static str {
        match self {
            Self::Baseline => BASELINE_ID,
            Self::Traditional => TRADITIONAL_ID,
            Self::Editorial => EDITORIAL_ID,
            Self::Classic => CLASSIC_ID,
        }
    }

    /// Body text size in points.
    pub fn base_font_size(self) -> f64 {
        match self {
            Self::Baseline => 11.0,
            Self::Traditional => 11.0,
            Self::Editorial => 11.5,
            Self::Classic => 10.5,
        }
    }

    /// Line box as a multiple of the body size.
    pub fn line_height(self) -> f64 {
        match self {
            Self::Baseline => 1.72,
            Self::Traditional => 1.45,
            Self::Editorial => 1.65,
            Self::Classic => 1.60,
        }
    }

    /// Measurable style tokens this theme hands to the layout pass.
    pub fn style(self) -> StyleTokens {
        match self {
            Self::Baseline => modern_style(),
            Self::Traditional => StyleTokens::legacy(),
            Self::Editorial => editorial_style(),
            Self::Classic => classic_style(),
        }
    }

    /// Layout intent for this theme.
    pub fn layout_hint(self) -> LayoutHint {
        let mut hint = LayoutHint::a4();
        hint.base_font_size = self.base_font_size();
        hint.line_height = self.line_height();
        hint.style = self.style();
        let margin = match self {
            Self::Baseline => Margin { top: 88.0, right: 100.0, bottom: 96.0, left: 100.0 },
            Self::Traditional => Margin::uniform(DEFAULT_MARGIN),
            Self::Editorial => Margin { top: 92.0, right: 92.0, bottom: 100.0, left: 92.0 },
            Self::Classic => Margin { top: 96.0, right: 108.0, bottom: 96.0, left: 108.0 },
        };
        hint.margin = margin;
        hint
    }

    /// A heading run is bold when its painted size exceeds the body size by
    /// more than this threshold, in points.
    ///
    /// Only the legacy traditional preset consumes this: its layout pass does
    /// not mark headings bold, so the SVG font pass promotes them by size.
    pub fn heading_weight_threshold(self) -> f64 {
        self.base_font_size() * 1.15
    }

    /// Metrics used to break lines and reserve advances.
    pub fn body_metrics(self) -> BodyMeasurements {
        match self {
            Self::Traditional => BodyMeasurements::Bundled,
            Self::Baseline | Self::Editorial | Self::Classic => BodyMeasurements::Owned(BodyMetrics::from_faces(
                [
                    (BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular }, "Noto Serif SC Regular"),
                    (BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Bold }, "Noto Serif SC Bold"),
                    (BodyFaceKey { family: BodyFamily::Mono, style: BodyFaceStyle::Regular }, "Noto Sans Mono Regular"),
                ]
                .into_iter()
                .filter_map(|(key, label)| face_for(key).map(|face| (key, label.to_string(), face.bytes.as_ref()))),
            )),
        }
    }
}

/// Modern scholarly defaults: the zero-configuration face of VuTeX.
fn modern_style() -> StyleTokens {
    StyleTokens {
        paragraph_gap: 8.0,
        paragraph_indent: 0.0,
        heading_scales: [1.60, 1.35, 1.18, 1.10],
        heading_gap_before: [0.0, 24.0, 16.0, 12.0],
        heading_gap_after: [20.0, 8.0, 4.0, 4.0],
        heading_bold: true,
        heading_rule: 0.75,
        heading_rule_color: "#C7C7C7",
        heading_rule_width: 0.0,
        heading_align: Align::Left,
        table_header_rule: 0.75,
        table_row_rule: 0.5,
        table_final_rule: 0.75,
        table_rule_strong_color: "#C7C7C7",
        table_rule_color: "#EEEEEC",
        table_header_tint: None,
        table_pad_h: 8.0,
        table_pad_v: 4.0,
        code_size: 10.0,
        code_line_height: 1.45,
        code_padding: 12.0,
        code_margin: 12.0,
        code_fill: "#F7F7F4",
        code_border: 0.5,
        code_border_color: "#E3E3E0",
        quote_rule: 2.5,
        quote_indent: 12.0,
        quote_text_color: "#4A4A4A",
        quote_rule_color: "#C7C7C7",
        list_marker: "–",
        list_marker_gap: 8.0,
        list_item_gap: 4.0,
        list_gap_before: 0.0,
        list_indent: 20.0,
        ink: "#1A1A1A",
        ink_secondary: "#4A4A4A",
        ink_muted: "#737373",
        ink_faint: "#9A9A9A",
        accent: "#2B5B84",
        rule_strong: "#C7C7C7",
        rule: "#E3E3E0",
        rule_faint: "#EEEEEC",
        tint: "#F7F7F4",
        footnote_gap: 12.0,
        footnote_rule: 0.5,
        image_max_width: 0.0,
        chrome: true,
        header_size: 8.5,
        header_color: "#737373",
        header_align: Align::Right,
        folio_size: 9.0,
        folio_color: "#737373",
        chrome_top: 40.0,
        chrome_bottom: 48.0,
    }
}

/// Magazine preset: larger display type, brick accent, warm tints.
fn editorial_style() -> StyleTokens {
    StyleTokens {
        paragraph_gap: 8.0,
        paragraph_indent: 0.0,
        heading_scales: [2.00, 1.50, 1.25, 1.12],
        heading_gap_before: [0.0, 24.0, 16.0, 12.0],
        heading_gap_after: [20.0, 8.0, 4.0, 4.0],
        heading_bold: true,
        heading_rule: 2.0,
        heading_rule_color: "#B3402F",
        heading_rule_width: 56.0,
        heading_align: Align::Left,
        table_header_rule: 0.75,
        table_row_rule: 0.5,
        table_final_rule: 0.75,
        table_rule_strong_color: "#C9C2B6",
        table_rule_color: "#EDE8DF",
        table_header_tint: Some("#F5F1E9"),
        table_pad_h: 8.0,
        table_pad_v: 4.0,
        code_size: 10.0,
        code_line_height: 1.45,
        code_padding: 12.0,
        code_margin: 12.0,
        code_fill: "#F5F1E9",
        code_border: 0.5,
        code_border_color: "#E0DACF",
        quote_rule: 2.5,
        quote_indent: 12.0,
        quote_text_color: "#45403A",
        quote_rule_color: "#C9C2B6",
        list_marker: "–",
        list_marker_gap: 8.0,
        list_item_gap: 4.0,
        list_gap_before: 0.0,
        list_indent: 20.0,
        ink: "#1C1A18",
        ink_secondary: "#45403A",
        ink_muted: "#7C756C",
        ink_faint: "#A79F94",
        accent: "#B3402F",
        rule_strong: "#C9C2B6",
        rule: "#E0DACF",
        rule_faint: "#EDE8DF",
        tint: "#F5F1E9",
        footnote_gap: 12.0,
        footnote_rule: 0.5,
        image_max_width: 0.0,
        chrome: true,
        header_size: 8.5,
        header_color: "#7C756C",
        header_align: Align::Left,
        folio_size: 9.0,
        folio_color: "#7C756C",
        chrome_top: 40.0,
        chrome_bottom: 48.0,
    }
}

/// Chinese book preset: centred headings, indented first line, no accent.
fn classic_style() -> StyleTokens {
    StyleTokens {
        paragraph_gap: 0.0,
        paragraph_indent: 21.0,
        heading_scales: [1.60, 1.30, 1.20, 1.12],
        heading_gap_before: [0.0, 24.0, 16.0, 12.0],
        heading_gap_after: [20.0, 8.0, 4.0, 4.0],
        heading_bold: true,
        heading_rule: 0.5,
        heading_rule_color: "#444444",
        heading_rule_width: 0.0,
        heading_align: Align::Center,
        table_header_rule: 0.75,
        table_row_rule: 0.0,
        table_final_rule: 0.75,
        table_rule_strong_color: "#444444",
        table_rule_color: "#BBBBBB",
        table_header_tint: None,
        table_pad_h: 8.0,
        table_pad_v: 4.0,
        code_size: 10.0,
        code_line_height: 1.45,
        code_padding: 12.0,
        code_margin: 12.0,
        code_fill: "#F4F4F2",
        code_border: 0.0,
        code_border_color: "#DDDDDD",
        quote_rule: 0.0,
        quote_indent: 21.0,
        quote_text_color: "#2B2B2B",
        quote_rule_color: "#BBBBBB",
        list_marker: "–",
        list_marker_gap: 8.0,
        list_item_gap: 4.0,
        list_gap_before: 0.0,
        list_indent: 21.0,
        ink: "#111111",
        ink_secondary: "#2B2B2B",
        ink_muted: "#555555",
        ink_faint: "#8A8A8A",
        accent: "#111111",
        rule_strong: "#444444",
        rule: "#BBBBBB",
        rule_faint: "#DDDDDD",
        tint: "#F4F4F2",
        footnote_gap: 12.0,
        footnote_rule: 0.5,
        image_max_width: 0.0,
        chrome: true,
        header_size: 8.5,
        header_color: "#555555",
        header_align: Align::Center,
        folio_size: 9.0,
        folio_color: "#555555",
        chrome_top: 40.0,
        chrome_bottom: 48.0,
    }
}

/// Bundled face that serves one key under the baseline theme.
///
/// The CJK Noto file carries the Latin outlines too, so the same file answers a
/// prose key whether the character is Latin or Han. A bold run without a bold
/// face for its request falls back to the regular file, matching the way CSS
/// synthesizes an upright bold when only one weight is installed.
fn face_for(key: BodyFaceKey) -> Option<&'static document_fonts::BodyFace> {
    body_faces().iter().find(|face| match (key.family, key.style) {
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
        (BodyFamily::Mono, _) => face.family == BodyFamily::Mono && face.style == BodyFaceStyle::Regular,
    })
}

/// Borrowed or owned body metrics.
pub enum BodyMeasurements {
    /// The configured Tinos plus Noto set.
    Bundled,
    /// A face set built from configured files in a different order.
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
    fn explicit_theme_ids_select_their_presets() {
        assert_eq!(Theme::from_option(Some("editorial")), Theme::Editorial);
        assert_eq!(Theme::from_option(Some("classic")), Theme::Classic);
        assert_eq!(Theme::Editorial.id(), "editorial");
        assert_eq!(Theme::Classic.id(), "classic");
        assert_eq!(Theme::Baseline.id(), "baseline");
    }

    #[test]
    fn baseline_opens_the_measure_and_the_line_box() {
        let hint = Theme::Baseline.layout_hint();
        assert!(hint.content_width() < Theme::Traditional.layout_hint().content_width());
        assert!(hint.line_height > Theme::Traditional.layout_hint().line_height);
    }

    #[test]
    fn traditional_keeps_its_reference_pagination() {
        let hint = Theme::Traditional.layout_hint();
        assert_eq!(hint.base_font_size, 11.0);
        assert_eq!(hint.line_height, 1.45);
        assert_eq!(hint.margin.top, 72.0);
        assert_eq!(hint.margin.right, 72.0);
        assert_eq!(hint.margin.bottom, 72.0);
        assert_eq!(hint.margin.left, 72.0);
        assert!((hint.content_width() - 451.276).abs() < 1e-9);
        assert_eq!(hint.style.paragraph_gap, 0.0);
        assert!(!hint.style.heading_bold);
        assert!(!hint.style.chrome);
    }

    #[test]
    fn baseline_metrics_cover_latin_and_han_with_configured_faces() {
        crate::assets::configure_test_fonts();
        let metrics = Theme::Baseline.body_metrics();
        let regular = BodyFaceKey { family: BodyFamily::Serif, style: BodyFaceStyle::Regular };
        assert!(metrics.get().advance_em(regular, 'A').is_some());
        assert!(metrics.get().advance_em(regular, '中').is_some());
        let mono = BodyFaceKey { family: BodyFamily::Mono, style: BodyFaceStyle::Regular };
        assert!(metrics.get().advance_em(mono, 'M').is_some());
    }

    #[test]
    fn modern_default_metrics_are_regression_pinned() {
        let hint = Theme::Baseline.layout_hint();
        let style = hint.style;
        assert_eq!(hint.base_font_size, 11.0);
        assert_eq!(hint.line_height, 1.72);
        assert!((hint.line_height * hint.base_font_size - 18.92).abs() < 1e-9);
        assert_eq!(hint.margin.top, 88.0);
        assert_eq!(hint.margin.right, 100.0);
        assert_eq!(hint.margin.bottom, 96.0);
        assert_eq!(hint.margin.left, 100.0);
        assert!((hint.content_width() - 395.276).abs() < 1e-9);
        assert!((hint.content_height() - 657.89).abs() < 1e-9);
        assert_eq!(style.paragraph_gap, 8.0);
        assert_eq!(style.paragraph_indent, 0.0);
        assert_eq!(style.heading_scales, [1.60, 1.35, 1.18, 1.10]);
        assert_eq!(style.heading_gap_before, [0.0, 24.0, 16.0, 12.0]);
        assert_eq!(style.heading_gap_after, [20.0, 8.0, 4.0, 4.0]);
        assert!(style.heading_bold);
        assert_eq!(style.heading_rule, 0.75);
    }

    #[test]
    fn modern_default_rules_and_inks_are_regression_pinned() {
        let style = Theme::Baseline.style();
        assert_eq!(style.table_header_rule, 0.75);
        assert_eq!(style.table_row_rule, 0.5);
        assert_eq!(style.table_final_rule, 0.75);
        assert_eq!(style.table_rule_strong_color, "#C7C7C7");
        assert_eq!(style.table_rule_color, "#EEEEEC");
        assert_eq!(style.table_pad_h, 8.0);
        assert_eq!(style.table_pad_v, 4.0);
        assert_eq!(style.code_size, 10.0);
        assert_eq!(style.code_line_height, 1.45);
        assert_eq!(style.code_padding, 12.0);
        assert_eq!(style.code_fill, "#F7F7F4");
        assert_eq!(style.code_border, 0.5);
        assert_eq!(style.code_border_color, "#E3E3E0");
        assert_eq!(style.quote_rule, 2.5);
        assert_eq!(style.quote_indent, 12.0);
        assert_eq!(style.quote_text_color, "#4A4A4A");
        assert_eq!(style.accent, "#2B5B84");
        assert_eq!(style.ink, "#1A1A1A");
        assert_eq!(style.ink_secondary, "#4A4A4A");
        assert_eq!(style.ink_muted, "#737373");
        assert_eq!(style.ink_faint, "#9A9A9A");
        assert_eq!(style.rule_strong, "#C7C7C7");
        assert_eq!(style.rule, "#E3E3E0");
        assert_eq!(style.rule_faint, "#EEEEEC");
        assert_eq!(style.tint, "#F7F7F4");
        assert_eq!(style.header_size, 8.5);
        assert_eq!(style.header_color, "#737373");
        assert_eq!(style.folio_size, 9.0);
        assert_eq!(style.folio_color, "#737373");
        assert!(style.chrome);
    }

    #[test]
    fn editorial_and_classic_keep_their_own_metre() {
        let editorial = Theme::Editorial.layout_hint();
        assert_eq!(editorial.base_font_size, 11.5);
        assert_eq!(editorial.line_height, 1.65);
        assert_eq!(editorial.margin.top, 92.0);
        assert_eq!(editorial.margin.bottom, 100.0);
        assert_eq!(editorial.style.accent, "#B3402F");
        assert_eq!(editorial.style.heading_scales[0], 2.00);
        assert_eq!(editorial.style.table_header_tint, Some("#F5F1E9"));

        let classic = Theme::Classic.layout_hint();
        assert_eq!(classic.base_font_size, 10.5);
        assert_eq!(classic.line_height, 1.60);
        assert_eq!(classic.margin.left, 108.0);
        assert_eq!(classic.style.paragraph_gap, 0.0);
        assert_eq!(classic.style.paragraph_indent, 21.0);
        assert_eq!(classic.style.heading_align, Align::Center);
        assert_eq!(classic.style.table_row_rule, 0.0);
        assert_eq!(classic.style.accent, "#111111");
    }
}
