# `vutex-render-pdf`

PDF output backend for VuTeX.

Diagram scenes are painted as vector geometry: shapes keep their fill and
stroke, node transforms compose under a single y-flip at the root, labels stay
upright with their anchored alignment, and opacity is honoured through
`ExtGState` resources whose `/ca` and `/CA` entries select a node's alpha
around `q` and `Q`.

Image nodes and page-level image slots are embedded as raster XObjects when
their source resolves. Sources resolve through `PdfResources`, the caller-owned
context: a supplied byte payload wins, then a `data:` URI with a base64 payload,
then an absolute filesystem path, then a path relative to the configured base
directory, then, only for backwards compatibility, the process working
directory. Anything else stays unresolved, including an `http(s)` or `file:`
URL the caller did not supply. A host that already fetched a remote asset hands
its bytes to `PdfResources::with_supplied`, so the backend embeds a picture it
cannot fetch itself, and `PdfResources::with_base_dir` keeps a document's
relative references next to the document instead of the process. One object
serves every slot naming the same source. An RGBA or grey-alpha PNG keeps its
alpha channel instead of losing it: the writer emits
the alpha plane as a single-channel `/DeviceGray` image, appends a
`/SMask <n> 0 R` entry to the colour image dictionary, and registers only the
colour image as the `/ImN` resource. Palette transparency from an 8-bit `tRNS`
chunk expands the same way, and a colour-key `tRNS` on a greyscale or truecolour
PNG becomes a mask whose keyed pixels are fully transparent. 16-bit samples
narrow to their high byte before embedding, and Adam7-interlaced rows are
de-interlaced before unfiltering. PNG sources with 1, 2, or 4 bits per sample
are still refused. An absent, unresolved, unsupported, or corrupt source
degrades to the dashed placeholder instead of failing the document.

SVG sources become vector geometry rather than rasters, so icons keep their
curves at any output resolution and register no `/XObject`. A source counts as
SVG when its path ends in `.svg`, its `data:` URI declares `image/svg+xml`, or
the caller supplied that MIME type in a scene resource reference. The supported
subset is `<svg>` with `width`, `height`, and `viewBox`, `<g>` transforms and
opacity, `<path>`, `<rect>` with `rx`/`ry`, `<circle>`, `<ellipse>`, `<line>`,
`<polyline>`, and `<polygon>`, path commands `M L H V C S Q T Z` (absolute and
relative, with implicit repeats), `fill`, `stroke`, `stroke-width`,
`fill-opacity`, `opacity`, `color`, `style` declarations for the same
properties, `none`, `currentColor`, `#rgb`, `#rrggbb`, and a small named-colour
table. The default `xMidYMid meet` viewBox fit centres the artwork and preserves
its aspect ratio; a missing `width` or `height` falls back to the matching
`viewBox` extent. The same source is parsed once and painted once per slot,
fitted and centred in the slot.

Everything outside that subset keeps the placeholder instead of painting
something wrong: elliptical arcs, gradients and patterns, CSS classes and
`<style>` rules, `<text>`, `<use>`, symbols, masks, clip paths, filters,
markers, nested `<svg>`, unknown elements, malformed XML, and a path without
usable geometry.

The writer needs no third-party crate. Text the WinAnsi encoding reaches uses
the base-14 standard fonts, so Latin documents embed no font program. Anything
outside WinAnsi is painted with subsets of `glyf` TrueType fonts, embedded as
Identity-H `CIDFontType2` programs with a `ToUnicode` map, so the glyphs reach
the page and extracted text stays readable, including code points above the
Basic Multilingual Plane.

Characters come from a fallback chain rather than one face: a Han body font
rarely carries emoji, and an emoji face rarely carries Han characters. Each
program covers the characters no earlier program covered, so a run mixing both
paints from two subsets while keeping one text matrix, and the font is switched
between glyph groups. `PdfFontPolicy` selects the chain: `Discover` probes
system TrueType fonts (symbol and Latin faces, then CJK faces, then monochrome
emoji; absent or unusable candidates are skipped), `File` embeds one file, and
`Files` embeds the authored list in order. With an explicit list every entry
must be readable, and a character no program covers is an
`vutex.render.pdf.glyph_missing` error naming the code points rather than a
dropped glyph or a `?`. `Base14Only` intentionally keeps the `?` degradation.
`render_pdf_with_resources` accepts both the font policy and the caller's
`PdfResources` context.
