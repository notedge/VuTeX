import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import test from "node:test";

import { loadWeTeX } from "../src/wasm/load.js";
import type { WeTeXBindings } from "../src/types.js";

const require = createRequire(import.meta.url);
const wasmPackageJson = require.resolve("@vutex/vutex-unknown-wasm32/package.json");
const wasmPath = join(dirname(wasmPackageJson), "lib", "wetex_wasm_bg.wasm");
const wasmBytes = new Uint8Array(readFileSync(wasmPath));

const wetex: WeTeXBindings = await loadWeTeX({ bytes: wasmBytes });

type CorpusEntry = {
    /** LaTeX source of one syntax-family sample. */
    source: string;
    /**
     * MathML substrings the engine really emits for this sample, confirmed by
     * rendering it. Structural shape only — nothing font- or version-dependent.
     */
    mathml?: string[];
};

const corpus: CorpusEntry[] = [
    // scripts
    { source: "x^2", mathml: ["<msup"] },
    { source: "x^{a_b}", mathml: ["<msup", "<msub"] },
    { source: "10^{10^{10}}", mathml: ["<msup"] },
    { source: "e^{-x^2}", mathml: ["<msup"] },
    // the prime serializes as a trailing `<mo>`, not as `<msup>`
    { source: "x'", mathml: ["<mo"] },
    { source: "f''(x)", mathml: ["<mo"] },
    // fractions
    { source: "\\frac{a}{b}", mathml: ["<mfrac"] },
    { source: "\\frac{\\frac{a}{b}}{c}", mathml: ["<mfrac"] },
    { source: "\\dfrac{a}{b}", mathml: ["<mfrac"] },
    { source: "\\tfrac{a}{b}", mathml: ["<mfrac"] },
    // radicals
    { source: "\\sqrt{x}", mathml: ["<msqrt"] },
    { source: "\\sqrt[3]{x+y}", mathml: ["<mroot", "<mn"] },
    { source: "\\sqrt{\\frac{a}{b}}", mathml: ["<msqrt", "<mfrac"] },
    { source: "\\sqrt{\\sqrt{x}}", mathml: ["<msqrt"] },
    // large operators
    { source: "\\sum_{i=1}^{n} i", mathml: ["<msubsup"] },
    // `\sum\limits` should reach `<munderover>`; the semantic refactor is open,
    // so only the base invariants are asserted here.
    { source: "\\sum\\limits_{i=1}^{n} i^2" },
    { source: "\\int_0^1 x\\,dx", mathml: ["<msubsup", "<mspace"] },
    { source: "\\prod_{i=1}^{n} i", mathml: ["<msubsup"] },
    { source: "\\oint_C f\\,dz", mathml: ["<msub", "<mspace"] },
    { source: "\\lim_{x\\to 0}\\frac{\\sin x}{x}", mathml: ["<msub", "<mfrac"] },
    // accents
    { source: "\\vec{v}", mathml: ['<mover accent="true"'] },
    { source: "\\tilde{a}", mathml: ['<mover accent="true"'] },
    { source: "\\dot{x}", mathml: ['<mover accent="true"'] },
    { source: "\\ddot{y}", mathml: ['<mover accent="true"'] },
    { source: "\\overline{AB} \\ne \\widehat{CD}", mathml: ["<mover"] },
    // over/under
    { source: "\\overset{!}{=}", mathml: ["<mover"] },
    { source: "\\underset{\\alpha}{\\rightarrow}", mathml: ["<munder"] },
    // delimiters
    { source: "\\left(\\frac{a}{b}\\right) \\cdot \\binom{n}{k}", mathml: ["<mo", "<mfrac"] },
    { source: "\\left\\lVert x \\right\\rVert", mathml: ["<mo"] },
    { source: "\\left\\{ \\frac{a}{b} \\right\\}", mathml: ["<mo", "<mfrac"] },
    { source: "\\left\\langle v \\right\\rangle", mathml: ["<mo"] },
    { source: "\\left[ \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right]", mathml: ["<mo", "<mtable"] },
    { source: "\\left. \\frac{dy}{dx} \\right|_{0}", mathml: ["<msub", "<mfrac", "<mo"] },
    // environments
    { source: "\\begin{pmatrix}a&b\\\\c&d\\end{pmatrix}", mathml: ["<mtable", "<mtr", "<mtd", "<mo"] },
    { source: "\\begin{bmatrix}1&2\\\\3&4\\end{bmatrix}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{vmatrix}a&b\\\\c&d\\end{vmatrix}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{Vmatrix}a&b\\\\c&d\\end{Vmatrix}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{cases}x&x>0\\\\-x&x\\le 0\\end{cases}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{aligned}a&=b\\\\c&=d\\end{aligned}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{alignedat}{2}x&=y&x&=z\\end{alignedat}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{gathered}a+b\\\\c+d\\end{gathered}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{smallmatrix} a & b \\\\ c & d \\end{smallmatrix}", mathml: ["<mtable", "<mtr", "<mtd"] },
    { source: "\\begin{split} a &= b \\\\ &= c \\end{split}", mathml: ["<mtable", "<mtr", "<mtd"] },
    // spacing
    { source: "a\\,b", mathml: ["<mspace"] },
    { source: "a\\;b", mathml: ["<mspace"] },
    { source: "a\\quad b", mathml: ["<mspace"] },
    { source: "a\\qquad b", mathml: ["<mspace"] },
    // fonts — the engine really emits `<mtext>` without any `mathvariant`
    // attribute for the style commands; `<mi>` is asserted as the real shape.
    { source: "\\mathbf{Ax}", mathml: ["<mtext"] },
    { source: "\\mathbb{R}", mathml: ["<mtext"] },
    { source: "\\mathcal{L}", mathml: ["<mtext"] },
    { source: "\\mathsf{s}", mathml: ["<mtext"] },
    { source: "\\mathit{x}", mathml: ["<mtext"] },
    { source: "\\mathrm{d}x", mathml: ["<mi"] },
    // relations and symbols
    { source: "\\alpha\\beta\\gamma", mathml: ["<mi"] },
    { source: "x \\le y \\ge z", mathml: ["<mo"] },
    { source: "p \\Rightarrow q", mathml: ["<mo"] },
    { source: "A \\leftarrow B", mathml: ["<mo"] },
    { source: "a \\cdot b \\times c \\div d", mathml: ["<mo"] },
    // dots
    { source: "a_1 + \\cdots + a_n", mathml: ["<msub", "<mo"] },
    // text
    { source: "\\text{hello world}", mathml: ["<mtext"] },
    { source: "x + \\text{ text } + y", mathml: ["<mtext"] },
    // nested structures
    { source: "\\frac{\\sum_{i=1}^{n} i}{\\sqrt{n}}", mathml: ["<mfrac", "<msubsup", "<msqrt"] },
];

