# `vutex-render-pdf`

PDF output backend for VuTeX.

The writer needs no third-party crate. Text the WinAnsi encoding reaches uses
the base-14 standard fonts, so Latin documents embed no font program. Anything
outside WinAnsi is painted with a subset of a system TrueType font, embedded as
an Identity-H `CIDFontType2` program with a `ToUnicode` map, so the glyphs reach
the page and extracted text stays readable. `PdfFontPolicy` pins or disables
that lookup.
