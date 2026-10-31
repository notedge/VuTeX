import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";

import { loadWeTeX } from "../src/wasm/load.js";

const require = createRequire(import.meta.url);
const wasmPackageJson = require.resolve("@vutex/vutex-unknown-wasm32/package.json");
const wasmPath = join(dirname(wasmPackageJson), "lib", "wetex_wasm_bg.wasm");
const wasmBytes = new Uint8Array(readFileSync(wasmPath));

const wetex = await loadWeTeX({ bytes: wasmBytes });

test("loadWeTeX({bytes}) instantiates and reports a semver version", () => {
    assert.match(wetex.wetexVersion(), /^\d+\.\d+\.\d+$/);
});

test("wrapped results decode to the documented shape with null mathml outside the mathml backends", () => {
    const results = {
        measure: wetex.measureFormula("x^2"),
        svg: wetex.renderSvg("x^2"),
        html: wetex.renderHtml("x^2"),
        mathml: wetex.renderMathml("x^2"),
        formula: wetex.renderFormula("x^2"),
    };

    for (const [name, result] of Object.entries(results)) {
        for (const key of ["metrics", "svg", "html", "diagnostics"]) {
            assert.ok(key in result, `${name} result is missing the ${key} field`);
        }
        assert.equal(typeof result.metrics.width, "number");
        assert.equal(typeof result.metrics.height, "number");
        assert.equal(typeof result.metrics.depth, "number");
        assert.equal(typeof result.metrics.baseline, "number");
        assert.equal(typeof result.svg, "string");
        assert.ok(Array.isArray(result.diagnostics));
    }

    assert.equal(results.measure.mathml == null, true);
    assert.equal(results.svg.mathml == null, true);
    assert.equal(results.html.mathml == null, true);
    assert.ok((results.mathml.mathml ?? "").startsWith("<math xmlns"));

    assert.equal(results.measure.svg, "");
    assert.match(results.svg.svg, /<path/);
    assert.match(results.formula.svg, /<path/);
    assert.ok((results.formula.mathml ?? "").startsWith("<math xmlns"));
});

test("renderHtml fills the html field and leaves svg empty", () => {
    const result = wetex.renderHtml("x^2");
    assert.match(result.html, /^<span class="wetex-html"/);
    assert.equal(result.svg, "");
});

test("renderMathml fills the mathml field and leaves svg and html empty", () => {
    const result = wetex.renderMathml("x^2");
    assert.ok((result.mathml ?? "").startsWith("<math xmlns"));
    assert.equal(result.svg, "");
    assert.equal(result.html, "");
});

test("encoded options round-trip: block layout is wider than inline", () => {
    const inline = wetex.measureFormula("\\frac{a}{b}", { displayMode: "inline" });
    const block = wetex.measureFormula("\\frac{a}{b}", { displayMode: "block" });
    assert.ok(block.metrics.width > inline.metrics.width);
});

test("omitted options match the inline default", () => {
    const omitted = wetex.measureFormula("\\frac{a}{b}");
    const inline = wetex.measureFormula("\\frac{a}{b}", { displayMode: "inline" });
    assert.deepEqual(omitted.metrics, inline.metrics);
});

test("fontSize scales the measured width linearly", () => {
    const large = wetex.measureFormula("\\frac{a}{b}", { fontSize: 20 });
    const small = wetex.measureFormula("\\frac{a}{b}", { fontSize: 10 });
    const ratio = large.metrics.width / small.metrics.width;
    assert.ok(Math.abs(ratio - 2) < 0.05, `expected ratio near 2, got ${ratio}`);
});

test("lm-math and xits-math measure and render differently", () => {
    const lm = wetex.renderSvg("\\frac{a}{b}", { fontFamily: "lm-math" });
    const xits = wetex.renderSvg("\\frac{a}{b}", { fontFamily: "xits-math" });
    assert.notEqual(lm.svg, xits.svg);
    assert.ok(lm.svg.length > 0);
    assert.equal(lm.diagnostics.length, 0);
    assert.equal(xits.diagnostics.length, 0);

    const lmMetrics = wetex.measureFormula("\\frac{a}{b}", { fontFamily: "lm-math" });
    const xitsMetrics = wetex.measureFormula("\\frac{a}{b}", { fontFamily: "xits-math" });
    assert.notEqual(lmMetrics.metrics.width, xitsMetrics.metrics.width);
});

const packageDir = dirname(dirname(fileURLToPath(import.meta.url)));
const srcIndexUrl = pathToFileURL(join(packageDir, "src", "index.ts")).href;

const snippet = `
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [moduleUrl, scenario, wasmPath] = process.argv.slice(2);
const mod = await import(moduleUrl);
const bytes = new Uint8Array(readFileSync(wasmPath));
const url = pathToFileURL(wasmPath);
try {
    let bindings;
    if (scenario === "url-only") {
        bindings = await mod.loadWeTeX({ url });
    } else if (scenario === "bytes-url") {
        bindings = await mod.loadWeTeX({ bytes, url });
    } else {
        bindings = await mod.loadWeTeX();
    }
    console.log(JSON.stringify({ outcome: "ok", version: bindings.wetexVersion() }));
} catch (error) {
    console.log(JSON.stringify({ outcome: "threw", message: String(error) }));
}
`;

function runColdScenario(scenario: string): { outcome: string; version?: string; message?: string } {
    const dir = mkdtempSync(join(tmpdir(), "wetex-load-"));
    const snippetPath = join(dir, "snippet.mjs");
    writeFileSync(snippetPath, snippet, "utf8");
    try {
        const child = spawnSync(
            process.execPath,
            ["--import", "tsx", snippetPath, srcIndexUrl, scenario, wasmPath],
            { cwd: packageDir, encoding: "utf8", timeout: 120_000 },
        );
        assert.equal(child.status, 0, `cold process for ${scenario} failed: ${child.stderr}`);
        const line = child.stdout.trim().split(/\r?\n/).at(-1) ?? "";
        return JSON.parse(line) as { outcome: string; version?: string; message?: string };
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
}

test("cold process: url-only loading throws in Node while bytes take precedence", () => {
    const urlOnly = runColdScenario("url-only");
    assert.equal(urlOnly.outcome, "threw");

    const bytesUrl = runColdScenario("bytes-url");
    assert.equal(bytesUrl.outcome, "ok");
    assert.equal(bytesUrl.version, wetex.wetexVersion());
});

test("cold process: loadWeTeX() without bytes also falls back to fetch and throws in Node", () => {
    const cold = runColdScenario("default");
    assert.equal(cold.outcome, "threw");
    assert.match(cold.message ?? "", /fetch failed/);
});
