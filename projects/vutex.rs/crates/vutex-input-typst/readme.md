# vutex-input-typst

Typst input adapter for VuTeX. It selects the Notedown Typst lowering profile, projects
the resulting `notedown-ir` document graph into a `vutex-types` conversion report, and
orchestrates the layout, presentation, and SVG render chain.

It owns no Typst parser and no document IR. Parsing stays in `oak-typst` and lowering
stays in `notedown-formats`, so Typst cannot grow a second style system or a second
typesetting kernel.

```rust,ignore
let rendered = vutex_input_typst::render_svg("main.typ", "= Title\n\nHello *world*.\n")?;
println!("{}", rendered.svg);
```
