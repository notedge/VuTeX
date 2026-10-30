# Product boundaries and rendering flow

VuTeX is organized around language-neutral document semantics, layout and output. Replaceable source adapters normalize input before page layout is computed.

```text
Source → Adapter → Document IR → Layout → Presentation → Output
```

Output directions include PDF, SVG, HTML and raster formats. Formula layout uses the same internal pipeline, while WeTeX provides a standalone WASM and JavaScript facade.
