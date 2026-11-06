import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import test from "node:test";

import { writePdfOutput } from "../src/cli/commands/render.js";
import { PackageManager } from "../src/packages/manager.js";
import { createVuTeXRuntime } from "../src/runtime.js";
import type { RenderDocumentResult, VuTeXBindings } from "../src/types.js";

type ParseCall = { label: string; format: string; bytes: Uint8Array };
type ReportCall = { operation: string; inputs: string; diagnostics: string; output?: string; extra?: string };

type Overrides = {
    parsed?: string;
    svg?: string;
    html?: string;
    pdf?: Uint8Array;
    lowerDiagnostics?: unknown[];
    sceneDiagnostics?: unknown[];
};

function fakeBindings(overrides: Overrides = {}) {
    const calls = {
        parse: [] as ParseCall[],
        lower: [] as string[],
        display: [] as Array<string | undefined>,
        mathFont: [] as Array<string | undefined>,
        template: [] as Array<unknown>,
        mathFaces: [] as Array<{ name: string; hash: string; bytes: Uint8Array }>,
        flow: [] as Array<string | undefined>,
        pdfSettings: [] as Array<string | undefined>,
        reports: [] as ReportCall[],
    };
    const bindings: VuTeXBindings = {
        vutexVersion: () => "0.0.0-test",
        measureFormula: () => "{}",
        renderFormulaSvg: () => "",
        renderFormulaMathml: () => "",
        renderFormula: () => "{}",
        parseDocument: (label, format, bytes) => {
            calls.parse.push({ label, format, bytes });
            return overrides.parsed
                ?? JSON.stringify({ ok: true, format, graph: { blocks: [] }, diagnostics: [] });
        },
        lowerDocument: (graphJson, label) => {
            calls.lower.push(label);
            return {
                graphJson: () => graphJson,
                diagnosticsJson: () => JSON.stringify(overrides.lowerDiagnostics ?? []),
                footnotesJson: () => JSON.stringify({ references: 0, definitions: 0, notes: 0 }),
            };
        },
        buildDisplay: (_lowered, theme, mathFont, template) => {
            calls.display.push(theme);
            calls.mathFont.push(mathFont);
            calls.template.push(template);
            return {
                pageCount: () => 1,
                itemCount: () => 2,
                themeId: () => theme ?? "baseline",
                mathFontId: () => mathFont,
                diagnosticsJson: () => JSON.stringify(overrides.sceneDiagnostics ?? []),
                svg: () => overrides.svg ?? "<svg/>",
                snapshotHtml: () => overrides.html ?? "<html/>",
                pdf: (settings) => {
                    calls.pdfSettings.push(settings);
                    return { pdf: overrides.pdf ?? new Uint8Array([1, 2, 3]), diagnosticsJson: "[]" };
                },
            };
        },
        buildFlow: (_graphJson, theme) => {
            calls.flow.push(theme);
            return { html: () => overrides.html ?? "<html/>" };
        },
        installMathFontFace: (name, hash, bytes) => {
            calls.mathFaces.push({ name, hash, bytes });
        },
        embedSvgFonts: (svg) => svg,
        subsetFace: () => null,
        summarizeDocument: () => JSON.stringify({ blocks: 1 }),
        assembleReport: (operation, inputs, diagnostics, output, extra) => {
            calls.reports.push({ operation, inputs, diagnostics, output, extra });
            return JSON.stringify({ operation });
        },
        themeFacts: () => JSON.stringify({ id: "baseline" }),
    };
    return { bindings, calls };
}

async function withTempDir<T>(run: (dir: string) => Promise<T>): Promise<T> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        return await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

test("render defaults output to svg, reads bytes, and emits through the scene", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.equal(calls.parse.length, 1);
        assert.equal(calls.parse[0]?.format, "markdown");
        assert.ok(calls.parse[0]?.bytes instanceof Uint8Array);
        assert.equal(calls.display.length, 1);
        assert.deepEqual(result.svg, "<svg/>");
        assert.equal(result.html, undefined);
        assert.equal(result.outputFormat, "svg");
        assert.equal(result.diagnostics.length, 0);
    });
});

test("render forwards the theme to the scene and never assembles fonts itself", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render(inputPath, { theme: "traditional" });

        assert.deepEqual(calls.display, ["traditional"]);
        assert.ok(calls.reports.every((report) => !report.operation.includes("font")));
    });
});

