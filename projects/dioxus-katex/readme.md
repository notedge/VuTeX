KaTeX for Dioxus
================

Render math with KaTeX in Dioxus!

## How to use

1. When `auto-cdn` feature is enable, CDN will be inserted to head when the first component of this type is loaded.

If you want to manage CDN by yourself, you can disable this feature and manually import it

```html
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.12.0/dist/katex.min.css">
```

2. Easily added using `<KaTeX/>`

```rust
use yew_katex::KaTeX;

html! {
    <KaTeX math="\\KaTeX" inline=false/>
}
```

## Todo

- [x] Automatically import css cdn when the first component is loaded