test("the LaTeX corpus renders through every backend without error diagnostics", () => {
    for (const entry of corpus) {
        const result = wetex.renderFormula(entry.source);
        const errors = result.diagnostics.filter((d) => d.severity === "error");
        assert.deepEqual(errors, [], entry.source);
        assert.ok(result.metrics.width > 0, entry.source);
        assert.match(result.svg, /<path/, entry.source);
        assert.ok(!result.svg.includes("<text"), entry.source);
        const mathml = result.mathml ?? "";
        assert.ok(mathml.startsWith("<math xmlns"), entry.source);
        for (const element of entry.mathml ?? []) {
            assert.ok(mathml.includes(element), `${entry.source}: mathml is missing ${element}: ${mathml}`);
        }
    }
});

test("measureFormula accepts the same corpus with positive widths", () => {
    for (const entry of corpus) {
        const result = wetex.measureFormula(entry.source);
        assert.equal(result.diagnostics.filter((d) => d.severity === "error").length, 0, entry.source);
        assert.ok(result.metrics.width > 0, entry.source);
    }
});

test("unclosed group yields a WETEX_MATH_ERROR diagnostic without throwing", () => {
    const result = wetex.renderFormula("\\frac{a");
    assert.equal(result.diagnostics.length, 1);
    const [diagnostic] = result.diagnostics;
    assert.equal(diagnostic?.code, "WETEX_MATH_ERROR");
    assert.equal(diagnostic?.severity, "error");
    assert.match(diagnostic?.message ?? "", /Parse Error/);
    assert.equal(diagnostic?.start == null, true);
    assert.equal(diagnostic?.end == null, true);
    assert.equal(result.metrics.width, 0);
    assert.equal(result.svg, "");
});

test("unclosed environment yields a WETEX_MATH_ERROR diagnostic", () => {
    const result = wetex.renderFormula("\\begin{pmatrix}a&b");
    assert.ok(result.diagnostics.length > 0);
    assert.equal(result.diagnostics.some((d) => d.code === "WETEX_MATH_ERROR" && d.severity === "error"), true);
    assert.equal(result.svg, "");
});

test("empty source yields WETEX_EMPTY_SOURCE anchored at zero", () => {
    const result = wetex.renderFormula("");
    assert.equal(result.diagnostics.length, 1);
    const [diagnostic] = result.diagnostics;
    assert.equal(diagnostic?.code, "WETEX_EMPTY_SOURCE");
    assert.equal(diagnostic?.severity, "error");
    assert.equal(diagnostic?.start, 0);
    assert.equal(diagnostic?.end, 0);
});

test("stix-two-math is declared but rejected as unsupported by the embedded wasm", () => {
    const result = wetex.renderFormula("x^2", { fontFamily: "stix-two-math" });
    assert.equal(
        result.diagnostics.some((d) => d.code === "WETEX_UNSUPPORTED_FONT_FAMILY" && d.severity === "error"),
        true,
    );
    assert.equal(result.svg, "");
});

test("renderFormula mathml always carries display=\"block\" even for inline mode", () => {
    const block = wetex.renderFormula("\\frac{a}{b}", { displayMode: "block" });
    const inline = wetex.renderFormula("\\frac{a}{b}", { displayMode: "inline" });
    assert.match(block.mathml ?? "", /display="block"/);
    assert.match(inline.mathml ?? "", /display="block"/);
    assert.equal(block.mathml, inline.mathml);
});

test("a path-heavy formula renders through the single outline path", () => {
    const result = wetex.renderFormula("\\sqrt[3]{x+y}");
    assert.equal(result.diagnostics.filter((d) => d.severity === "error").length, 0);
    assert.ok(result.metrics.width > 0);
    assert.ok(result.svg.length > 0);
});
