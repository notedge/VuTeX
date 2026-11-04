import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadMath } from "../lib/ours.mjs";
import { createReport } from "../lib/report.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const corpus = JSON.parse(readFileSync(resolve(here, "corpus.json"), "utf8"));
const require = createRequire(import.meta.url);

const MathJax = require("mathjax");
const MJ = await MathJax.init({
    loader: { load: ["input/tex", "adaptors/liteDOM", "startup", "output/svg"] },
    startup: { typeset: false },
    tex: { packages: ["base", "ams", "cases"] },
});
const katex = require("katex");

// ---------------------------------------------------------------------------
// Ours: WaTeX SVG groups carry `translate(x, y)` and glyphs carry
// `translate(a, b) scale(s, -s)`, where `s` is the glyph size in CSS pixels.
// The root group's y is the baseline, so a glyph's signed baseline offset is
// the sum of enclosing group offsets plus its own.
// ---------------------------------------------------------------------------
function parseOurs(svg, base) {
    const stack = [];
    const glyphs = [];
    let baseline = null;
    const tokens = svg.matchAll(/<g\b[^>]*>|<\/g>|<path\b[^>]*>/g);
    for (const token of tokens) {
        const raw = token[0];
        if (raw === "</g>") {
            stack.pop();
            continue;
        }
        if (raw.startsWith("<g")) {
            const match = raw.match(/transform="translate\(([-\d.eE+]+),?\s+([-\d.eE+]+)\)"/);
            const y = match ? Number(match[2]) : 0;
            stack.push(y);
            if (baseline === null) {
                baseline = y;
            }
            continue;
        }
        const translate = raw.match(/translate\(([-\d.eE+]+),?\s+([-\d.eE+]+)\)\s*scale\(([-\d.eE+]+)/);
        if (!translate) {
            continue;
        }
        const ownY = Number(translate[2]);
        const scale = Number(translate[3]);
        const absolute = stack.reduce((sum, value) => sum + value, 0) + ownY;
        glyphs.push({ offset: absolute - baseline, scale });
    }
    const shifts = glyphs.map((glyph) => -glyph.offset / base);
    const shift = shifts.length ? shifts.reduce((a, b) => (Math.abs(a) >= Math.abs(b) ? a : b)) : null;
    const scales = glyphs.map((glyph) => glyph.scale).filter((value) => value > 0);
    const ratio = scales.length ? Math.min(...scales) / Math.max(...scales) : null;
    return { shift, ratio };
}

function oursFeatures(render, base) {
    const svg = render.svg ?? "";
    const parsed = parseOurs(svg, base);
    return {
        baseline_height: render.metrics.height / base,
        baseline_depth: render.metrics.depth / base,
        script_shift: parsed.shift,
        script_size_ratio: parsed.ratio,
    };
}

// ---------------------------------------------------------------------------
// MathJax: one em is 1000 font units and one ex is 442. The viewBox is in font
// units, so `-min_y` is the baseline height and `min_y + height` the depth.
// Script groups carry `translate(x, y) scale(0.707)`, with y upward positive.
// ---------------------------------------------------------------------------
function serialize(node) {
    if (!node || typeof node !== "object") {
        return "";
    }
    if (node.kind === "text") {
        return (node.value ?? "").replace(/&/g, "&amp;");
    }
    const attrs = node.attributes
        ? ` ${Object.entries(node.attributes).map(([key, value]) =>
            `${key}="${String(value).replace(/&/g, "&amp;").replace(/"/g, "&quot;")}"`).join(" ")}`
        : "";
    const children = (node.children ?? []).map(serialize).join("");
    return `<${node.kind}${attrs}>${children}</${node.kind}>`;
}

