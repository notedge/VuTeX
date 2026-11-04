# Render cross-check

Independent engines in the *same ecosystem* re-render the same input, and the
measurable dimensions of both outputs are compared against ours. The point is to
let someone else's implementation expose our own problems.

Two hard rules:

- **Same ecosystem only.** TypeScript examples compare against npm libraries.
  Python examples compare against Python libraries. A dimension with no
  counterpart in the target ecosystem is skipped and written down as
  `no reference in this ecosystem`, never replaced by a different-ecosystem tool.
- **Never use our own code as the reference.** `plotter.input.*`,
  `document.renderer.*`, and VuTeX's own exporters are the subject under test.

## Layout

```
crosscheck/
  lib/            shared loaders and the JSONL report writer
  math/           KaTeX and MathJax (mathjax-full) SVG geometry
  diagram/        @viz-js/viz (Graphviz npm build) and mermaid-cli
  pdf/            pdfjs-dist on the TS side, pymupdf and pdfminer.six on the Python side
  tikz/           capability audit before and after lowering moved upstream
  results/        generated evidence, git-ignored
```

`node run-all.mjs` runs math, diagram, both PDF passes, and the TikZ audit in
that order. The Python pass reuses the PDFs written by the TypeScript pass.

`pdf/python/` is its own directory with its own `requirements.txt` and virtual
environment. Third-party packages live only in this example's
`devDependencies` and in that Python environment. Nothing here is imported by
`exports/*` or by a published package.

## Judgments

Every feature declares one of three judgments in `criteria.json`:

| judgment | meaning |
| --- | --- |
| `equivalent` | the values must agree inside a written tolerance |
| `tolerance` | the values are compared, and the tolerance is written as a number |
| `record-only` | both values are stored, no verdict is emitted |

A deviation outside tolerance becomes one line in
`results/crosscheck/<lib>/<timestamp>.jsonl` with `feature`, `ours`, `theirs`,
`classification`, and the tolerance that was applied. The classification set is
closed:

- `real-defect` — the references agree with each other inside tolerance, and we
  disagree with all of them by more than twice the tolerance.
- `spec-ambiguity` — the references disagree with each other, or the deviation
  sits between the tolerance and twice the tolerance.
- `their-bug` — we agree with one reference and the other reference deviates.
- `our-limitation` — a dimension we knowingly do not produce, recorded so the
  gap stays visible.

Values inside tolerance are counted in the run summary and are not written as
diff lines.

## Math geometry

Values are normalised to em. Our metrics arrive in CSS pixels at the layout base
size (14 for inline, 16 for display), so `em = px / base`. MathJax SVG geometry
arrives in font units where one em is 1000 units and one ex is 442 units, which
is the x-height constant of the MathJax TeX fonts. KaTeX has no SVG backend, so
its height and script shift come from inline `em` styles in the emitted HTML.

Hard-coded tolerances:

| feature | tolerance |
| --- | --- |
| `baseline_height` | 0.04 em |
| `baseline_depth` | 0.03 em |
| `script_shift` | 0.05 em |
| `script_size_ratio` | 0.03 |

KaTeX emits no depth and no per-glyph vector geometry, so `baseline_depth` and
fraction internals are `record-only` against KaTeX and judged against MathJax
only.

Math geometry does not depend on the embedded faces, so the install leaves it
untouched. The two runs are listed side by side to make that visible:

| measurement | before the install | after the install |
| --- | --- | --- |
| `sub` `baseline_depth` | 0.247em | 0.247em |
| `sub` `script_shift` | -0.247em | -0.247em |
| `frac` `script_size_ratio` | 1.0 | 1.0 |

Both rows stay `real-defect`; the face install neither fixes nor masks them.

## Diagram geometry

