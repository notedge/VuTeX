# VuTeX

**Language-neutral document typesetting and rendering in Rust.** VuTeX consumes Notedown's canonical document model (`notedown-ir::DocumentGraph`) and produces layout plans, presentations, and renderer backends.

> **VuTeX is a product name, not a TeX renderer.** TeX / Markdown / Typst / DOCX / EPUB / HTML are input formats lowered through Notedown's import services. VuTeX never re-implements a language parser or owns a second document IR.

## Architecture

```text
Oak language frontends
  (oak-notedown / oak-markdown / oak-tex / oak-typst)
        │
        ▼
notedown-formats::import (Markdown / Notedown / HTML / TeX / Typst / DOCX / EPUB)
        │
        ▼
notedown-ir::DocumentGraph   ◄──  canonical document model
  (blocks, inlines, tables, links, images, math, citations,
   relations, assets, provenance, coverage, loss)
        │
        ▼
vutex-layout (page / block / line / table / pagination)
        │
        ├── vutex-style (semantic styles, named styles, inheritance, themes)
        ├── vutex-text (shaping, font metrics, line breaking)
        │     └── wetex (formula layout — WASM product face)
        ▼
vutex-presentation (display / flow)
        │
        ▼
vutex-render (backend contract)
  ├─ vutex-render-pdf
  ├─ vutex-render-svg
  ├─ vutex-render-html
  └─ vutex-render-raster
        │
        ▼
vutex-napi / wetex-wasm (host adapters)
        │
        ▼
@vutex/vutex / @vutex/wetex (TypeScript product facades)
```

**Notedown is VuTeX's canonical document model.** VuTeX does not own a second `Document` / `Node` / `Graph` IR. Every crate that manipulates document semantics operates directly on `notedown-ir::DocumentGraph`.  Source spans, relations, assets, provenance and coverage/loss all stay attached — no projection boundary that drops provenance.

### What VuTeX does NOT own

- No document IR parallel to `notedown-ir`.
- No language parsers (Markdown / TeX / Typst / ...).
- No format import/export (that is `notedown-formats`).
- No pipeline/input/transform/output builder — those belong to upstream products (Panduck) or language-specific lowering.

## Crate layout

| Crate | Role |
|-------|------|
| `vutex-types` | Shared IDs, diagnostics, conversion reports, math primitives |
| `vutex-style` | Semantic styles, named styles, inheritance, themes |
| `vutex-text` | Shaping, font metrics, line breaking |
| `vutex-layout` | Consumes `notedown-ir::DocumentGraph` → `LayoutPlan` |
| `vutex-presentation` | Flow/display presentation lowering |
| `vutex-render` + backends | Renderer contracts (PDF / SVG / HTML / raster) |
| `wetex-wasm` | Math formula distribution |
| `vutex-napi` | Node native host |

### Backend status

| Backend | Status |
|---------|--------|
| `vutex-render-svg` | Implemented: one stacked SVG document per render. |
| `vutex-render-html` | Implemented: semantic reflow HTML and fixed-geometry snapshot HTML. |
| `vutex-render-pdf` | Implemented: dependency-free PDF 1.7 writer over the base-14 fonts, Latin-1 text only. |
| `vutex-render-raster` | Stub: returns the `vutex.render.raster.unimplemented` diagnostic. |

## Quick start

```bash
cargo check --workspace          # every workspace member compiles
cargo test --workspace           # layout, presentation, renderer, napi and wasm suites
```

## Product packages

```text
@vutex/vutex              Runtime facade + CLI
@vutex/wetex              Formula distribution
@vutex/vutex-unknown-wasm32   wasm-pack artifact
@vutex/vutex-<platform>   napi platform bags
```

Design truth lives in the VuTeX Living docs. Implementation follows Notedown's import/export contract — see [notedown](https://github.com/notedge/notedown) for the canonical model.