test("render forwards html output to the reflow scene instead of the display scene", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings({ html: "<html data-reflow/>" });
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath, { output: "html" });

        assert.equal(calls.flow.length, 1);
        assert.equal(calls.display.length, 0);
        assert.equal(result.html, "<html data-reflow/>");
        assert.equal(result.svg, undefined);
    });
});

test("render emits snapshot html from the display scene", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings({ html: "<html data-snapshot/>" });
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath, { output: "snapshot" });

        assert.equal(calls.display.length, 1);
        assert.equal(calls.flow.length, 0);
        assert.equal(result.outputFormat, "snapshot");
        assert.equal(result.html, "<html data-snapshot/>");
    });
});

test("render passes resource settings to the pdf primitive and keeps bytes out of json", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const pdf = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37]);
        const { bindings, calls } = fakeBindings({ pdf });
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath, {
            output: "pdf",
            resources: { "https://example.test/a.png": "AAAA" },
        });

        const settings = JSON.parse(calls.pdfSettings[0] ?? "{}") as Record<string, unknown>;
        assert.equal(settings.base_dir, dir);
        assert.deepEqual(settings.resources, { "https://example.test/a.png": "AAAA" });
        assert.deepEqual(result.pdf, pdf);
        assert.equal(result.svg, undefined);
    });
});

test("render reports a missing input without calling the parser", async () => {
    await withTempDir(async (dir) => {
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(join(dir, "absent.md"));

        assert.equal(calls.parse.length, 0);
        assert.equal(result.diagnostics.length, 1);
        assert.equal(result.diagnostics[0]?.code, "VUTEX_INPUT_NOT_FOUND");
        assert.equal(result.diagnostics[0]?.severity, "error");
    });
});

test("render propagates parse diagnostics and does not lay out a failed import", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "body", "utf8");
        const parsed = JSON.stringify({
            ok: false,
            format: "markdown",
            diagnostics: [{ code: "VUTEX_IMPORT_FAILED", severity: "error", message: "bad source", stage: "import" }],
        });
        const { bindings, calls } = fakeBindings({ parsed });
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.equal(calls.lower.length, 0);
        assert.equal(result.diagnostics[0]?.code, "VUTEX_IMPORT_FAILED");
    });
});

test("render keeps unknown output formats intact as a diagnostic", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath, { output: "bogus" });

        assert.equal(calls.lower.length, 0);
        assert.equal(calls.display.length, 0);
        assert.equal(result.outputFormat, "bogus");
        assert.equal(result.diagnostics[0]?.code, "VUTEX_OUTPUT_FORMAT_UNSUPPORTED");
    });
});

test("render merges lowering and layout diagnostics in order", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings } = fakeBindings({
            lowerDiagnostics: [{ code: "LOWER", severity: "warning", message: "lowered" }],
            sceneDiagnostics: [{ code: "LAYOUT", severity: "error", message: "overflow" }],
        });
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.deepEqual(result.diagnostics.map((item) => item.code), ["LOWER", "LAYOUT"]);
    });
});

test("render honors an explicit input override over the file extension", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.data");
        await writeFile(inputPath, "# Title\n", "utf8");
        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render(inputPath, { input: "markdown" });

        assert.equal(calls.parse[0]?.format, "markdown");
    });
});

test("render applies the declared theme and math-font slots", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        await writeFile(join(dir, "vutex.toml"), '[packages]\nmath-font = "xits-math"\ntheme = "editorial"\n', "utf8");
        await new PackageManager(dir).install();

        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.deepEqual(calls.display, ["editorial"]);
        assert.deepEqual(calls.mathFont, ["xits-math"]);
        assert.deepEqual(result.packages?.map((item) => item.slot), ["math-font", "theme"]);
        const extra = calls.reports.at(-1)?.extra ?? "";
        assert.ok(extra.includes('"theme":"editorial"'), extra);
        assert.ok(extra.includes('"mathFont":"xits-math"'), extra);
        assert.equal(result.diagnostics.filter((item) => item.severity === "error").length, 0);
    });
});

test("render registers a downloaded math face before laying out", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const resource = join(dir, "resources", "math");
        await mkdir(resource, { recursive: true });
        await writeFile(join(resource, "math.otf"), "font payload", "utf8");
        await writeFile(join(resource, "math-font.json"), JSON.stringify({ file: "math.otf" }), "utf8");
        await writeFile(join(dir, "vutex.toml"), '[packages]\nmath-font = "./resources/math"\n', "utf8");
        await new PackageManager(dir).install();

        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.equal(calls.mathFaces.length, 1);
        assert.equal(calls.mathFaces[0]?.name, "resources/math");
        assert.ok((calls.mathFaces[0]?.hash ?? "").startsWith("sha256:"));
        assert.equal(new TextDecoder().decode(calls.mathFaces[0]?.bytes), "font payload");
        assert.deepEqual(calls.mathFont, ["resources/math"]);
        assert.equal(result.diagnostics.filter((item) => item.severity === "error").length, 0);
    });
});

