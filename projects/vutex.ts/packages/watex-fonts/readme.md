# @vutex/watex-fonts

Math font assets for WaTeX in browsers and Node: Latin Modern Math and XITS
Math, with their licence texts.

## Contents

- `assets/fonts/fonts.json` — file names, roles and licence files.
- `assets/fonts/LatinModernMath-Regular.otf`
- `assets/fonts/XITSMath-Regular.otf`
- `assets/fonts/*.md` — GUST Font License and SIL OFL 1.1 texts.

## CDN

`@vutex/watex` loads these files from jsDelivr by default, pinned to the exact
package version:

```text
https://cdn.jsdelivr.net/npm/@vutex/watex-fonts@<version>/assets/fonts/LatinModernMath-Regular.otf
https://cdn.jsdelivr.net/npm/@vutex/watex-fonts@<version>/assets/fonts/XITSMath-Regular.otf
```

Pass `assetBaseUrl` to point at another CDN, or pass the two byte buffers
directly to `loadWaTeX` when the host already has them.

## Licence

Fonts are distributed under their own licences; see the `*.md` files next to
each face.