function mathjaxFeatures(tex) {
    const container = MJ.tex2svg(tex, { display: false });
    const svgNode = container.children.find((child) => child.kind === "svg");
    const svg = serialize(svgNode);
    const box = svg.match(/viewBox="([^"]+)"/);
    if (!box) {
        return null;
    }
    const [minX, minY, width, height] = box[1].trim().split(/\s+/).map(Number);
    const groups = [...svg.matchAll(/transform="translate\(([-\d.eE+]+),?\s*([-\d.eE+]+)\)\s*scale\(([-\d.eE+]+)\)"/g)]
        .map((match) => ({ y: Number(match[2]), scale: Number(match[3]) }));
    const scripted = groups.filter((group) => group.scale < 0.99);
    const shifts = scripted.map((group) => group.y / 1000);
    const shift = shifts.length ? shifts.reduce((a, b) => (Math.abs(a) >= Math.abs(b) ? a : b)) : null;
    const ratio = scripted.length ? Math.min(...scripted.map((group) => group.scale)) : null;
    return {
        baseline_height: -minY / 1000,
        baseline_depth: (minY + height) / 1000,
        script_shift: shift,
        script_size_ratio: ratio,
        width: width / 1000,
    };
}

// ---------------------------------------------------------------------------
// KaTeX: no SVG backend, so geometry comes from inline em styles. The first
// `.base` strut carries the baseline height and its `vertical-align` the
// depth. A script shift is `pstrut height - |top|` inside `.msupsub`, and the
// glyph size comes from the `sizeN` class (`size6` is 1.0).
// ---------------------------------------------------------------------------
const KATEX_SIZES = { size1: 0.5, size2: 0.6, size3: 0.7, size4: 0.8, size5: 0.9, size6: 1.0 };

function katexFeatures(tex) {
    const html = katex.renderToString(tex, { output: "html", throwOnError: true });
    const struts = [...html.matchAll(/<span class="strut" style="height:([\d.]+)em;(?:vertical-align:-([\d.]+)em;)?"/g)]
        .map((match) => ({ height: Number(match[1]), depth: match[2] ? Number(match[2]) : 0 }));
    const maxHeight = struts.length ? Math.max(...struts.map((strut) => strut.height)) : null;
    const maxDepth = struts.length ? Math.max(...struts.map((strut) => strut.depth)) : null;
    let shift = null;
    let ratio = null;
    if (html.includes('class="msupsub"')) {
        const top = html.match(/style="top:-([\d.]+)em;/);
        const pstrut = top ? html.slice(html.indexOf(top[0])).match(/class="pstrut" style="height:([\d.]+)em;"/) : null;
        if (top && pstrut) {
            shift = Number(top[1]) - Number(pstrut[1]);
        }
    }
    const sizing = html.match(/class="sizing [^"]*(size\d)/);
    if (sizing) {
        ratio = KATEX_SIZES[sizing[1]];
    }
    return {
        baseline_height: maxHeight === null ? null : maxHeight - maxDepth,
        baseline_depth: maxDepth,
        script_shift: shift,
        script_size_ratio: ratio,
    };
}

// ---------------------------------------------------------------------------

const math = await loadMath();
const base = corpus.base_size_inline;
const report = createReport("math");

for (const testCase of corpus.cases) {
    const ours = oursFeatures(math.renderSvg(testCase.tex), base);
    const theirs = {
        katex: katexFeatures(testCase.tex),
        mathjax: mathjaxFeatures(testCase.tex),
    };
    for (const feature of testCase.features) {
        const reference = [
            { engine: "katex", value: theirs.katex ? theirs.katex[feature] : null },
            { engine: "mathjax", value: theirs.mathjax ? theirs.mathjax[feature] : null },
        ];
        const judgment = corpus.judgments[feature];
        report.compare({
            caseId: testCase.id,
            feature,
            ours: ours[feature],
            theirs: reference,
            tolerance: corpus.tolerances[feature],
            judgment,
            note: feature === "baseline_depth" && theirs.katex && theirs.katex.baseline_depth === null
                ? "KaTeX emits no depth for this case"
                : undefined,
        });
    }
    console.log(`${testCase.id} ${JSON.stringify(ours)}`);
}

report.summary("math");
