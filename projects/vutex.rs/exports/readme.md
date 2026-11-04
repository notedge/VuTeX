# Export capability status

What the two VuTeX exports expose today, what they actually do, and where the
output is known to fall short. Written for consumers of the packages, not as a
plan.

Status words used below:

- **available** — the entry returns a result for the documented input.
- **available, fidelity gap** — it returns a result, but a measured dimension is
  known to differ from an independent implementation.
- **rejected** — it returns a diagnostic and no output for that capability.
- **unavailable** — the entry aborts.

## `vutex-napi` (Node-API)

### Entry points

| Entry | Returns | Status |
| --- | --- | --- |
| `vutexVersion()` | binding version string | available |
| `inspectDocument(path)` | `{ inputFormat, path, reportJson }` | available for `.md`, `.nd`, `.html`, `.tex`, `.typst` |
| `renderDocument(path, optionsJson)` | `{ exitCode, outputFormat, svg, html, pdf, reportJson }` | available, fidelity gaps below |
| `measureFormula`, `renderFormulaSvg`, `renderFormulaMathml`, `renderFormula` | JSON `FormulaRender` | available — installs the embedded faces once per process |

`renderDocument` resolves the import format from the path extension or the
`input` option. `output` accepts `svg`, `html`, `snapshot` or `pdf`, and
defaults to `svg`. Every request returns `exitCode = 1` with a `status: failed`
report when it cannot proceed.

### Document pipeline

| Capability | Status | What you get | Chain |
| --- | --- | --- | --- |
| Markdown, Notedown, HTML, TeX, Typst import | available | a document graph and structural counts | `document-formats` → `document-ir` |
| SVG output | available | one stacked SVG document | `document-layout` → `document-presentation` → `document-renderer-svg` |
| HTML output | available | semantic reflow HTML | `document-presentation` → `document-renderer-html` |
| Snapshot output | available | fixed-geometry HTML | `document-presentation` → `document-renderer-html` |
| PDF output | available, fidelity gaps | paginated PDF bytes | `document-renderer-pdf` |
| DOT, Mermaid, PlantUML fences | available, fidelity gap | a resolved diagram scene placed by layout | `plotter-input-dot` / `-mermaid` / `-plantuml` |
| TikZ fences | available, fidelity gap | a resolved diagram scene for the supported subset | `plotter-input-tikz` |
| Inline and block maths in a document | available, fidelity gap | vector outlines in SVG and PDF, absent from text extraction | `document-layout` maths → `document-renderer-svg` / `-pdf` |
| PDF text faces | **not embedded** | base-14 `Times-Roman` references | `document-renderer-pdf` |

Measured gaps from the cross-check corpus:

| Gap | Ours | Reference | Effect |
| --- | --- | --- | --- |
| Subscript baseline depth | 0.247em | 0.150em (KaTeX and MathJax agree) | subscript sits too low |
| Fraction numerator/denominator size | 1.0× | 0.700× (KaTeX), 0.707× (MathJax) | fractions do not shrink their parts |
| Maths in PDF and SVG | 5 vector path operators, formula absent from extraction | vector outlines with searchable text | renders correctly, but copy and search miss the formula |
| PDF text font embedding | not embedded | embedded | portable readers may substitute the face |
| DOT diamond bounding-box aspect | 6.636 | 5.448 (Graphviz) | our layout spreads horizontally |
| TikZ `\node`, `white`, `purple` | rejected | rendered before the upstream switch | six constructs lost |

The subscript and fraction rows are real defects: both independent references
agree with each other and with TeX semantics. The maths-in-PDF row is a known
limitation, not a defect: the outlines are geometrically correct and the
formula simply does not survive text extraction. The DOT aspect and the TikZ
rows are real regressions against their references and the previous host
implementation.

### Explicit rejections

| Request | Diagnostic |
| --- | --- |
| Path does not exist | `VUTEX_INPUT_NOT_FOUND`, `exitCode = 1` |
| Unknown extension or unsupported `input` override | `VUTEX_INPUT_FORMAT_UNSUPPORTED` |
| Import adapter fails on the source | `VUTEX_IMPORT_FAILED` |
| `output` outside `svg`, `html`, `snapshot`, `pdf` | `VUTEX_OUTPUT_FORMAT_UNSUPPORTED` |
| Render backend fails | `VUTEX_RENDER_FAILED` |
| Diagram fence whose language is not `mermaid`, `dot`, `plantuml` or `tikz` | no scene, authored source text stays visible |
| Diagram lowering rejects the source | diagnostic code from the plotter frontend, with a source span; the block renders as source text |
| TikZ construct outside the supported subset | `davinci.diagram.unsupported` |
| PDF image payload that is not PNG or JPEG | placeholder image |
| Unknown `fontPolicy` value | falls back to `discover` |

## `watex-wasm` (WASM)

### Entry points

| Entry | Returns | Status |
| --- | --- | --- |
| `watexVersion()` | binding version string | available |
| `measureFormula(source, optionsJson)` | JSON `FormulaRender` with metrics | available, fidelity gap |
| `renderSvg(source, optionsJson)` | JSON `FormulaRender` with SVG | available, fidelity gap |
| `renderHtml(source, optionsJson)` | JSON `FormulaRender` with an HTML fragment | available, fidelity gap |
| `renderMathml(source, optionsJson)` | JSON `FormulaRender` with Presentation MathML | available |
| `renderFormula(source, optionsJson)` | JSON `FormulaRender` with SVG and MathML | available, fidelity gap |

Input is TeX maths only. The same payload type is used everywhere: `metrics`,
`svg`, `html`, `mathml`, `semantics` and `diagnostics`. Both exports install the
embedded Latin Modern Math and XITS Math faces once per process before layout,
so a caller never has to install fonts first.

### Options

| Option | Values | Default |
| --- | --- | --- |
| `displayMode` | `inline`, `block` | `inline` |
| `fontSize` | CSS pixels | 14 inline, 16 block |
| `fontFamily` | `xits-math`, `lm-math`, `stix-two-math` | `xits-math` |
| `fontConformance` | `allow-fallbacks`, `strict` | `allow-fallbacks` |

### Explicit rejections

| Request | Diagnostic |
| --- | --- |
| `fontFamily: stix-two-math` | `WATEX_UNSUPPORTED_FONT_FAMILY`, no layout runs |
| No embedded math face installed in the host | `WATEX_FONT_UNAVAILABLE`, layout does not run |
| Empty source | `WATEX_EMPTY_SOURCE` |
| TeX command outside the supported maths grammar | parse or layout diagnostic; SVG is empty when parsing fails |
| Markdown, HTML or a full LaTeX document shell | parse diagnostics, no document semantics |

`fontConformance: strict` turns any consumed font fallback into a failed layout.
The default keeps each fallback as a countable warning.

## Reading the numbers

The measurements above come from the same-ecosystem cross-check in
`projects/vutex.ts/examples/crosscheck`. Each JSONL row records `ours`,
`theirs`, the tolerance and one of `real-defect`, `spec-ambiguity`, `their-bug`
or `our-limitation`. Tolerances are fixed in the case corpora, so a rerun
reproduces the same verdicts.
