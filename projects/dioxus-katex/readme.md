KaTeX for Dioxus
================

Render math with KaTeX in Dioxus!

## How to use

- First you need to import css cdn:

```html
<link rel="stylesheet" href="https://unpkg.com/katex@0.12.0/dist/katex.min.css">
```

- Call `use_katex_display` hook to prepare context.
- Call `compile` to get math expression node.

```rust
use dioxus_katex::use_katex_display;

let katex = use_katex_display(&cx);
let math = katex.compile(text);
```
