# VuTeX

Language-neutral document typesetting and rendering in Rust.

> **VuTeX is a product name, not a TeX renderer.** TeX, Markdown, MDX, Typst, and Notedown are input adapters only.

## Pipeline

```text
Source Language → Adapter → Canonical Document IR → Layout → Presentation → Output
```

## Crate layout

```text
vutex-types              IDs, diagnostics, conversion reports
vutex-document           Canonical document IR
vutex-style              Semantic styles and themes
vutex-layout             Layout hints and layout plans
vutex-text               Shaping and line breaking
vutex-presentation       Display and flow presentation IR
vutex-render             Renderer contracts
vutex-render-pdf/svg/html/raster
vutex-input-notedown/markdown/mdx/typst/tex
wetex-wasm                WeTeX formula WASM facade
vutex-napi                Node native host
```

Product packages:

```text
@vutex/vutex              Runtime facade + CLI
@vutex/wetex              Formula distribution
@vutex/vutex-unknown-wasm32   wasm-pack artifact
@vutex/vutex-<platform>   napi platform bags
```

Design truth lives outside this repository. Implementation follows the VuTeX Living docs in the parent workspace.

## Quick start

```bash
cargo check --workspace
cargo test --workspace
pnpm install
pnpm build:napi
pnpm build:wasm
pnpm typecheck
pnpm test
```

## Status

Scaffold stage: `vutex-*` + `vutex-napi` + `@vutex/vutex` CLI and package manager stubs. Formula and document output remain placeholders until `G-VUTEX-MATH` and slice 1 layout land.