Counts are structural: Graphviz SVG carries `class="node"` and `class="edge"`
groups, and mermaid-cli SVG carries `class="node"` groups and edge paths. Our
SVG carries node shapes and connector paths, so the comparison is count against
count. Bounding boxes are compared as aspect ratio plus relative area, because
Graphviz emits points while we emit CSS pixels.

| feature | tolerance |
| --- | --- |
| `node_count` | exact |
| `edge_count` | exact |
| `bbox_aspect` | 8 percent |
| `bbox_area` | record-only |

## PDF

Our PDF is the artifact under test. Both sides extract from the same file:

- TypeScript: `pdfjs-dist` `getTextContent`, page count, and a normalised text
  comparison against the authored source.
- Python: `pymupdf` and `pdfminer.six`, page count, text, and font embedding
  flags (`pymupdf` only, since that is where the flag exists).

The maths case has two observable states, and the face install decides which
one appears:

| state | vector paths | extracted text | verdict |
| --- | --- | --- | --- |
| no face installed | 0 (`pdfjs`), 0 drawings (`pymupdf`) | `E=mc^2` is searchable | not typeset |
| faces installed | 5 (`pdfjs`), 5 drawings (`pymupdf`) | formula is absent, prose intact | typeset but not searchable |

Before the install fix, a fresh `vutex-napi` process rendered documents in the
first state, which made the earlier reading "the pipeline never creates a Math
block" look true. It was not: the block exists and lays out, but the missing
face degraded it to the authored source text. With a face installed the same
source yields vector outlines, so the formula is geometrically correct and no
longer extractable. Every export entry now installs the faces, so documents
render in the second state.

`math_searchable` and `math_typeset` are `our-limitation` in the second state,
not defects. The fraction and subscript measurements do not move with the
install: `frac` still reports a 1.0 size ratio and `sub` still sinks 0.247em,
which belong to the engine and are tracked there.

The text face itself is still non-embedded base-14 `Times-Roman`, so
`font_embedded` stays an `our-limitation` for every PDF.

| feature | tolerance |
| --- | --- |
| `page_count` | exact |
| `text_normalised` | exact after whitespace collapse |
| `font_embedded` | equivalent per font, `pymupdf` only |

## TikZ capability audit

`tikz/audit.mjs` is not a cross-check: there is no external reference in this
ecosystem. It replays 37 constructs through the deleted host parser's accepted
subset (`a76d2bf`, recorded before the switch) and through the current
`plotter.input.tikz` lowering, and writes only the rows whose verdict changed.
A construct that used to render and now fails is a capability regression and is
classified `real-defect`.

First run: 6 regressions. All `\node` forms are rejected (`\node at (0,0)
{label}`, `\node[above, text=red] at (0,0) {label}`), and so are `white` and
`purple`, in both the `[white]` / `[purple]` shorthand and the `[color=white]`
/ `[color=purple]` spellings. The upstream crate's own readme lists a `node`
tag, but its parser has no `node` branch, so the documentation and the
implementation disagree.

Frequency cannot be measured from this repository: `document.rs` mentions
`tikzpicture` only in that crate's readme, and VuTeX carries no TikZ corpus.
The audit therefore records the constructs and the verdict change, and leaves
the frequency judgment to document owners.

The six regressions are handed to the `plotter` side (`\node` branch and the
colour table). This repository adds no local fallback and waits for a rev bump.

## Evidence state, not repository state

Two caveats on how the first run was produced:

- The `sub` depth, `frac` size and untypeset-PDF-maths rows stay `real-defect`.
  Both independent references agree with each other and with TeX semantics, so
  there is no specification reading under which our values are correct.
- The smoke build needed a local, git-ignored `.cargo` override that drops the
  `oak-mermaid` path patch and resolves the pinned git rev. The overlay adds
  `MermaidDiagram::State` and `EntityRelationship` before
  `plotter.input.mermaid` handles them, so the build breaks until the
  `plotter` side catches up. That override is only for collecting evidence. It
  is not a fix and it is never committed. The repository is not buildable in
  that window.
