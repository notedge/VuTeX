/**
 * TeX cross-check: compile the same display formulas with a real TeX
 * engine (Tectonic) and compare scale-normalised layout facts against the
 * VuTeX PDF. Only typography facts are compared, never bytes.
 *
 * Two corpora run here. `display` carries the construct cases and `atoms`
 * carries single-symbol formulas, whose facts isolate the font difference
 * from any assembly decision.
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadNative, resultsRoot } from "../lib/ours.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const corpus = resolve(here, "corpus");
const sets = ["display", "display-extra-a", "display-extra-b", "atoms-a", "atoms-b"];

const engine = process.env.TECTONIC ?? "tectonic";
const pythonDir = resolve(here, "..", "pdf", "python");
const venvPython = process.platform === "win32"
    ? resolve(pythonDir, ".venv", "Scripts", "python.exe")
    : resolve(pythonDir, ".venv", "bin", "python");
const python = existsSync(venvPython) ? venvPython : process.env.PYTHON ?? "python";
const native = loadNative();

function worstDelta(deltas) {
    const worst = Object.entries(deltas).sort((a, b) => Math.abs(b[1]) - Math.abs(a[1]))[0];
    const magnitude = Math.abs(worst[1]);
    const judgment = magnitude <= 0.25 ? "equivalent" : magnitude <= 0.5 ? "minor-diff" : "significant-diff";
    return `${judgment} (worst ${worst[0]} delta ${worst[1]})`;
}

function runSet(set) {
    const build = resolve(resultsRoot, ".build", "tex", set);
    mkdirSync(build, { recursive: true });
    const oursPdf = resolve(build, "ours.pdf");
    const referenceDir = resolve(build, "tex");
    mkdirSync(referenceDir, { recursive: true });

    const rendered = native.renderDocument(resolve(corpus, `${set}.md`), JSON.stringify({ output: "pdf" }));
    if (rendered.exitCode !== 0) {
        throw new Error(`VuTeX failed to render ${set}.md: ${rendered.reportJson}`);
    }
    writeFileSync(oursPdf, rendered.pdf);

    execFileSync(engine, ["--outdir", referenceDir, resolve(corpus, `${set}.tex`)], { stdio: "inherit" });
    const referencePdf = resolve(referenceDir, `${set}.pdf`);
    if (!existsSync(referencePdf)) {
        throw new Error(`tectonic did not produce ${referencePdf}`);
    }

    const out = resolve(resultsRoot, "crosscheck", "tex", set, `${new Date().toISOString().replace(/[:.]/g, "-")}.json`);
    mkdirSync(dirname(out), { recursive: true });
    const run = execFileSync(python, [
        resolve(here, "compare_geometry.py"),
        "--ours", oursPdf,
        "--reference", referencePdf,
        "--out", out,
        "--set", set,
    ], { encoding: "utf8" });
    const payload = JSON.parse(run.slice(run.indexOf("{")));

    console.log(`\n[${set}] tectonic reference: ${referencePdf}`);
    for (const record of payload.records) {
        if (record.error) {
            console.log(`${record.case}: ERROR ${record.error}`);
            continue;
        }
        console.log(`${record.case}: ${worstDelta(record.deltas)}`);
        console.log(`  legacy: ${worstDelta(record.legacy.deltas)}`);
    }
    console.log(`[${set}] results: ${out}`);
    return payload;
}

const payloads = sets.map(runSet);
const summary = payloads.map((payload) => ({
    corpus: payload.corpus,
    records: payload.records,
}));
writeFileSync(
    resolve(resultsRoot, "crosscheck", "tex", "summary.json"),
    JSON.stringify({ criterion: payloads[0].criterion, sets: summary }, null, 2),
);
const total = summary.reduce((sum, entry) => sum + entry.records.length, 0);
console.log(`\ntex facts: ${total} cases across ${sets.length} corpora`);
