/**
 * TeX cross-check: compile the same two display formulas with a real TeX
 * engine (Tectonic) and compare scale-normalised layout facts against the
 * VuTeX PDF. Only typography facts are compared, never bytes.
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadNative, resultsRoot } from "../lib/ours.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const corpus = resolve(here, "corpus");
const build = resolve(resultsRoot, ".build", "tex");
mkdirSync(build, { recursive: true });

const oursPdf = resolve(build, "ours.pdf");
const referenceDir = resolve(build, "tex");
mkdirSync(referenceDir, { recursive: true });

const native = loadNative();
const rendered = native.renderDocument(resolve(corpus, "display.md"), JSON.stringify({ output: "pdf" }));
if (rendered.exitCode !== 0) {
    throw new Error(`VuTeX failed to render display.md: ${rendered.reportJson}`);
}
writeFileSync(oursPdf, rendered.pdf);

const engine = process.env.TECTONIC ?? "tectonic";
execFileSync(engine, ["--outdir", referenceDir, resolve(corpus, "display.tex")], { stdio: "inherit" });
const referencePdf = resolve(referenceDir, "display.pdf");
if (!existsSync(referencePdf)) {
    throw new Error(`tectonic did not produce ${referencePdf}`);
}

const pythonDir = resolve(here, "..", "pdf", "python");
const venvPython = process.platform === "win32"
    ? resolve(pythonDir, ".venv", "Scripts", "python.exe")
    : resolve(pythonDir, ".venv", "bin", "python");
const python = existsSync(venvPython) ? venvPython : process.env.PYTHON ?? "python";
const out = resolve(resultsRoot, "crosscheck", "tex", `${new Date().toISOString().replace(/[:.]/g, "-")}.json`);
mkdirSync(dirname(out), { recursive: true });

const run = execFileSync(python, [
    resolve(here, "compare_geometry.py"),
    "--ours", oursPdf,
    "--reference", referencePdf,
    "--out", out,
], { encoding: "utf8" });
console.log(run);
const payload = JSON.parse(run.slice(run.indexOf("{")));
for (const record of payload.records) {
    if (record.error) {
        console.log(`${record.case}: ERROR ${record.error}`);
        continue;
    }
    const worst = Object.entries(record.deltas).sort((a, b) => Math.abs(b[1]) - Math.abs(a[1]))[0];
    const magnitude = Math.abs(worst[1]);
    const judgment = magnitude <= 0.25 ? "equivalent" : magnitude <= 0.5 ? "minor-diff" : "significant-diff";
    console.log(`${record.case}: ${judgment} (worst ${worst[0]} delta ${worst[1]})`);
}
console.log(`tectonic reference: ${referencePdf}`);
console.log(`results: ${out}`);
