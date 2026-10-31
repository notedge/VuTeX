import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { createVuTeXRuntime } from "../src/runtime.js";
import type { VuTeXBindings } from "../src/types.js";

type FormulaCall = { source: string; optionsJson: string | undefined };
type RenderCall = { inputPath: string; optionsJson: string | undefined };

type FakeOverrides = {
    renderResponse?: {
        exitCode: number;
        outputFormat: string;
        svg?: string;
        html?: string;
        pdf?: Uint8Array;
        reportJson: string;
    };
    inspectResponse?: { inputFormat: string; path: string; reportJson: string };
    measureResponse?: string;
    renderFormulaResponse?: string;
};

function fakeBindings(overrides: FakeOverrides = {}) {
    const calls = {
        measure: [] as FormulaCall[],
        renderFormula: [] as FormulaCall[],
        render: [] as RenderCall[],
        inspect: [] as string[],
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
        inspectDocument: (inputPath) => {
            calls.inspect.push(inputPath);
            return overrides.inspectResponse ?? { inputFormat: "markdown", path: inputPath, reportJson: "{}" };
        },
        renderDocument: (inputPath, optionsJson) => {
            calls.render.push({ inputPath, optionsJson });
            return overrides.renderResponse ?? { exitCode: 0, outputFormat: "svg", reportJson: "{}" };
        },
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

test("render payload: explicit output wins over the manifest document output", () => {
    return withTempDir(async (dir) => {
        await writeFile(join(dir, "vutex.toml"), '[document]\noutput = "pdf"\n', "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render("main.tex", { output: "svg" });
        const payload = JSON.parse(calls.render[0]?.optionsJson ?? "{}") as Record<string, unknown>;
        assert.equal(payload.output, "svg");
    });
});

test("render payload: manifest document output and profile and input are forwarded", () => {
    return withTempDir(async (dir) => {
        await writeFile(
            join(dir, "vutex.toml"),
            '[document]\ninput = "src/main.md"\noutput = "html"\n\n[profile]\nname = "book"\n',
            "utf8",
        );
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render("ignored.tex");
        const payload = JSON.parse(calls.render[0]?.optionsJson ?? "{}") as Record<string, unknown>;
        assert.equal(payload.output, "html");
        assert.equal(payload.profile, "book");
        assert.equal(payload.input, "src/main.md");
        assert.equal(calls.render[0]?.inputPath, "ignored.tex");
    });
});

test("render payload: explicit profile overrides the manifest profile", () => {
    return withTempDir(async (dir) => {
        await writeFile(join(dir, "vutex.toml"), '[profile]\nname = "book"\n', "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render("main.tex", { profile: "slide" });
        const payload = JSON.parse(calls.render[0]?.optionsJson ?? "{}") as Record<string, unknown>;
        assert.equal(payload.profile, "slide");
    });
});

test("render payload: without a manifest only output defaults to svg", () => {
    return withTempDir(async (dir) => {
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render("main.tex");
        assert.equal(calls.render[0]?.optionsJson, '{"output":"svg"}');
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
            { code: "WETEX_TEST", severity: "warning", message: "careful", start: 1, end: 2 },
        ],
    });
    const { bindings, calls } = fakeBindings({ renderFormulaResponse: raw });
    const runtime = createVuTeXRuntime(bindings);

    const result = runtime.renderFormula("a+b", { displayMode: "inline" });

    assert.equal(calls.renderFormula[0]?.optionsJson, '{"displayMode":"inline"}');
    assert.deepEqual(result, JSON.parse(raw));
});

test("inspect forwards the binding response and ignores its options", async () => {
    const reportJson = JSON.stringify({
        diagnostics: [{ code: "VUTEX_TEST", severity: "info", message: "hello", stage: "inspect" }],
    });
    const { bindings, calls } = fakeBindings({
        inspectResponse: { inputFormat: "markdown", path: "doc.md", reportJson },
    });
    const runtime = createVuTeXRuntime(bindings);

    const result = await runtime.inspect("doc.md", { input: "other.md" });

    assert.equal(calls.inspect[0], "doc.md");
    assert.equal(result.inputFormat, "markdown");
    assert.equal(result.path, "doc.md");
    assert.equal(result.reportJson, reportJson);
    assert.equal(result.diagnostics.length, 1);
    assert.equal(result.diagnostics[0]?.code, "VUTEX_TEST");
});

test("malformed report json yields an empty diagnostics array", () => {
    return withTempDir(async (dir) => {
        const { bindings } = fakeBindings({
            renderResponse: { exitCode: 1, outputFormat: "svg", reportJson: "not-json" },
        });
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex");
        assert.deepEqual(result.diagnostics, []);
    });
});

test("packages delegates to the real package manager against the project root", () => {
    return withTempDir(async (dir) => {
        const { bindings } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });

        assert.deepEqual(await runtime.packages.list(), []);

        await runtime.packages.add("font-libertinus", { version: "^0.1.0" });
        assert.deepEqual(await runtime.packages.list(), [
            { name: "font-libertinus", version: "^0.1.0", source: "manifest" },
        ]);

        await runtime.packages.install();
        const locked = await runtime.packages.list();
        assert.deepEqual(locked, [
            { name: "font-libertinus", version: "^0.1.0", source: "@vutex/font-libertinus" },
        ]);

        const report = await runtime.packages.doctor();
        assert.equal(report.ok, true);

        await runtime.packages.remove("font-libertinus");
        assert.deepEqual(await runtime.packages.list(), [
            { name: "font-libertinus", version: "^0.1.0", source: "@vutex/font-libertinus" },
        ]);
    });
});
