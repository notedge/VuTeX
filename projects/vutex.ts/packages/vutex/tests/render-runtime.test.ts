import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import test from "node:test";

import { writePdfOutput } from "../src/cli/commands/render.js";
import { createVuTeXRuntime } from "../src/runtime.js";
import type { RenderDocumentResult, VuTeXBindings } from "../src/types.js";

type RenderCall = {
    inputPath: string;
    payload: Record<string, unknown>;
};

type RenderResponse = {
    exitCode: number;
    outputFormat: string;
    svg?: string;
    html?: string;
    pdf?: Uint8Array;
    reportJson: string;
};

function fakeBindings(respond: (call: RenderCall) => RenderResponse): {
    bindings: VuTeXBindings;
    calls: RenderCall[];
} {
    const calls: RenderCall[] = [];
    const bindings: VuTeXBindings = {
        vutexVersion: () => "0.0.0-test",
        measureFormula: () => "{}",
        renderFormulaSvg: () => "",
        renderFormulaMathml: () => "",
        renderFormula: () => "{}",
        inspectDocument: () => ({ inputFormat: "tex", path: "", reportJson: "{}" }),
        renderDocument: (inputPath, optionsJson) => {
            const payload = JSON.parse(optionsJson ?? "{}") as Record<string, unknown>;
            const call = { inputPath, payload };
            calls.push(call);
            return respond(call);
        },
    };
    return { bindings, calls };
}

test("render defaults output to svg and passes svg through", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const { bindings, calls } = fakeBindings(() => ({
            exitCode: 0,
            outputFormat: "svg",
            svg: "<svg/>",
            reportJson: "{}",
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex");

        assert.equal(calls.length, 1);
        assert.equal(calls[0].inputPath, "main.tex");
        assert.equal(calls[0].payload.output, "svg");
        assert.equal(result.svg, "<svg/>");
        assert.equal(result.html, undefined);
        assert.equal(result.exitCode, 0);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("render forwards html output format and result", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const { bindings, calls } = fakeBindings(() => ({
            exitCode: 0,
            outputFormat: "html",
            html: "<html/>",
            reportJson: "{}",
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex", { output: "html" });

        assert.equal(calls[0].payload.output, "html");
        assert.equal(result.html, "<html/>");
        assert.equal(result.svg, undefined);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("render forwards snapshot output format and result", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const { bindings, calls } = fakeBindings(() => ({
            exitCode: 0,
            outputFormat: "snapshot",
            html: "<html data-snapshot=\"true\"/>",
            reportJson: "{}",
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex", { output: "snapshot" });

        assert.equal(calls[0].payload.output, "snapshot");
        assert.equal(result.outputFormat, "snapshot");
        assert.equal(result.html, "<html data-snapshot=\"true\"/>");
        assert.equal(result.svg, undefined);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("render forwards pdf output format and passes bytes through", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const pdf = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37]);
        const { bindings, calls } = fakeBindings(() => ({
            exitCode: 0,
            outputFormat: "pdf",
            pdf,
            reportJson: "{}",
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex", { output: "pdf" });

        assert.equal(calls[0].payload.output, "pdf");
        assert.equal(result.outputFormat, "pdf");
        assert.equal(result.pdf, pdf);
        assert.equal(result.svg, undefined);
        assert.equal(result.html, undefined);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("cli pdf output writes bytes beside the input and keeps them out of the json payload", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const inputPath = join(dir, "chapter.md");
        await writeFile(inputPath, "# Title\n");
        const bytes = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37, 0x0a]);
        const result: RenderDocumentResult = {
            exitCode: 0,
            outputFormat: "pdf",
            pdf: bytes,
            reportJson: '{"status":"success"}',
            diagnostics: [],
        };

        const written = await writePdfOutput(inputPath, bytes, result);
        const expectedPath = join(dir, "chapter.pdf");

        assert.equal(written.outputPath, expectedPath);
        assert.ok(isAbsolute(written.outputPath));
        assert.deepEqual(new Uint8Array(await readFile(written.outputPath)), bytes);
        assert.equal("pdf" in written.payload, false);
        assert.equal(written.payload.outputPath, expectedPath);
        assert.equal(written.payload.reportJson, '{"status":"success"}');
        assert.equal(written.payload.outputFormat, "pdf");
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("render keeps unknown output formats intact for the native side", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const { bindings, calls } = fakeBindings(() => ({
            exitCode: 1,
            outputFormat: "bogus",
            reportJson: "{}",
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        await runtime.render("main.tex", { output: "bogus" });

        assert.equal(calls[0].payload.output, "bogus");
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("render parses diagnostics from the report", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const reportJson = JSON.stringify({
            diagnostics: [
                {
                    code: "unsupported-output",
                    severity: "error",
                    message: "output format is not supported",
                },
            ],
        });
        const { bindings } = fakeBindings(() => ({
            exitCode: 1,
            outputFormat: "bogus",
            reportJson,
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex", { output: "bogus" });

        assert.equal(result.diagnostics.length, 1);
        assert.equal(result.diagnostics[0].code, "unsupported-output");
        assert.equal(result.diagnostics[0].severity, "error");
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("render returns a non-zero exit code unchanged", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-render-"));
    try {
        const { bindings } = fakeBindings(() => ({
            exitCode: 2,
            outputFormat: "svg",
            reportJson: "{}",
        }));
        const runtime = createVuTeXRuntime(bindings, { projectRoot: dir });
        const result = await runtime.render("main.tex");

        assert.equal(result.exitCode, 2);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
