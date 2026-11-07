import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

import { createVuTeXRuntime } from "@vutex/vutex";
import type { VuTeXBindings } from "@vutex/vutex";

import { TYPST_DEFAULTS } from "../src/index.js";

function fakeBindings() {
    const calls = {
        parsedFormats: [] as string[],
    };
    const bindings: VuTeXBindings = {
        vutexVersion: () => "0.0.0-test",
        measureFormula: () => "{}",
        renderFormulaSvg: () => "",
        renderFormulaMathml: () => "",
        renderFormula: () => "{}",
        parseDocument: (_label, format) => {
            calls.parsedFormats.push(format);
            return JSON.stringify({ ok: true, format, graph: { blocks: [] }, diagnostics: [] });
        },
        lowerDocument: (graphJson) => ({
            graphJson: () => graphJson,
            diagnosticsJson: () => "[]",
            footnotesJson: () => JSON.stringify({ references: 0, definitions: 0, notes: 0 }),
        }),
        buildDisplay: (_lowered, theme) => ({
            pageCount: () => 1,
            itemCount: () => 0,
            themeId: () => theme ?? "baseline",
            mathFontId: () => undefined,
            diagnosticsJson: () => "[]",
            svg: () => "<svg/>",
            snapshotHtml: () => "<html/>",
            pdf: () => ({ pdf: new Uint8Array(), diagnosticsJson: "[]" }),
        }),
        buildFlow: () => ({
            html: () => "<html/>",
            htmlWithDiagnostics: () => ({ html: "<html/>", diagnosticsJson: "[]" }),
        }),
        installMathFontFace: () => {},
        mathFontTexParameterFamilies: () => ["latin-modern-math", "xits-math"],
        embedSvgFonts: (svg) => svg,
        subsetFace: () => null,
        summarizeDocument: () => JSON.stringify({ blocks: 1 }),
        assembleReport: () => JSON.stringify({ operation: "render" }),
        themeFacts: () => JSON.stringify({ id: "baseline" }),
    };
    return { bindings, calls };
}

async function withTempDir<T>(run: (dir: string) => Promise<T>): Promise<T> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-typst-"));
    try {
        return await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

test("typst defaults route a file without a recognised extension to the typst adapter", async () => {
    await withTempDir(async (dir) => {
        const source = join(dir, "chapter");
        await writeFile(source, "= Title\n\nBody.\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir, defaults: TYPST_DEFAULTS });
        const result = await runtime.render(source, { output: "svg" });

        assert.equal(result.diagnostics.filter((item) => item.severity === "error").length, 0);
        assert.deepEqual(calls.parsedFormats, ["typst"]);
    });
});

test("a render input option wins over the typst default", async () => {
    await withTempDir(async (dir) => {
        const source = join(dir, "chapter");
        await writeFile(source, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir, defaults: TYPST_DEFAULTS });
        await runtime.render(source, { output: "svg", input: "markdown" });

        assert.deepEqual(calls.parsedFormats, ["markdown"]);
    });
});

test("caller defaults override the typst value key by key", async () => {
    await withTempDir(async (dir) => {
        const source = join(dir, "chapter");
        await writeFile(source, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, {
            projectRoot: dir,
            defaults: { ...TYPST_DEFAULTS, input: "markdown" },
        });
        await runtime.render(source, { output: "svg" });

        assert.deepEqual(calls.parsedFormats, ["markdown"]);
    });
});
