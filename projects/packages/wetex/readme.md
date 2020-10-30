# @vutex/wetex

**WeTeX** exports VuTeX formula rendering to JavaScript hosts through WASM.

```ts
import { loadWeTeX, renderSvg } from "@vutex/wetex";

const wetex = await loadWeTeX();
const result = renderSvg(wetex, "x^2 + y^2 = z^2");
console.log(result.svg);
```

Standalone **web** package: only `@vutex/vutex-unknown-wasm32` + thin TypeScript over `wetex-wasm`. Does **not** depend on `@vutex/vutex`.
