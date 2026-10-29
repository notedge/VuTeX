#!/usr/bin/env node
/**
 * Emit MathJax v3's structured Presentation MathML for the shared oracle corpus.
 *
 * MathJax is a second independent TeX → MathML implementation, so its output
 * doubles the structure oracle: every corpus formula now has two MathML
 * reference trees (KaTeX and MathJax) that VuTeX's semantic lowering must agree
 * with at the element-nesting level.
 *
 * MathJax, like KaTeX, does not report metrics, font or size, so those record
 * fields stay `null` exactly as `fixture-schema.json` requires. A corpus entry
 * MathJax rejects is recorded as `unsupported` with its error message instead
 * of being dropped, so the Rust test can tell "no evidence" apart from
 * "agreement".
 *
 * Usage, from this directory:
 *
 *   npm install
 *   node emit-mathjax-mathml.mjs
 *
 * The mathjax version is pinned in `scripts/oracle/package.json` and the fixture
 * records the version that actually produced it.
 */

import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");

const CORPUS_PATH = resolve(repoRoot, "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const OUT_PATH = resolve(repoRoot, "projects/crates/vutex-layout/tests/fixtures/oracle/mathjax-mathml.json");

// MathJax v3 exposes a CommonJS entry (`es5/node-main.js`); we load it via
// createRequire because this project uses `"type": "module"` and mathjax
// ships no native ESM export.
const require = createRequire(import.meta.url);
const MathJax = require("mathjax");
const mathjaxVersion = require("mathjax/package.json").version;

const COMMAND = "node scripts/oracle/emit-mathjax-mathml.mjs";

// ---------------------------------------------------------------------------
// Initialize MathJax with TeX input → MathML output, using the minimal loader
// config that MathJax v3.2.2 exposes through its CommonJS node-main entry.
// ---------------------------------------------------------------------------

// Packages needed for the corpus: base covers frac/sqrt/sum/int, ams provides
// matrix/cases/aligned/alignedat/split, cases provides the cases environment.
const MJ = await MathJax.init({
  loader: { load: ["input/tex", "adaptors/liteDOM", "startup"] },
  startup: { typeset: false },
  tex: {
    packages: ["base", "ams", "cases"],
  },
});

/** Render one formula to MathML, or capture why MathJax refused it. */
async function renderMathML(texSource, displayMode) {
  try {
    // MathJax's tex2mml returns a clean `<math xmlns="…">…</math>` string
    // directly — no wrapper span like KaTeX emits.
    const mathml = MJ.tex2mml(texSource, { display: displayMode });
    return { status: "available", reason: null, mathml };
  } catch (error) {
    const message = error && (error.message || String(error)) || String(error);
    return { status: "unsupported", reason: message, mathml: null };
  }
}

/** A record with every field this engine cannot measure left at `null`. */
function record(formula, displayMode, rendered) {
  return {
    schema_version: "v0",
    corpus_id: formula.id,
    tex: formula.tex,
    display_mode: displayMode,
    source: {
      engine: "mathjax",
      version: mathjaxVersion,
      command: COMMAND,
      status: rendered.status,
      status_reason: rendered.reason,
    },
    font: null,
    size: null,
    normalized_atoms: null,
    style_cramped: null,
    width: null,
    height: null,
    depth: null,
    axis: null,
    ink_bounds: null,
    rule_thickness: null,
    delimiter_size: null,
    script_positions: null,
    mathml: rendered.mathml,
  };
}

async function main() {
  const corpus = JSON.parse(readFileSync(CORPUS_PATH, "utf8"));
  const records = [];
  for (const formula of corpus.formulas) {
    for (const displayMode of [false, true]) {
      records.push(record(formula, displayMode, await renderMathML(formula.tex, displayMode)));
    }
  }

  const document = {
    schema_version: "v0",
    source: {
      engine: "mathjax",
      version: mathjaxVersion,
      command: COMMAND,
      status: records.some((r) => r.source.status === "available") ? "available" : "blocked",
      status_reason: null,
    },
    corpus_path: "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
    records,
  };

  mkdirSync(dirname(OUT_PATH), { recursive: true });
  writeFileSync(OUT_PATH, `${JSON.stringify(document, null, 2)}\n`, "utf8");

  const unsupported = records.filter((r) => r.source.status !== "available");
  console.log(`wrote ${OUT_PATH}`);
  console.log(`${records.length} records from mathjax ${mathjaxVersion}, ${unsupported.length} unsupported`);
  for (const failed of unsupported) {
    console.log(`  unsupported: ${failed.corpus_id} (display=${failed.display_mode}): ${failed.source.status_reason}`);
  }
}

main().catch((error) => {
  console.error("mathjax emit failed:", error);
  process.exit(1);
});
