#![forbid(missing_docs)]
#![forbid(missing_debug_implementations)]
#![forbid(rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../Readme.md")]
#![doc(html_logo_url = "https://avatars.githubusercontent.com/u/31191489")]
#![doc(html_favicon_url = "https://avatars.githubusercontent.com/u/31191489")]

pub use self::katex::{
    builder::{use_katex, use_katex_display, use_katex_inline},
    UseKatex,
};
pub use katex_wasmbind::KaTeXOptions;
mod katex;
