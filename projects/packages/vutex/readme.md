# @vutex/vutex

VuTeX TypeScript runtime facade and official CLI.

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
