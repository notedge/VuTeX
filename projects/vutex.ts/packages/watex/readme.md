# @vutex/watex

**WaTeX** exports VuTeX formula rendering to JavaScript hosts through WASM.

```bash
npm install @vutex/watex
```

```ts
import { loadWaTeX, renderSvg } from "@vutex/watex";

const watex = await loadWaTeX();
const result = renderSvg(watex, "x^2 + y^2 = z^2");
console.log(result.svg);
```

Standalone **web** package: only `@vutex/vutex-unknown-wasm32` + thin TypeScript over `watex-wasm`. Does **not** depend on `@vutex/vutex`.

The WASM artifact ships no font. `loadWaTeX()` installs the two math faces
before returning: in Node it reads them from `@vutex/watex-fonts`, in a browser
it fetches the pinned jsDelivr URLs for the installed exact version. A host
that serves the assets itself passes `assetBaseUrl`, and one that already read
them passes `assets: { latinModern, xits }`. Calling the raw WASM exports
without the JS facade requires `installMathFonts(latinModern, xits)` first;
otherwise a render reports `VUTEX_FONT_ASSETS_NOT_CONFIGURED`.

Formula geometry follows the Tectonic `0.17.0` + `default_bundle_v33.tar`
reference used by the repository geometry gate. This is the gated math subset,
not a full TeX engine. Unsupported commands return diagnostics instead of a
silent fallback, and strict font conformance rejects missing glyph variants or
constants with `vutex.math.conformance.*`.
