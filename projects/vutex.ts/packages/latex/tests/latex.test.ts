import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

import { createVuTeXRuntime } from "@vutex/vutex";
import type { VuTeXBindings } from "@vutex/vutex";

import { LATEX_DEFAULTS } from "../src/index.js";

type DisplayCall = { theme: string | undefined; mathFont: string | undefined };
type ReportCall = { extra: string | undefined };

function fakeBindings() {
    const calls = {
        display: [] as DisplayCall[],
        reports: [] as ReportCall[],
    };
    const bindings: VuTeXBindings = {
        vutexVersion: () => "0.0.0-test",
        measureFormula: () => "{}",
        renderFormulaSvg: () => "",
        renderFormulaMathml: () => "",
        renderFormula: () => "{}",
        parseDocument: (_label, format) =>
            JSON.stringify({ ok: true, format, graph: { blocks: [] }, diagnostics: [] }),
        lowerDocument: (graphJson) => ({
            graphJson: () => graphJson,
            diagnosticsJson: () => "[]",
            footnotesJson: () => JSON.stringify({ references: 0, definitions: 0, notes: 0 }),
        }),
        buildDisplay: (_lowered, theme, mathFont) => {
            calls.display.push({ theme, mathFont });
            return {
                pageCount: () => 1,
                itemCount: () => 0,
                themeId: () => theme ?? "baseline",
                mathFontId: () => mathFont,
                diagnosticsJson: () => "[]",
                svg: () => "<svg/>",
                snapshotHtml: () => "<html/>",
                pdf: () => ({ pdf: new Uint8Array(), diagnosticsJson: "[]" }),
            };
        },
        buildFlow: () => ({ html: () => "<html/>" }),
        installMathFontFace: () => {},
        mathFontTexParameterFamilies: () => ["latin-modern-math", "xits-math"],
        embedSvgFonts: (svg) => svg,
        subsetFace: () => null,
        summarizeDocument: () => JSON.stringify({ blocks: 1 }),
        assembleReport: (_operation, _inputs, _diagnostics, _content, extra) => {
            calls.reports.push({ extra });
            return JSON.stringify({ operation: "render" });
        },
        themeFacts: () => JSON.stringify({ id: "traditional" }),
    };
    return { bindings, calls };
}

async function withTempDir<T>(run: (dir: string) => Promise<T>): Promise<T> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-latex-"));
    try {
        return await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

test("latex defaults pick the traditional theme and xits-math when the manifest picks nothing", async () => {
    await withTempDir(async (dir) => {
        await writeFile(join(dir, "chapter.md"), "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir, defaults: LATEX_DEFAULTS });
        const result = await runtime.render(join(dir, "chapter.md"), { output: "svg" });

        assert.equal(result.diagnostics.filter((item) => item.severity === "error").length, 0);
        assert.deepEqual(calls.display, [{ theme: "traditional", mathFont: "xits-math" }]);
        const extra = JSON.parse(calls.reports[0]?.extra ?? "{}") as { theme?: string; mathFont?: string };
        assert.equal(extra.theme, "traditional");
        assert.equal(extra.mathFont, "xits-math");
    });
});

test("a manifest theme slot wins over the latex defaults", async () => {
    await withTempDir(async (dir) => {
        await writeFile(join(dir, "vutex.toml"), '[packages]\ntheme = "baseline"\n', "utf8");
        await writeFile(join(dir, "chapter.md"), "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir, defaults: LATEX_DEFAULTS });
        await runtime.packages.install();
        const result = await runtime.render(join(dir, "chapter.md"), { output: "svg" });
        assert.equal(result.diagnostics.filter((item) => item.severity === "error").length, 0);
        assert.equal(calls.display[0]?.theme, "baseline");
        assert.equal(calls.display[0]?.mathFont, "xits-math");
    });
});
