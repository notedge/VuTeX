# `vutex-render-pdf`

PDF output backend for VuTeX.

Diagram scenes are painted as vector geometry: shapes keep their fill and
stroke, node transforms compose under a single y-flip at the root, labels stay
upright with their anchored alignment, and opacity is honoured through
`ExtGState` resources whose `/ca` and `/CA` entries select a node's alpha
around `q` and `Q`.

Image nodes are embedded as raster XObjects when their resource resolves: a
`data:` URI with a base64 payload or a filesystem path is read, PNG samples are
inflated to 8-bit data, and JPEG streams pass through `/DCTDecode`. An RGBA or
grey-alpha PNG keeps its alpha channel instead of losing it: the writer emits
the alpha plane as a single-channel `/DeviceGray` image, appends a
`/SMask <n> 0 R` entry to the colour image dictionary, and registers only the
colour image as the `/ImN` resource. Palette transparency from an 8-bit `tRNS`
chunk expands the same way. Alpha sources still unsupported keep their opaque
colour plane: 16-bit samples are refused, and colour-key `tRNS` on greyscale or
truecolour PNGs is ignored. An unresolved reference, an unsupported format, or
a corrupt header degrades to the dashed placeholder instead of failing the
document.

The writer needs no third-party crate. Text the WinAnsi encoding reaches uses
the base-14 standard fonts, so Latin documents embed no font program. Anything
outside WinAnsi is painted with a subset of a system TrueType font, embedded as
an Identity-H `CIDFontType2` program with a `ToUnicode` map, so the glyphs reach
the page and extracted text stays readable. `PdfFontPolicy` pins or disables
that lookup.