test("render fails closed when a math font package declares no face file", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const resource = join(dir, "resources", "math");
        await mkdir(resource, { recursive: true });
        await writeFile(join(resource, "math.otf"), "font payload", "utf8");
        await writeFile(join(dir, "vutex.toml"), '[packages]\nmath-font = "./resources/math"\n', "utf8");
        await new PackageManager(dir).install();

        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.equal(calls.display.length, 0);
        assert.equal(calls.mathFaces.length, 0);
        assert.ok(result.diagnostics.some((item) => item.code === "VUTEX_PACKAGE_MATH_FONT_INVALID"));
    });
});

test("render refuses to install a math face for reflowed html", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const resource = join(dir, "resources", "math");
        await mkdir(resource, { recursive: true });
        await writeFile(join(resource, "math.otf"), "font payload", "utf8");
        await writeFile(join(resource, "math-font.json"), JSON.stringify({ file: "math.otf" }), "utf8");
        await writeFile(join(dir, "vutex.toml"), '[packages]\nmath-font = "./resources/math"\n', "utf8");
        await new PackageManager(dir).install();

        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath, { output: "html" });

        assert.equal(calls.mathFaces.length, 0);
        assert.equal(calls.flow.length, 1);
        assert.ok(result.diagnostics.some((item) => item.code === "VUTEX_PACKAGE_MATH_FONT_INACTIVE"));
    });
});

test("render applies the declared template page geometry", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const template = join(dir, "templates", "article");
        await mkdir(template, { recursive: true });
        await writeFile(
            join(template, "template.json"),
            JSON.stringify({ page: { size: "letter" }, margin: 72 }),
            "utf8",
        );
        await writeFile(join(dir, "vutex.toml"), '[packages]\ntemplate = "./templates/article"\n', "utf8");
        await new PackageManager(dir).install();

        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.equal(calls.display.length, 1);
        assert.deepEqual(calls.template, [
            { pageWidth: 612, pageHeight: 792, marginTop: 72, marginRight: 72, marginBottom: 72, marginLeft: 72 },
        ]);
        assert.ok(result.diagnostics.every((item) => item.code !== "VUTEX_PACKAGE_TEMPLATE_INACTIVE"));
        assert.equal(result.diagnostics.filter((item) => item.severity === "error").length, 0);
        const extra = calls.reports.at(-1)?.extra ?? "";
        assert.ok(extra.includes('"template":"templates/article"'), extra);
    });
});

test("render fails closed when a template package has no valid template.json", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const template = join(dir, "templates", "article");
        await mkdir(template, { recursive: true });
        await writeFile(join(template, "template.json"), JSON.stringify({ page: { size: "tabloid" } }), "utf8");
        await writeFile(join(dir, "vutex.toml"), '[packages]\ntemplate = "./templates/article"\n', "utf8");
        await new PackageManager(dir).install();

        const { bindings, calls } = fakeBindings();
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render(inputPath);

        assert.equal(calls.display.length, 0);
        assert.ok(result.diagnostics.some((item) => item.code === "VUTEX_PACKAGE_TEMPLATE_INVALID"));
    });
});

test("cli pdf output writes bytes beside the input and keeps them out of the json payload", async () => {
    await withTempDir(async (dir) => {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n", "utf8");
        const bytes = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37, 0x0a]);
        const result: RenderDocumentResult = {
            outputFormat: "pdf",
            pdf: bytes,
            reportJson: '{"operation":"render"}',
            diagnostics: [],
        };

        const written = await writePdfOutput(inputPath, bytes, result);
        const expectedPath = join(dir, "chapter.pdf");

        assert.equal(written.outputPath, expectedPath);
        assert.ok(isAbsolute(written.outputPath));
        assert.deepEqual(new Uint8Array(await readFile(written.outputPath)), bytes);
        assert.equal("pdf" in written.payload, false);
        assert.equal(written.payload.outputPath, expectedPath);
        assert.equal(written.payload.reportJson, '{"operation":"render"}');
        assert.equal(written.payload.outputFormat, "pdf");
    });
});
