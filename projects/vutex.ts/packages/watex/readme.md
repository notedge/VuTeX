# @vutex/watex

**WaTeX** exports VuTeX formula rendering to JavaScript hosts through WASM.

```ts
import { loadWaTeX, renderSvg } from "@vutex/watex";

const watex = await loadWaTeX();
const result = renderSvg(watex, "x^2 + y^2 = z^2");
console.log(result.svg);
```

Standalone **web** package: only `@vutex/vutex-unknown-wasm32` + thin TypeScript over `watex-wasm`. Does **not** depend on `@vutex/vutex`.
