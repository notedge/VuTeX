import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
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
    "inspectDocument",
    "renderDocument",
] as const;

const native = loadBindings();

type FormulaPayload = {
    metrics: { width: number; height: number; depth: number; baseline: number };
    svg: string;
    mathml: string | null;
    diagnostics: unknown[];
};

type ReportPayload = {
    status?: string;
    diagnostics?: VuTeXDiagnostic[];
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

test("loadVuTeXNode exposes the seven documented binding functions", () => {
    const bindings = loadVuTeXNode();
    for (const name of BINDING_FUNCTIONS) {
        assert.equal(typeof bindings[name], "function");
    }
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

nativeTest("native inspectDocument reports the markdown structure", (bindings) => {
    return withTempDir(async (dir) => {
        const doc = join(dir, "doc.md");
        await writeFile(doc, "# Title\n\nInline $x^2$ math.\n", "utf8");
        const response = bindings.inspectDocument(doc);
        assert.equal(response.inputFormat, "markdown");
        assert.equal(response.path, doc);
        const report = JSON.parse(response.reportJson) as ReportPayload;
        assert.ok(Array.isArray(report.diagnostics));
    });
});

nativeTest("native renderDocument emits svg for a markdown document", (bindings) => {
    return withTempDir(async (dir) => {
        const doc = join(dir, "doc.md");
        await writeFile(doc, "# Title\n\nInline $x^2$ math.\n", "utf8");
        const response = bindings.renderDocument(doc, JSON.stringify({ output: "svg" }));
        assert.equal(response.exitCode, 0);
        assert.equal(response.outputFormat, "svg");
        assert.ok(response.svg && response.svg.startsWith("<svg "));
        const report = JSON.parse(response.reportJson) as ReportPayload;
        assert.equal(report.status, "success");
    });
});

nativeTest("native renderDocument fails with VUTEX_INPUT_NOT_FOUND for a missing path", (bindings) => {
    return withTempDir(async (dir) => {
        const missing = join(dir, "nope.md");
        const response = bindings.renderDocument(missing, JSON.stringify({ output: "svg" }));
        assert.equal(response.exitCode, 1);
        const report = JSON.parse(response.reportJson) as ReportPayload;
        const diagnostic = report.diagnostics?.find((item) => item.code === "VUTEX_INPUT_NOT_FOUND");
        assert.equal(diagnostic?.severity, "error");
        assert.equal(diagnostic?.stage, "render");
        assert.match(diagnostic?.message ?? "", /input path does not exist/);
    });
});

nativeTest("native renderDocument fails with VUTEX_OUTPUT_FORMAT_UNSUPPORTED for a bogus format", (bindings) => {
    return withTempDir(async (dir) => {
        const doc = join(dir, "doc.md");
        await writeFile(doc, "# Title\n", "utf8");
        const response = bindings.renderDocument(doc, JSON.stringify({ output: "bogus" }));
        assert.equal(response.exitCode, 1);
        const report = JSON.parse(response.reportJson) as ReportPayload;
        const diagnostic = report.diagnostics?.find((item) => item.code === "VUTEX_OUTPUT_FORMAT_UNSUPPORTED");
        assert.equal(diagnostic?.severity, "error");
        assert.match(diagnostic?.message ?? "", /unsupported output format: bogus/);
    });
});
