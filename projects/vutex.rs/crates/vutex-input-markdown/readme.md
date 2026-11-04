# `vutex-input-markdown`

Markdown input adapter. It selects the `notedown-formats` Markdown import
profile, returns the resulting `notedown-ir::DocumentGraph`, and normalizes the
import loss into VuTeX's shared conversion report and diagnostics.

The crate owns no Markdown parser and no document schema of its own. Parsing and
semantic lowering stay in `notedown-formats`, so a second syntax model cannot
drift from the one the other input adapters share.

`import_source` returns the graph with a validated conversion report.
`import_file` reads one Markdown file. `render_svg` runs the same import through
layout, display presentation, and SVG output. Diagram fences stay owned by the
host: register their lowered scenes before rendering.
