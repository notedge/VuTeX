#![forbid(missing_docs)]
#![forbid(missing_debug_implementations)]
#![forbid(rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../Readme.md")]
#![doc(html_logo_url = "https://avatars.githubusercontent.com/u/31191489")]
#![doc(html_favicon_url = "https://avatars.githubusercontent.com/u/31191489")]

use wasm_bindgen::prelude::*;
mod options;
pub use options::KaTeXOptions;

#[wasm_bindgen(module = "/src/katex.min.js")]
extern "C" {
    #[wasm_bindgen]
    pub fn render_to_string(expr: &str, args: &JsValue) -> String;
}
