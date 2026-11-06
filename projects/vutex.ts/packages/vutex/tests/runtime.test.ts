import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { createVuTeXRuntime } from "../src/runtime.js";
import type { VuTeXBindings } from "../src/types.js";

type FormulaCall = { source: string; optionsJson: string | undefined };
type ParseCall = { label: string; format: string; bytes: Uint8Array };

function fakeBindings(overrides: { measureResponse?: string; renderFormulaResponse?: string } = {}) {
    const calls = {
        measure: [] as FormulaCall[],
        renderFormula: [] as FormulaCall[],
        parse: [] as ParseCall[],
        summarize: [] as string[],
        reports: [] as string[],
    };
    const bindings: VuTeXBindings = {
        vutexVersion: () => "0.0.0-test",
        measureFormula: (source, optionsJson) => {
            calls.measure.push({ source, optionsJson });
            return overrides.measureResponse ?? "{}";
        },
        renderFormulaSvg: () => "",
        renderFormulaMathml: () => "",
        renderFormula: (source, optionsJson) => {
            calls.renderFormula.push({ source, optionsJson });
            return overrides.renderFormulaResponse ?? "{}";
        },
        parseDocument: (label, format, bytes) => {
            calls.parse.push({ label, format, bytes });
            return JSON.stringify({ ok: true, format, graph: { blocks: [] }, diagnostics: [] });
        },
        lowerDocument: (graphJson) => ({
            graphJson: () => graphJson,
            diagnosticsJson: () => "[]",
            footnotesJson: () => JSON.stringify({ references: 0, definitions: 0, notes: 0 }),
        }),
        buildDisplay: () => ({
            pageCount: () => 1,
            itemCount: () => 0,
            themeId: () => "baseline",
            mathFontId: () => undefined,
            diagnosticsJson: () => "[]",
            svg: () => "<svg/>",
            snapshotHtml: () => "<html/>",
            pdf: () => ({ pdf: new Uint8Array(), diagnosticsJson: "[]" }),
        }),
        buildFlow: () => ({ html: () => "<html/>" }),
        installMathFontFace: () => {},
        embedSvgFonts: (svg) => svg,
        subsetFace: () => null,
        summarizeDocument: (graphJson) => {
            calls.summarize.push(graphJson);
            return JSON.stringify({ blocks: 1 });
        },
        assembleReport: (operation) => {
            calls.reports.push(operation);
            return JSON.stringify({ operation });
        },
        themeFacts: () => JSON.stringify({ id: "baseline" }),
    };
    return { bindings, calls };
}

async function withTempDir<T>(run: (dir: string) => Promise<T>): Promise<T> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-runtime-"));
    try {
        return await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

test("version passes through the binding version string", () => {
    const { bindings } = fakeBindings();
    const runtime = createVuTeXRuntime(bindings);
    assert.equal(runtime.version(), "0.0.0-test");
});

test("render: explicit output wins over the manifest document output", () => {
    return withTempDir(async (dir) => {
        await writeFile(join(dir, "vutex.toml"), '[document]\noutput = "pdf"\n', "utf8");
        await writeFile(join(dir, "main.md"), "# Title\n", "utf8");
        const { bindings } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(join(dir, "main.md"), { output: "svg" });
        assert.equal(result.outputFormat, "svg");
    });
});

test("render: manifest document input and output are forwarded to the primitives", () => {
    return withTempDir(async (dir) => {
        await writeFile(
            join(dir, "vutex.toml"),
            '[document]\ninput = "markdown"\noutput = "html"\n\n[profile]\nname = "book"\n',
            "utf8",
        );
        await writeFile(join(dir, "ignored.tex"), "Hello.\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(join(dir, "ignored.tex"));

        assert.equal(result.outputFormat, "html");
        assert.equal(calls.parse[0]?.format, "markdown");
        assert.equal(calls.parse[0]?.label, join(dir, "ignored.tex"));
    });
});

test("render: without a manifest only output defaults to svg", () => {
    return withTempDir(async (dir) => {
        await writeFile(join(dir, "main.md"), "# Title\n", "utf8");
        const { bindings } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(join(dir, "main.md"));
        assert.equal(result.outputFormat, "svg");
    });
});

test("measureFormula encodes options and passes undefined through", () => {
    const { bindings, calls } = fakeBindings();
    const runtime = createVuTeXRuntime(bindings);

    runtime.measureFormula("x^2");
    runtime.measureFormula("x^2", {});
    runtime.measureFormula("x^2", { displayMode: "block" });

    assert.deepEqual(
        calls.measure.map((call) => call.optionsJson),
        [undefined, '{"displayMode":"inline"}', '{"displayMode":"block"}'],
    );
    assert.deepEqual(
        calls.measure.map((call) => call.source),
        ["x^2", "x^2", "x^2"],
    );
});

test("renderFormula decodes the binding json string into a typed result", () => {
    const raw = JSON.stringify({
        metrics: { width: 12, height: 3, depth: 1, baseline: 2 },
        svg: "<svg><path/></svg>",
        mathml: "<math/>",
        diagnostics: [
            { code: "WATEX_TEST", severity: "warning", message: "careful", start: 1, end: 2 },
        ],
    });
    const { bindings, calls } = fakeBindings({ renderFormulaResponse: raw });
    const runtime = createVuTeXRuntime(bindings);

    const result = runtime.renderFormula("a+b", { displayMode: "inline" });

    assert.equal(calls.renderFormula[0]?.optionsJson, '{"displayMode":"inline"}');
    assert.deepEqual(result, JSON.parse(raw));
});

test("inspect parses bytes and summarizes the graph", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "doc.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings);

        const result = await runtime.inspect(inputPath);

        assert.equal(result.inputFormat, "markdown");
        assert.equal(result.path, inputPath);
        assert.equal(calls.parse.length, 1);
        assert.equal(calls.summarize.length, 1);
        assert.equal(result.diagnostics.length, 0);
    });
});

test("inspect reports a missing input without parsing", async () => {
    await withTempDir(async (dir) => {
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings);

        const result = await runtime.inspect(join(dir, "absent.md"));

        assert.equal(calls.parse.length, 0);
        assert.equal(result.diagnostics[0]?.code, "VUTEX_INPUT_NOT_FOUND");
    });
});

test("packages delegates to the real package manager against the project root", () => {
    return withTempDir(async (dir) => {
        const { bindings } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });

        assert.deepEqual(await runtime.packages.list(), []);

        await runtime.packages.add("tinos", { slot: "font" });
        assert.deepEqual(await runtime.packages.list(), [
            { slot: "font", name: "tinos", version: "unresolved", source: "manifest" },
        ]);

        await runtime.packages.install();
        const locked = await runtime.packages.list();
        assert.equal(locked.length, 1);
        assert.equal(locked[0]?.slot, "font");
        assert.equal(locked[0]?.name, "tinos");
        assert.equal(locked[0]?.version, "builtin");
        assert.equal(locked[0]?.source, "builtin");
        assert.match(locked[0]?.artifactHash ?? "", /^sha256:[0-9a-f]{64}$/);

        const report = await runtime.packages.doctor();
        assert.equal(report.ok, true);

        await runtime.packages.remove("tinos");
        assert.deepEqual(await runtime.packages.list(), locked);
    });
});
