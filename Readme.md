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
        │     └── watex (formula layout — WASM product face)
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
vutex-napi / watex-wasm (host adapters)
        │
        ▼
@vutex/vutex / @vutex/watex (TypeScript product facades)
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
| `watex-wasm` | Math formula distribution |
| `vutex-napi` | Node native host |

### Backend status

| Backend | Status |
|---------|--------|
| `vutex-render-svg` | Implemented: one stacked SVG document per render. |
| `vutex-render-html` | Implemented: semantic reflow HTML and fixed-geometry snapshot HTML. |
| `vutex-render-pdf` | Implemented: dependency-free PDF 1.7 writer over the base-14 fonts, plus a subset of a discovered system TrueType font for text outside WinAnsi. |
| `vutex-render-raster` | Stub: returns the `vutex.render.raster.unimplemented` diagnostic. |

## Math reference and conformance

Formula geometry is not validated against a local TeX Live installation. The
reference oracle is **Tectonic `0.17.0` with `default_bundle_v33.tar`**, using
**amsmath 2021/10/15 v2.17l** for the corpus constructs. Gate tolerances are
width 5%, height 10% and depth 15%; the geometry gate is
`cargo test -p vutex-layout --test oracle_latex_geometry -- --include-ignored`.

The gate walks the whole corpus in both styles rather than a curated subset.
`split` is the one entry amsmath refuses inside a box, so its two records carry
`status: unsupported` with the engine's message instead of a measurement. Two
width checks are documented font-data exceptions — the radical degree advance
and the inline sum-over-radical fraction — and every other divergence stays
blocking.

VuTeX is not a TeX engine. It implements the authoring constructs listed in
`projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json`; other
TeX macros, `\input`, TeX font metrics and unrestricted macro expansion are out
of scope. Unsupported input must produce a structured diagnostic rather than a
silently substituted glyph.

## Quick start

```bash
cargo check --workspace          # every workspace member compiles
cargo test --workspace           # layout, presentation, renderer, napi and wasm suites
```

## Product packages

```text
@vutex/vutex              Runtime facade + CLI
@vutex/watex              Formula distribution
@vutex/vutex-unknown-wasm32   wasm-pack artifact
@vutex/vutex-<platform>   napi platform bags
```

Design truth lives in the VuTeX Living docs. Implementation follows Notedown's import/export contract — see [notedown](https://github.com/notedge/notedown) for the canonical model.

## Bundled math fonts and licenses

The code is licensed under MPL-2.0 (see [`License.md`](./License.md)). The two
math fonts embedded from `projects/vutex.rs/crates/vutex-text/assets` keep
their own licenses, and each license text ships next to its font. Font
substitution is recorded as `vutex.math.conformance.*`; strict mode fails on a
fallback instead of silently accepting it.

| Font | License | License file | Source | SHA-256 |
|---|---|---|---|---|
| `XITSMath-Regular.otf` | SIL Open Font License 1.1 | `XITSMath-Regular.OFL.txt` | <https://github.com/alif-type/xits/releases/tag/v1.302> | `3025792ADB0B7072ACE08BF5726D341DE32F2254612DDEC6DD98271A8DD29689` |
| `LatinModernMath-Regular.otf` | GUST Font License 1.0 (legally equivalent to LPPL 1.3c or later) | `LatinModernMath-Regular.GUST-FONT-LICENSE.txt` | <https://www.gust.org.pl/projects/e-foundry/lm-math> | `6075562B771F8B82F0C179E363389684F2DD09DE30038269E2628E504BD7BE0F` |

## Known limitations

- `vutex-render-raster` is a stub and returns `vutex.render.raster.unimplemented`; use the SVG, HTML or PDF backends.
- Formula support is the gated corpus, not arbitrary TeX. Unknown commands and unsupported environments report errors.
- The native `.nd` Notedown format has no math syntax and keeps `$…$` as text; author math in Markdown with `$…$` inline or `$$…$$` display.
- The PDF backend writes base-14 text plus a subset of a discovered system TrueType font; it is not a full Unicode font pipeline.
- Glyph advances still differ from the Tectonic reference where the embedded Latin Modern Math design departs from the Computer Modern faces: `\sqrt[3]{x}` is 6.2% narrower and the inline `\frac{\sum_{i=1}^{n} i}{\sqrt{n}}` 16.6% narrower, while radical heights and depths agree within 0.25%. These width gaps come from font data, not from layout rules, and the geometry gate only covers a four-formula subset.
