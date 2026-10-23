use crate::render_to_string;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::JsValue;

#[derive(Debug, Clone, Deserialize, Serialize)]
/// Read <https://katex.org/docs/options.html> for more information.
pub struct KaTeXOptions {
    /// Whether to render the math in the display mode.
    #[serde(rename = "displayMode")]
    pub display_mode: bool,

    /// If true, display math has `\tags` rendered on the left instead of the right.\
    /// like `\usepackage[leqno]{amsmath}` in LaTeX.
    #[serde(rename = "leqno")]
    pub left_equation_numbers: bool,
    /// If true, display math renders flush left with a `2em` left margin\
    /// like `\documentclass[fleqn]` in LaTeX with the amsmath package.
    #[serde(rename = "fleqn")]
    pub flush_left_equations: bool,
    /// Whether to let KaTeX throw a ParseError for invalid LaTeX.
    #[serde(rename = "throwOnError")]
    pub throw_on_error: bool,
    /// Color used for invalid LaTeX.

    /// Specifies a minimum thickness, in ems.
    #[serde(rename = "minRuleThickness")]
    pub min_rule_thickness: f64,
    /// Max size for user-specified sizes.
    /// If set to `None`, users can make elements and spaces arbitrarily large.
    #[serde(rename = "maxSize")]
    pub max_size: f64,
    /// Limit the number of macro expansions to the specified number.
    /// If set to `None`, the macro expander will try to fully expand as in LaTeX.
    #[serde(rename = "maxExpand")]
    pub max_expand: i32,
    /// strict mode
    pub strict: bool,
    /// Whether to trust users' input.
    /// Cannot be assigned at the same time with [`OptsBuilder::trust_callback`].
    pub trust: bool,
    /// group
    #[serde(rename = "globalGroup")]
    pub global_group: bool,

    /// Output format, `html` or `mathml` or `htmlAndMathml`
    output: String,
    /// Collection of custom macros.
    macros: HashMap<String, String>,
    #[serde(rename = "errorColor")]
    error_color: String,
}

impl Default for KaTeXOptions {
    fn default() -> Self {
        Self {
            display_mode: false,
            output: "html".to_string(),
            flush_left_equations: false,
            left_equation_numbers: false,
            throw_on_error: false,
            error_color: "#cc0000".to_string(),
            macros: Default::default(),
            min_rule_thickness: 0.04,
            max_size: f64::INFINITY,
            max_expand: 1000,
            strict: false,
            trust: false,
            global_group: false,
        }
    }
}

impl KaTeXOptions {
    /// Set output as `\displaystyle`
    pub fn display_mode() -> KaTeXOptions {
        KaTeXOptions { display_mode: true, ..KaTeXOptions::default() }
    }
    /// Set output as `\inlinestyle`
    pub fn inline_mode() -> KaTeXOptions {
        KaTeXOptions { display_mode: false, ..KaTeXOptions::default() }
    }
    /// Render formulas to html string.
    pub fn render(&self, input: &str) -> String {
        render_to_string(input, &JsValue::from_serde(self).unwrap())
    }
}

impl KaTeXOptions {
    /// Determines the markup language of the output. \
    /// The valid choices are:
    /// - `html`: Outputs KaTeX in HTML only.
    /// - `mathml`: Outputs KaTeX in MathML only.
    /// - `htmlAndMathml`: Outputs HTML for visual rendering and includes MathML for accessibility. This is the default.
    pub fn set_output_format(&mut self, format: &str) -> bool {
        let set = match format.to_ascii_lowercase().as_str() {
            "html" => "html",
            "mathml" => "mathml",
            "htmlandmathml" => "htmlAndMathml",
            _ => return false,
        };
        self.output = String::from(set);
        return true;
    }
    /// Set the color of the error message.
    pub fn set_error_color(&mut self) -> bool {
        unimplemented!()
    }
    /// Insert a custom macro.
    pub fn insert_macro_rule(&mut self) -> bool {
        unimplemented!()
    }
}
