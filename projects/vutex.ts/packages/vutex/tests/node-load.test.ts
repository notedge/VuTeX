import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { loadBindings } from "../src/cli/context.js";
import { loadVuTeXNode } from "../src/node/load.js";
import type { VuTeXBindings, VuTeXDiagnostic } from "../src/types.js";

const BINDING_FUNCTIONS = [
    "vutexVersion",
    "measureFormula",
    "renderFormulaSvg",
    "renderFormulaMathml",
    "renderFormula",
    "parseDocument",
    "lowerDocument",
    "buildDisplay",
    "buildFlow",
    "embedSvgFonts",
    "subsetFace",
    "summarizeDocument",
    "assembleReport",
    "themeFacts",
] as const;

const native = loadBindings();

type FormulaPayload = {
    metrics: { width: number; height: number; depth: number; baseline: number };
    svg: string;
    mathml: string | null;
    diagnostics: unknown[];
};

type ParsedPayload = {
    ok: boolean;
    format: string;
    graph?: unknown;
    diagnostics: VuTeXDiagnostic[];
};

async function withTempDir<T>(run: (dir: string) => Promise<T>): Promise<T> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-nodeload-"));
    try {
        return await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

function nativeTest(name: string, run: (bindings: VuTeXBindings) => void | Promise<void>): void {
    test(name, { skip: native ? false : "native bindings unavailable" }, async () => {
        assert.ok(native);
        await run(native);
    });
}

test("loadVuTeXNode exposes the documented primitive functions", { skip: native ? false : "native bindings unavailable" }, () => {
    const bindings = loadVuTeXNode();
    for (const name of BINDING_FUNCTIONS) {
        assert.equal(typeof bindings[name], "function");
    }
    assert.equal("renderDocument" in bindings, false, "the aggregate render export must not return");
    assert.equal("inspectDocument" in bindings, false, "the path-based inspect export must not return");
});

test("loadVuTeXNode rejects an unsupported platform key", () => {
    const original = Object.getOwnPropertyDescriptor(process, "platform");
    Object.defineProperty(process, "platform", { value: "freebsd" });
    try {
        assert.throws(() => loadVuTeXNode(), /Unsupported platform/);
    } finally {
        if (original) {
            Object.defineProperty(process, "platform", original);
        } else {
            delete (process as { platform?: string }).platform;
        }
    }
});

nativeTest("native vutexVersion reports a dotted version", (bindings) => {
    assert.match(bindings.vutexVersion(), /^\d+\.\d+\.\d+$/);
});

nativeTest("native measureFormula returns metrics with an empty svg", (bindings) => {
    const payload = JSON.parse(bindings.measureFormula("x^2 + y^2")) as FormulaPayload;
    assert.equal(typeof payload.metrics.width, "number");
    assert.ok(payload.metrics.width > 0);
    assert.equal(payload.svg, "");
    assert.deepEqual(payload.diagnostics, []);
});

nativeTest("native renderFormula returns a real svg document", (bindings) => {
    const payload = JSON.parse(bindings.renderFormula("x^2")) as FormulaPayload;
    assert.match(payload.svg, /^<svg /);
    assert.equal(typeof payload.metrics.width, "number");
});

nativeTest("native parseDocument turns bytes into a graph", (bindings) => {
    return withTempDir(async (dir) => {
        const doc = join(dir, "doc.md");
        await writeFile(doc, "# Title\n\nInline $x^2$ math.\n", "utf8");
        const bytes = await readFile(doc);
        const payload = JSON.parse(bindings.parseDocument(doc, "markdown", new Uint8Array(bytes))) as ParsedPayload;
        assert.equal(payload.ok, true);
        assert.equal(payload.format, "markdown");
        assert.ok(payload.graph);
        assert.deepEqual(payload.diagnostics, []);
    });
});

nativeTest("native parseDocument reports an unknown format as data", (bindings) => {
    const payload = JSON.parse(bindings.parseDocument("doc.bin", "unknown", new Uint8Array([1, 2, 3]))) as ParsedPayload;
    assert.equal(payload.ok, false);
    assert.equal(payload.diagnostics[0]?.code, "VUTEX_INPUT_FORMAT_UNSUPPORTED");
});

nativeTest("native scene emits svg with embedded fonts and no generic family", (bindings) => {
    return withTempDir(async (dir) => {
        const doc = join(dir, "doc.md");
        await writeFile(doc, "# Title\n\nHello world with `code`.\n", "utf8");
        const bytes = await readFile(doc);
        const parsed = JSON.parse(bindings.parseDocument(doc, "markdown", new Uint8Array(bytes))) as ParsedPayload;
        assert.ok(parsed.graph);
        const lowered = bindings.lowerDocument(JSON.stringify(parsed.graph), doc);
        const scene = bindings.buildDisplay(lowered, "baseline");
        const svg = scene.svg();
        assert.match(svg, /^<svg /);
        assert.ok(svg.includes("@font-face"), "scene svg has no embedded face");
        assert.equal(svg.includes('font-family="serif"'), false, "scene svg kept a generic family");
        assert.equal(svg.includes('font-family="monospace"'), false, "scene svg kept a generic family");
        assert.ok(scene.pageCount() >= 1);
    });
});

nativeTest("native flow emits html with embedded fonts", (bindings) => {
    return withTempDir(async (dir) => {
        const doc = join(dir, "doc.md");
        await writeFile(doc, "# Title\n\nHello world.\n", "utf8");
        const bytes = await readFile(doc);
        const parsed = JSON.parse(bindings.parseDocument(doc, "markdown", new Uint8Array(bytes))) as ParsedPayload;
        assert.ok(parsed.graph);
        const flow = bindings.buildFlow(JSON.stringify(parsed.graph), "baseline");
        const html = flow.html();
        assert.ok(html.includes("@font-face"));
        assert.equal(html.includes("font-family:serif"), false);
    });
});
