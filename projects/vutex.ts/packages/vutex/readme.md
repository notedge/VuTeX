# @vutex/vutex

VuTeX TypeScript runtime facade and official CLI.

## Install

```bash
npm install -g @vutex/vutex
```

`@vutex/vutex` installs the matching `@vutex/vutex-<platform>` native package
as an optional dependency. The CLI requires that native package; browsers and
JavaScript-only hosts use `@vutex/watex` instead.

## Usage

```bash
vutex render sample.nd --output out.svg
vutex inspect sample.nd
vutex package doctor
```

```ts
import { createVuTeX } from "@vutex/vutex";

const vutex = await createVuTeX({ projectRoot: process.cwd() });
const result = await vutex.render("sample.nd", { output: "svg" });
```

Resource packages (fonts, templates, profiles) are managed by the VuTeX package manager via `vutex.toml`, not npm.

## Scope

VuTeX is a document typesetting runtime, not a TeX engine. Formula geometry is
checked against Tectonic `0.17.0` with `default_bundle_v33.tar` and amsmath
2021/10/15 v2.17l, with 5% width, 10% height and 15% depth tolerances. Input
outside the gated corpus must surface a structured diagnostic. The raster
backend is a stub; use SVG, HTML or PDF. Fonts, licenses and known limitations
are listed in the repository `Readme.md`.
