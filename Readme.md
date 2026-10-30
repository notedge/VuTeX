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
| `vutex-render-pdf` | Implemented: dependency-free PDF 1.7 writer over the base-14 fonts, plus a subset of a discovered system TrueType font for text outside WinAnsi. |
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

## Bundled math fonts and licenses

The code is licensed under MPL-2.0 (see [`License.md`](./License.md)). The two
math fonts embedded from `projects/vutex.rs/crates/vutex-text/assets` keep
their own licenses, and each license text ships next to its font:

| Font | License | License file | Source | SHA-256 |
|---|---|---|---|---|
| `XITSMath-Regular.otf` | SIL Open Font License 1.1 | `XITSMath-Regular.OFL.txt` | <https://github.com/alif-type/xits> | `3025792ADB0B7072ACE08BF5726D341DE32F2254612DDEC6DD98271A8DD29689` |
| `LatinModernMath-Regular.otf` | GUST Font License 1.0 (legally equivalent to LPPL 1.3c or later) | `LatinModernMath-Regular.GUST-FONT-LICENSE.txt` | <https://www.gust.org.pl/projects/e-foundry/lm-math> | `6075562B771F8B82F0C179E363389684F2DD09DE30038269E2628E504BD7BE0F` |
