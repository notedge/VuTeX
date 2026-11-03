//! Real-WASM vs native parity: the shipped wasm artifact and the Node-API
//! binary are compiled from the same Rust sources, so every formula must
//! measure and render identically through both.  Any drift means the wasm
//! artifact is stale or one backend diverged.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import test from "node:test";

import { loadVuTeXNode } from "@vutex/vutex/node";

import { loadWaTeX } from "../src/wasm/load.js";
import type { FormulaRender } from "../src/types.js";

// Node has no fetch for the web-target artifact, so instantiate from the
// shipped bytes — exactly the path `load.test.ts` exercises.
const require = createRequire(import.meta.url);
const wasmPackageJson = require.resolve("@vutex/vutex-unknown-wasm32/package.json");
const wasmBytes = new Uint8Array(readFileSync(join(dirname(wasmPackageJson), "lib", "watex_wasm_bg.wasm")));
const wasm = await loadWaTeX({ bytes: wasmBytes });
const native = loadVuTeXNode();

const CORPUS: readonly string[] = [
    "x + y",
    "\\frac{a+b}{c+d}",
    "\\sqrt[3]{x^2}",
    "\\sum_{i=1}^{n} i",
    "\\int_0^1 x^2\\,dx",
    "\\left(\\frac{a}{b}\\right)",
    "\\vec{x} + \\hat{y}",
    "\\begin{matrix} a & b \\\\ c & d \\end{matrix}",
    "\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}",
    "e^{-x^2} + 10^{10}",
    "\\mathbb{R} \\times \\mathcal{L}",
    "\\text{loss } \\alpha \\le \\beta",
];

function nativeRender(raw: string): FormulaRender {
    return JSON.parse(raw) as FormulaRender;
}

test("the real wasm artifact measures every corpus formula exactly like native", () => {
    for (const source of CORPUS) {
        const fromWasm = wasm.measureFormula(source);
        const fromNative = nativeRender(native.measureFormula(source));
        assert.deepEqual(fromWasm.metrics, fromNative.metrics, `${source}: wasm metrics must equal native metrics`);
        assert.deepEqual(fromWasm.diagnostics, fromNative.diagnostics, `${source}: diagnostics must agree`);
    }
});

test("the real wasm artifact renders the same svg and mathml as native", () => {
    for (const source of CORPUS) {
        const fromWasm = wasm.renderSvg(source);
        const fromNative = nativeRender(native.renderFormulaSvg(source));
        assert.equal(fromWasm.svg, fromNative.svg, `${source}: svg markup must be identical`);

        const wasmMathml = wasm.renderMathml(source);
        const nativeMathml = nativeRender(native.renderFormulaMathml(source));
        assert.equal(wasmMathml.mathml, nativeMathml.mathml, `${source}: mathml must be identical`);
    }
});

test("render options reach both backends identically", () => {
    const cases = [
        { displayMode: "block" as const },
        { fontFamily: "lm-math" as const },
        { fontSize: 20 },
    ];
    for (const options of cases) {
        const source = "\\frac{a}{b}";
        const fromWasm = wasm.measureFormula(source, options);
        const fromNative = nativeRender(native.measureFormula(source, JSON.stringify(options)));
        assert.deepEqual(fromWasm.metrics, fromNative.metrics, `${JSON.stringify(options)}: metrics must agree`);
    }
    // The options must actually change the measurement, otherwise the parity
    // above would hold vacuously.
    const inline = wasm.measureFormula("\\frac{a}{b}");
    const block = wasm.measureFormula("\\frac{a}{b}", { displayMode: "block" });
    assert.notDeepEqual(inline.metrics, block.metrics, "display mode must change the metrics");
});

test("strict font conformance renders the corpus cleanly through both backends", () => {
    // Gate G5 on the host surface: the embedded font publishes every fact the
    // corpus consumes, so `strict` must render identically to the default
    // instead of failing — on the real wasm artifact and on native alike.
    const options = { fontConformance: "strict" as const };
    for (const source of CORPUS) {
        const fromWasm = wasm.renderFormula(source, options);
        const fromNative = nativeRender(native.renderFormula(source, JSON.stringify(options)));
        assert.deepEqual(fromWasm.metrics, fromNative.metrics, `${source}: strict metrics must agree`);
        assert.equal(fromWasm.svg, fromNative.svg, `${source}: strict svg must be identical`);
        const conformanceDiagnostics = fromWasm.diagnostics.filter((d) => d.code.startsWith("vutex.math.conformance."));
        assert.deepEqual(conformanceDiagnostics, [], `${source}: strict mode must consume only font-published facts`);
    }
});
