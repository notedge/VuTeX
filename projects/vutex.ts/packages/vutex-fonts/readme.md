# @vutex/vutex-fonts

Bundled font assets used by the VuTeX native runtime: six body faces and two
OpenType math faces, together with their licence texts.

## Contents

- `assets/fonts/fonts.json` — file names, roles and licence files.
- `assets/fonts/*.ttf` — Tinos (regular, bold, italic), Noto Serif SC
  (regular, bold), Noto Sans Mono (regular).
- `assets/fonts/*.otf` — Latin Modern Math, XITS Math.
- `assets/fonts/*.md` — SIL OFL 1.1 and GUST Font License texts.

`fonts.json` lists names and roles only. Package versions and tarball integrity
are provided by npm.

## Usage

`@vutex/vutex` depends on this package and resolves the assets directly. When
configuring the native runtime yourself, pass the directory
`@vutex/vutex-fonts/assets/fonts` to the byte-loading or directory-loading
entry point.

## Licence

Fonts are distributed under their own licences; see the `*.md` files next to
each face. The package itself contains no VuTeX code.
