#!/usr/bin/env node
/**
 * Emit KaTeX's structured Presentation MathML for the shared oracle corpus.
 *
 * KaTeX is an independent TeX implementation, so its MathML gives VuTeX a
 * structure oracle: a wrong `mfrac` / `msup` / `mtable` nesting in VuTeX's
 * semantic lowering shows up as a structural difference even when both engines
 * agree on every painted glyph.
 *
 * KaTeX reports no metrics, no font and no size, so those record fields stay
 * `null` exactly as `fixture-schema.json` requires. A corpus entry KaTeX
 * rejects is recorded as `unsupported` with its error message instead of being
 * dropped, so the Rust test can tell "no evidence" apart from "agreement".
 *
 * Usage, from the repository root or this directory:
 *
 *   node scripts/oracle/emit-katex-mathml.mjs
 *
 * The katex version is pinned in `scripts/oracle/package.json` and require()d
 * from the installed package, so the fixture records the version that actually
 * produced it.
 */

import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");

const CORPUS_PATH = resolve(repoRoot, "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const OUT_PATH = resolve(repoRoot, "projects/crates/vutex-layout/tests/fixtures/oracle/katex-mathml.json");

const require = createRequire(import.meta.url);
const katex = require("katex");
const katexVersion = require("katex/package.json").version;

const COMMAND = "node scripts/oracle/emit-katex-mathml.mjs";

/** Render one formula to MathML, or capture why KaTeX refused it. */
function renderMathML(tex, displayMode) {
  try {
    return { status: "available", reason: null, mathml: katex.renderToString(tex, {
      output: "mathml",
      displayMode,
      throwOnError: true,
      strict: false,
    }) };
  } catch (error) {
    return { status: "unsupported", reason: String(error && error.message ? error.message : error), mathml: null };
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
      engine: "katex",
      version: katexVersion,
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

function main() {
  const corpus = JSON.parse(readFileSync(CORPUS_PATH, "utf8"));
  const records = [];
  for (const formula of corpus.formulas) {
    for (const displayMode of [false, true]) {
      records.push(record(formula, displayMode, renderMathML(formula.tex, displayMode)));
    }
  }

  const document = {
    schema_version: "v0",
    source: {
      engine: "katex",
      version: katexVersion,
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
  console.log(`${records.length} records from katex ${katexVersion}, ${unsupported.length} unsupported`);
  for (const failed of unsupported) {
    console.log(`  unsupported: ${failed.corpus_id} (display=${failed.display_mode}): ${failed.source.status_reason}`);
  }
}

main();
