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

const corpus = [
    "x^2",
    "\\frac{a}{b}",
    "\\sqrt{x}",
    "\\sqrt[3]{x+y}",
    "\\sum_{i=1}^{n} i",
    "\\sum\\limits_{i=1}^{n} i^2",
    "\\int_0^1 x\\,dx",
    "\\begin{pmatrix}a&b\\\\c&d\\end{pmatrix}",
    "\\begin{bmatrix}1&2\\\\3&4\\end{bmatrix}",
    "\\begin{vmatrix}a&b\\\\c&d\\end{vmatrix}",
    "\\begin{Vmatrix}a&b\\\\c&d\\end{Vmatrix}",
    "\\begin{cases}x&x>0\\\\-x&x\\le 0\\end{cases}",
    "\\begin{aligned}a&=b\\\\c&=d\\end{aligned}",
    "\\begin{alignedat}{2}x&=y&x&=z\\end{alignedat}",
    "\\begin{gathered}a+b\\\\c+d\\end{gathered}",
    "\\text{hello world}",
    "\\alpha\\beta\\gamma",
    "\\lim_{x\\to 0}\\frac{\\sin x}{x}",
    "\\overline{AB} \\ne \\widehat{CD}",
    "\\left(\\frac{a}{b}\\right) \\cdot \\binom{n}{k}",
];

test("the LaTeX corpus renders through every backend without error diagnostics", (t) => {
    for (const source of corpus) {
        t.test(source, () => {
            const result = wetex.renderFormula(source);
            const errors = result.diagnostics.filter((d) => d.severity === "error");
            assert.deepEqual(errors, []);
            assert.ok(result.metrics.width > 0);
            assert.match(result.svg, /<path/);
            assert.ok(!result.svg.includes("<text"));
            assert.ok((result.mathml ?? "").startsWith("<math xmlns"));
        });
    }
});

test("measureFormula accepts the same corpus with positive widths", () => {
    for (const source of corpus) {
        const result = wetex.measureFormula(source);
        assert.equal(result.diagnostics.filter((d) => d.severity === "error").length, 0, source);
        assert.ok(result.metrics.width > 0, source);
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

test("outline:false still renders a usable result for a path-heavy formula", () => {
    const result = wetex.renderFormula("\\sqrt[3]{x+y}", { outline: false });
    assert.equal(result.diagnostics.filter((d) => d.severity === "error").length, 0);
    assert.ok(result.metrics.width > 0);
    assert.ok(result.svg.length > 0);
});
