import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { assertCatalogCoverage } from "@vmz/commander";

import { loadBindings } from "../src/cli/context.js";
import { buildVuTeXCli } from "../src/cli/cli.js";
import { emitHuman, emitJson, presentDiagnostics } from "../src/cli/diagnostics.js";
import { ExitCode } from "../src/cli/exit-codes.js";
import { runCli } from "../src/cli/main.js";
import type { VuTeXDiagnostic } from "../src/types.js";

const native = loadBindings();

type Capture = { value: number; stdout: string[]; stderr: string[] };

async function capture(run: () => Promise<number>): Promise<Capture> {
    const stdout: string[] = [];
    const stderr: string[] = [];
    const originalLog = console.log;
    const originalError = console.error;
    console.log = (...args: unknown[]) => {
        stdout.push(args.join(" "));
    };
    console.error = (...args: unknown[]) => {
        stderr.push(args.join(" "));
    };
    try {
        const value = await run();
        return { value, stdout, stderr };
    } finally {
        console.log = originalLog;
        console.error = originalError;
    }
}

async function withTempDir<T>(run: (dir: string) => Promise<T>): Promise<T> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-cli-"));
    try {
        return await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

function nativeCliTest(name: string, run: () => Promise<void>): void {
    test(name, { skip: native ? false : "native bindings unavailable" }, run);
}

test("ExitCode maps the three documented process codes", () => {
    assert.deepEqual(ExitCode, { Ok: 0, ActionFailed: 1, InvalidArgs: 2 });
});

test("emitHuman writes to console.error and emitJson pretty-prints to console.log", async () => {
    const { stdout, stderr } = await capture(async () => {
        emitHuman("plain message");
        emitJson({ a: 1 });
        return 0;
    });
    assert.deepEqual(stderr, ["plain message"]);
    assert.deepEqual(stdout, ['{\n  "a": 1\n}']);
});

test("presentDiagnostics json mode emits the diagnostics envelope on stdout", async () => {
    const diagnostics: VuTeXDiagnostic[] = [
        { code: "VUTEX_A", severity: "error", message: "first", stage: "render" },
    ];
    const { value, stdout, stderr } = await capture(async () => {
        presentDiagnostics(diagnostics, true);
        return 0;
    });
    assert.equal(value, 0);
    assert.deepEqual(stderr, []);
    assert.deepEqual(JSON.parse(stdout.join("\n")), { diagnostics });
});

test("presentDiagnostics human mode formats the stage prefix and em dash", async () => {
    const diagnostics: VuTeXDiagnostic[] = [
        { code: "VUTEX_A", severity: "error", message: "first", stage: "render" },
        { code: "VUTEX_B", severity: "warning", message: "second" },
    ];
    const { stdout, stderr } = await capture(async () => {
        presentDiagnostics(diagnostics, false);
        return 0;
    });
    assert.deepEqual(stdout, []);
    assert.deepEqual(stderr, [
        "[render] error: VUTEX_A — first",
        "warning: VUTEX_B — second",
    ]);
});

test("the en-US cli catalog covers every registered help id", async () => {
    const catalog = JSON.parse(
        await readFile(new URL("../locales/en-US/cli.json", import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.doesNotThrow(() => assertCatalogCoverage(buildVuTeXCli(), catalog));
});

test("runCli offline matrix keeps every path out of the bindings", async () => {
    const empty = await capture(() => runCli([]));
    assert.equal(empty.value, ExitCode.Ok);
    assert.match(empty.stdout.join("\n"), /Usage: vutex <command> \[options\]/);

    const bogus = await capture(() => runCli(["bogus-cmd"]));
    assert.equal(bogus.value, ExitCode.ActionFailed);
    assert.ok(bogus.stderr.some((line) => line.includes("unknown command `bogus-cmd`")));

    const renderNoInput = await capture(() => runCli(["render"]));
    assert.equal(renderNoInput.value, ExitCode.InvalidArgs);
    assert.deepEqual(renderNoInput.stdout, []);
    assert.deepEqual(renderNoInput.stderr, []);

    const packageGroup = await capture(() => runCli(["package"]));
    assert.equal(packageGroup.value, ExitCode.Ok);
    assert.match(packageGroup.stdout.join("\n"), /list/);

    const packageBogus = await capture(() => runCli(["package", "bogus"]));
    assert.equal(packageBogus.value, ExitCode.ActionFailed);
    assert.ok(packageBogus.stderr.some((line) => line.includes("unknown command `package bogus`")));

    const unknownOption = await capture(() => runCli(["render", "--nope"]));
    assert.equal(unknownOption.value, ExitCode.ActionFailed);
    assert.ok(unknownOption.stderr.some((line) => line.includes("unknown option `--nope`")));

    const renderHelp = await capture(() => runCli(["render", "--help"]));
    assert.equal(renderHelp.value, ExitCode.Ok);
    assert.match(renderHelp.stdout.join("\n"), /vutex render/);
});

nativeCliTest("cli render --json emits the machine payload", async () => {
    await withTempDir(async (dir) => {
        const cwd = process.cwd();
        process.chdir(dir);
        try {
            await writeFile(join(dir, "doc.md"), "# Title\n\nInline $x^2$ math.\n", "utf8");
            const { value, stdout } = await capture(() => runCli(["render", "doc.md", "--json"]));
            assert.equal(value, ExitCode.Ok);
            const payload = JSON.parse(stdout.join("\n")) as {
                exitCode: number;
                outputFormat: string;
                svg: string;
            };
            assert.equal(payload.exitCode, 0);
            assert.equal(payload.outputFormat, "svg");
            assert.match(payload.svg, /^<svg /);
        } finally {
            process.chdir(cwd);
        }
    });
});

nativeCliTest("cli render prints the svg body in human mode", async () => {
    await withTempDir(async (dir) => {
        const cwd = process.cwd();
        process.chdir(dir);
        try {
            await writeFile(join(dir, "doc.md"), "# Title\n", "utf8");
            const { value, stdout } = await capture(() => runCli(["render", "doc.md"]));
            assert.equal(value, ExitCode.Ok);
            assert.match(stdout[0] ?? "", /^<svg /);
        } finally {
            process.chdir(cwd);
        }
    });
});

nativeCliTest("cli package doctor fails on an empty project", async () => {
    await withTempDir(async (dir) => {
        const cwd = process.cwd();
        process.chdir(dir);
        try {
            const { value, stderr } = await capture(() => runCli(["package", "doctor"]));
            assert.equal(value, ExitCode.ActionFailed);
            assert.ok(stderr.some((line) => line.includes("VuTeX resource packages have issues")));
        } finally {
            process.chdir(cwd);
        }
    });
});

nativeCliTest("cli inspect --json reports the markdown document", async () => {
    await withTempDir(async (dir) => {
        const cwd = process.cwd();
        process.chdir(dir);
        try {
            await writeFile(join(dir, "doc.md"), "# Title\n", "utf8");
            const { value, stdout } = await capture(() => runCli(["inspect", "doc.md", "--json"]));
            assert.equal(value, ExitCode.Ok);
            const payload = JSON.parse(stdout.join("\n")) as {
                inputFormat: string;
                diagnostics: unknown[];
            };
            assert.equal(payload.inputFormat, "markdown");
            assert.deepEqual(payload.diagnostics, []);
        } finally {
            process.chdir(cwd);
        }
    });
});

nativeCliTest("cli inspect fails with the input-not-found diagnostic", async () => {
    await withTempDir(async (dir) => {
        const cwd = process.cwd();
        process.chdir(dir);
        try {
            const { value, stderr } = await capture(() => runCli(["inspect", "nope.md"]));
            assert.equal(value, ExitCode.ActionFailed);
            assert.ok(
                stderr.some((line) =>
                    line.includes("[inspect] error: VUTEX_INPUT_NOT_FOUND — input path does not exist:"),
                ),
                `stderr was: ${JSON.stringify(stderr)}`,
            );
        } finally {
            process.chdir(cwd);
        }
    });
});
