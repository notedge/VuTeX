// TikZ capability audit after the host implementation was replaced by
// `plotter.input.tikz`. There is no external reference here: the "before"
// verdict comes from the deleted host parser at `a76d2bf`, whose accepted
// subset is documented in its module header and whose branch tables are reused
// verbatim by the upstream crate. The "after" verdict is measured by rendering
// each construct through the current Node-API export.
//
// Usage: node tikz/audit.mjs

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadNative, resultsRoot } from "../lib/ours.mjs";
import { createReport } from "../lib/report.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const native = loadNative();
const scratch = resolve(resultsRoot, ".build", "tikz");
mkdirSync(scratch, { recursive: true });

const CASES = [
    { id: "draw-line", before: "accept", body: "\\draw (0,0) -- (1,1);" },
    { id: "draw-polyline", before: "accept", body: "\\draw (0,0) -- (1,0) -- (1,1);" },
    { id: "draw-rectangle", before: "accept", body: "\\draw (0,0) rectangle (1,1);" },
    { id: "draw-circle", before: "accept", body: "\\draw (0,0) circle (1);" },
    { id: "fill", before: "accept", body: "\\fill (0,0) rectangle (1,1);" },
    { id: "filldraw", before: "accept", body: "\\filldraw (0,0) circle (1);" },
    { id: "node-label", before: "accept", body: "\\node at (0,0) {label};" },
    { id: "node-options", before: "accept", body: "\\node[above, text=red] at (0,0) {label};" },
    { id: "colour-black", before: "accept", body: "\\draw[black] (0,0) -- (1,0);" },
    { id: "colour-red", before: "accept", body: "\\draw[red] (0,0) -- (1,0);" },
    { id: "colour-blue", before: "accept", body: "\\draw[blue] (0,0) -- (1,0);" },
    { id: "colour-green", before: "accept", body: "\\draw[green] (0,0) -- (1,0);" },
    { id: "colour-white", before: "accept", body: "\\draw[white] (0,0) -- (1,0);" },
    { id: "colour-white-explicit", before: "accept", body: "\\draw[color=white] (0,0) -- (1,0);" },
    { id: "colour-purple", before: "accept", body: "\\draw[purple] (0,0) -- (1,0);" },
    { id: "colour-purple-explicit", before: "accept", body: "\\draw[color=purple] (0,0) -- (1,0);" },
    { id: "width-thin", before: "accept", body: "\\draw[thin] (0,0) -- (1,0);" },
    { id: "width-thick", before: "accept", body: "\\draw[thick] (0,0) -- (1,0);" },
    { id: "width-very-thick", before: "accept", body: "\\draw[very thick] (0,0) -- (1,0);" },
    { id: "width-explicit", before: "accept", body: "\\draw[line width=1mm] (0,0) -- (1,0);" },
    { id: "dashed", before: "accept", body: "\\draw[dashed] (0,0) -- (1,0);" },
    { id: "opacity", before: "accept", body: "\\draw[opacity=0.5] (0,0) -- (1,0);" },
    { id: "fill-opacity", before: "accept", body: "\\fill[fill opacity=0.5] (0,0) rectangle (1,1);" },
    { id: "picture-scale", before: "accept", body: "\\begin{tikzpicture}[scale=2]\n\\draw (0,0) -- (1,0);\n\\end{tikzpicture}" },
    { id: "unit-cm", before: "accept", body: "\\draw (0,0) -- (1cm,0);" },
    { id: "unit-mm", before: "accept", body: "\\draw (0,0) -- (10mm,0);" },
    { id: "unit-pt", before: "accept", body: "\\draw (0,0) -- (28.4527pt,0);" },
    { id: "path-command", before: "reject", body: "\\path (0,0) -- (1,0);" },
    { id: "clip", before: "reject", body: "\\clip (0,0) rectangle (1,1);" },
    { id: "curve-to", before: "reject", body: "\\draw (0,0) to[bend left] (1,0);" },
    { id: "foreach", before: "reject", body: "\\foreach \\x in {0,1} {\\draw (0,0) -- (\\x,0);}" },
    { id: "axis-environment", before: "reject", body: "\\begin{axis}\\addplot {x^2};\\end{axis}" },
    { id: "includegraphics", before: "reject", body: "\\includegraphics{logo.png}" },
    { id: "macro-definition", before: "reject", body: "\\newcommand{\\foo}{\\draw (0,0) -- (1,0);}" },
    { id: "arrow-tip", before: "reject", body: "\\draw[->] (0,0) -- (1,0);" },
    { id: "ellipse-shape", before: "reject", body: "\\draw (0,0) ellipse (1 and 0.5);" },
    { id: "arc-shape", before: "reject", body: "\\draw (0,0) arc (0:90:1);" },
];

function audit(name, body) {
    const path = resolve(scratch, `${name}.md`);
    writeFileSync(path, `# tikz\n\n\`\`\`tikz\n${body}\n\`\`\`\n`, "utf8");
    const result = native.renderDocument(path, JSON.stringify({ output: "svg" }));
    const svg = result.svg ?? "";
    const accepted = result.exitCode === 0 && svg.includes("data-diagram=");
    const report = JSON.parse(result.reportJson);
    const codes = (report.diagnostics ?? []).map((item) => item.code);
    return { accepted, codes, exit: result.exitCode };
}

const report = createReport("vutex-tikz");

for (const testCase of CASES) {
    const found = audit(testCase.id, testCase.body);
    const after = found.accepted ? "accept" : "reject";
    if (after !== testCase.before) {
        report.record({
            case: testCase.id,
            feature: "tikz_construct",
            ours: { verdict: after, codes: found.codes },
            theirs: { verdict: testCase.before, source: "a76d2bf host parser" },
            tolerance: "exact",
            classification: after === "reject" ? "real-defect" : "our-limitation",
            note: after === "reject"
                ? `accepted before, rejected by plotter.input.tikz; ${testCase.note ?? "no externals in this ecosystem"}`.trimEnd()
                : "rejected before, accepted by plotter.input.tikz",
        });
    }
    console.log(`${testCase.id}: before=${testCase.before} after=${after} codes=${found.codes.join(",")}`);
}

report.summary("vutex-tikz");
