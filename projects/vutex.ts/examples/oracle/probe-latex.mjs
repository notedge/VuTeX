#!/usr/bin/env node
/**
 * Probe for a TeX engine and record whether the LaTeX geometry oracle can run.
 *
 * LaTeX is the only candidate that can report the box metrics VuTeX needs
 * (`width`, `height`, `depth`) from an independent typesetter. Nothing else in
 * this repository reports them, so when TeX is absent the geometry oracle is
 * genuinely missing evidence — it is recorded as `blocked`, never as "passing".
 *
 * The fixture this script writes (`tests/fixtures/oracle/latex-status.json`) is
 * a status document: it carries the probe output verbatim, the reason the
 * oracle cannot run, and the procedure that a working harness must implement.
 * Every measurement field of every record stays `null` while the status is
 * `blocked`, so a Rust test can assert that no geometry was invented.
 *
 * Usage, from the repository root or this directory:
 *
 *   node projects/vutex.ts/examples/oracle/probe-latex.mjs
 */

import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");

const CORPUS_PATH = resolve(repoRoot, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const OUT_PATH = resolve(repoRoot, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/latex-status.json");
const COMMAND = "node projects/vutex.ts/examples/oracle/probe-latex.mjs";

/** Binaries the geometry oracle depends on, in the order they are probed. */
const PROBES = [
  {
    name: "lualatex",
    argv: ["--version"],
    note: "required: LuaLaTeX sets the corpus and prints the box metrics the oracle needs",
  },
  {
    name: "kpsewhich",
    argv: ["--version"],
    note: "required: locates `amsmath.sty` so its version can be recorded",
  },
  {
    name: "tlmgr",
    argv: ["--version"],
    note: "informative: identifies the TeX Live installation the engine comes from",
  },
  {
    name: "pdflatex",
    argv: ["--version"],
    note: "informative: a classic pdfTeX install would prove a TeX tree exists even without Lua",
  },
];

/** First non-empty line of an engine's `--version` output. */
function firstLine(text) {
  if (!text) {
    return null;
  }
  const line = text.split(/\r?\n/).find((candidate) => candidate.trim() !== "");
  return line === undefined ? null : line.trim();
}

/** Run one probe without letting a missing binary become a fatal error. */
function probe({ name, argv, note }) {
  const result = spawnSync(name, argv, { encoding: "utf8", timeout: 60_000, windowsHide: true });
  const missing = result.error !== undefined && result.error !== null;
  return {
    name,
    argv,
    found: !missing,
    exit_code: missing ? null : result.status,
    version_line: missing ? null : firstLine(result.stdout) ?? firstLine(result.stderr),
    error: missing ? String(result.error.code ?? result.error.message) : null,
    note,
  };
}

/** Locate `amsmath.sty` and record its declared version, when a TeX tree exists. */
function probeAmsmath(kpsewhichFound) {
  if (!kpsewhichFound) {
    return {
      name: "amsmath",
      found: false,
      path: null,
      version: null,
      reason: "`kpsewhich` is not on PATH, so `amsmath.sty` cannot be located and its version cannot be read",
    };
  }

  const located = spawnSync("kpsewhich", ["amsmath.sty"], { encoding: "utf8", timeout: 60_000, windowsHide: true });
  const path = located.stdout ? located.stdout.trim() : "";
  if (located.status !== 0 || path === "") {
    return {
      name: "amsmath",
      found: false,
      path: null,
      version: null,
      reason: "`kpsewhich amsmath.sty` returned no path, so `amsmath` is missing from the TeX tree",
    };
  }

  let declared = null;
  try {
    const style = readFileSync(path, "utf8");
    const match = style.match(/\\ProvidesPackage\{amsmath\}\[([^\]]*)\]/);
    declared = match === null ? null : match[1].trim();
  } catch (error) {
    return {
      name: "amsmath",
      found: false,
      path,
      version: null,
      reason: `found \`${path}\` but could not read it: ${String(error && error.message ? error.message : error)}`,
    };
  }

  return { name: "amsmath", found: true, path, version: declared, reason: null };
}

/**
 * Why the geometry oracle cannot produce measurements right now.
 *
 * The message is derived from the probe results so it reports what was
 * actually observed instead of a hardcoded claim.
 */
function blockedReason(probes) {
  const lualatex = probes.find((candidate) => candidate.name === "lualatex");
  if (lualatex.found) {
    return [
      "`lualatex` is on PATH, but `projects/vutex.ts/examples/oracle` does not implement the box-metrics harness",
      "that parses `\\showbox` output, so no measurement can be emitted yet",
    ].join(" ");
  }
  const missing = probes.filter((candidate) => !candidate.found).map((candidate) => candidate.name);
  return [
    `no TeX engine is available: ${missing.join(", ")} are all missing from PATH`,
    `(\`lualatex\` exited with \`${lualatex.error}\`)`,
    "so the corpus cannot be laid out by LaTeX and no box metric exists",
  ].join(" ");
}

/** The harness a future contributor must implement before this oracle can run. */
const PROCEDURE = [
  "Install a TeX distribution that ships LuaLaTeX and `amsmath`, then re-run `node projects/vutex.ts/examples/oracle/probe-latex.mjs`: `lualatex --version` must print a `This is LuaHBTeX, Version ...` line and `kpsewhich amsmath.sty` must return a path whose `\\ProvidesPackage{amsmath}[...]` line gives the amsmath version.",
  "For every corpus formula, set it in a box in both styles: `\\setbox0=\\hbox{$<tex>$}` for inline and `\\setbox0=\\hbox{$\\displaystyle <tex>$}` for display.",
  "Read `width`, `height` and `depth` from that box: `\\showbox0` writes a `. width X pt, height Y pt, depth Z pt` line into the run log, and `\\pdfsavepos` with `\\pdflastxpos`/`\\pdflastypos` gives the baseline origin of the same box when an absolute position is needed.",
  "Convert TeX points to em with the font size actually used: `em = size_pt` (10pt for `\\normalsize`), so `value_em = value_pt / size_pt`. Record `font` with the family name, the `sha256` of the `.otf` file TeX loaded and its `units_per_em`.",
  "Fields the TeX box model cannot report stay `null`: `ink_bounds` (a box dimension includes side bearings and italic correction but no glyph ink), `script_positions` (only derivable by measuring the script boxes separately), and `delimiter_size` for delimiters TeX assembles internally unless the harness prints that box too.",
  "Emit one record per formula and style into `tests/fixtures/oracle/latex-status.json` with `source.status` `available`, the probed engine version and the amsmath version, and leave every field the harness did not measure as `null`.",
];

/** Record with every field the absent engine cannot measure left at `null`. */
function record(formula, displayMode, reason) {
  return {
    schema_version: "v0",
    corpus_id: formula.id,
    tex: formula.tex,
    display_mode: displayMode,
    source: {
      engine: "latex",
      version: null,
      command: COMMAND,
      status: "blocked",
      status_reason: reason,
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
    mathml: null,
  };
}

function main() {
  const corpus = JSON.parse(readFileSync(CORPUS_PATH, "utf8"));
  const probes = PROBES.map(probe);
  const amsmath = probeAmsmath(probes.find((candidate) => candidate.name === "kpsewhich").found);
  const lualatex = probes.find((candidate) => candidate.name === "lualatex");
  const runnable = lualatex.found && lualatex.exit_code === 0;
  const reason = blockedReason(probes);

  const records = [];
  for (const formula of corpus.formulas) {
    for (const displayMode of [false, true]) {
      records.push(record(formula, displayMode, reason));
    }
  }

  const document = {
    schema_version: "v0",
    source: {
      engine: "latex",
      version: runnable ? lualatex.version_line : null,
      command: COMMAND,
      status: "blocked",
      status_reason: reason,
    },
    corpus_path: "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
    procedure: PROCEDURE,
    probes,
    amsmath,
    records,
  };

  mkdirSync(dirname(OUT_PATH), { recursive: true });
  writeFileSync(OUT_PATH, `${JSON.stringify(document, null, 2)}\n`, "utf8");

  console.log(`wrote ${OUT_PATH}`);
  console.log(`latex oracle status: ${document.source.status}`);
  console.log(`reason: ${reason}`);
  for (const candidate of probes) {
    console.log(`  ${candidate.name}: ${candidate.found ? `found (${candidate.version_line})` : `missing (${candidate.error})`}`);
  }
}

main();
