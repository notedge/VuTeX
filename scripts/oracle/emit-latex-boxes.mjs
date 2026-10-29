#!/usr/bin/env node
/**
 * Emit LaTeX box-metrics oracle fixture.
 *
 * This harness drives an external TeX engine through every formula in the
 * oracle corpus, captures the resulting box dimensions (`width`, `height`,
 * `depth`), and writes them into `latex-status.json` following the shared
 * fixture-schema.  It is the canonical generator for the VuTeX LaTeX
 * geometry oracle — the only independent typesetter that can report box
 * metrics we need.
 *
 * Engines are probed in preference order:
 *   1. tectonic  — self-contained, no TeX tree install
 *   2. lualatex  — requires a TeX Live / MacTeX distribution
 *
 * If no engine is available the harness emits a `blocked` fixture with the
 * probe evidence — the exact same shape the old `probe-latex.mjs` produced,
 * but generated here so the harness owns both paths.
 *
 * Usage:
 *   node scripts/oracle/emit-latex-boxes.mjs
 *   node scripts/oracle/emit-latex-boxes.mjs \
 *     --tex path/to/corpus.json --out path/to/latex-status.json
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { platform } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
let corpusArg, outArg;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--tex") corpusArg = args[++i];
  else if (a === "--out") outArg = args[++i];
}

const CORPUS_PATH = corpusArg
  ? resolve(corpusArg)
  : resolve(repoRoot, "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const OUT_PATH = outArg
  ? resolve(outArg)
  : resolve(repoRoot, "projects/crates/vutex-layout/tests/fixtures/oracle/latex-status.json");
const COMMAND = "node scripts/oracle/emit-latex-boxes.mjs";

// TeX normal size is 10pt; 1em = 10pt in the reference measurements.
const BASE_SIZE_PT = 10;

// ---------------------------------------------------------------------------
// Engine probe
// ---------------------------------------------------------------------------

/**
 * TeX engine candidates, in priority order.
 *
 * Tectonic comes first because it ships its own TeX tree and needs zero
 * installation.  LuaLaTeX needs TeX Live or MacTeX but is the traditional
 * engine this oracle was designed for.
 */
const ENGINE_CANDIDATES = [
  { name: "tectonic", argv: ["--version"], family: "tectonic" },
  { name: "lualatex", argv: ["--version"], family: "luatex" },
];

/** Extra probes that only matter when a TeX tree exists. */
const TREE_PROBES = [
  { name: "kpsewhich", argv: ["--version"], note: "locates amsmath.sty for version recording" },
  { name: "pdflatex", argv: ["--version"], note: "classic pdfTeX signals a full TeX tree" },
  { name: "tlmgr", argv: ["--version"], note: "identifies TeX Live distribution" },
];

/** First non-empty line of an engine's output. */
function firstLine(text) {
  if (!text) return null;
  const line = text.split(/\r?\n/).find((c) => c.trim() !== "");
  return line === undefined ? null : line.trim();
}

/** Run one probe, never throw on missing binary. */
function probe({ name, argv, note }) {
  const result = spawnSync(name, argv, {
    encoding: "utf8",
    timeout: 60_000,
    windowsHide: true,
  });
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

/** Probe amsmath.sty via kpsewhen it exists. */
function probeAmsmath(kpseProbe) {
  if (!kpseProbe.found) {
    return {
      name: "amsmath",
      found: false,
      path: null,
      version: null,
      reason: "`kpsewhich` not on PATH — cannot locate amsmath.sty",
    };
  }
  const located = spawnSync("kpsewhich", ["amsmath.sty"], {
    encoding: "utf8",
    timeout: 60_000,
    windowsHide: true,
  });
  const path = (located.stdout || "").trim();
  if (located.status !== 0 || !path) {
    return {
      name: "amsmath",
      found: false,
      path: null,
      version: null,
      reason: "`kpsewhich amsmath.sty` returned no path",
    };
  }
  let version = null;
  try {
    const style = readFileSync(path, "utf8");
    const m = style.match(/\\ProvidesPackage\{amsmath\}\[([^\]]*)\]/);
    version = m ? m[1].trim() : null;
  } catch (err) {
    return {
      name: "amsmath",
      found: false,
      path,
      version: null,
      reason: `found ${path} but could not read it: ${err.message || err}`,
    };
  }
  return { name: "amsmath", found: true, path, version, reason: null };
}

// ---------------------------------------------------------------------------
// Blocked document (engine absent)
// ---------------------------------------------------------------------------

const PROCEDURE = [
  "Install a TeX distribution (TeX Live / MacTeX) that ships LuaLaTeX and amsmath, or fetch `tectonic` which is self-contained. Re-run `node scripts/oracle/emit-latex-boxes.mjs`.",
  "For every corpus formula, set it in a box in both styles: inline via `\\setbox0=\\hbox{$<tex>$}`, display via `\\setbox0=\\hbox{\\displaystyle $<tex>$}`.",
  "Emit box dimensions to the log with explicit markers: `\\typeout{BOXMETRIC: <inline|display> <corpus_id> \\the\\wd0 \\the\\ht0 \\the\\dp0}`.",
  "Parse the log, convert TeX points to em: `em = pt / 10.0` (normalsize = 10pt, so 1em == 10pt).",
  "Only `width`, `height`, `depth` are reliable: `ink_bounds`, `script_positions`, `delimiter_size` stay null.",
];

function blockedReason(probes, primary) {
  if (primary && primary.found && primary.exit_code === 0) {
    return [
      `\`${primary.name}\` is on PATH (${primary.version_line}), but no box metrics were emitted —`,
      "the harness may have crashed before producing measurable output.",
    ].join(" ");
  }
  const names = probes.filter((p) => !p.found).map((p) => p.name);
  const errs = probes
    .filter((p) => !p.found)
    .map((p) => `${p.name} exited with \`${p.error}\``)
    .join("; ");
  return [
    `no TeX engine is available: ${names.join(", ")} are all missing from PATH`,
    `(${errs || "no probe ran"}), so the corpus cannot be laid out by LaTeX and no box metric exists`,
  ].join(" ");
}

/** Build the shape of one record — null measurement fields. */
function recordBlocked(formula, displayMode, reason, command) {
  return {
    schema_version: "v0",
    corpus_id: formula.id,
    tex: formula.tex,
    display_mode: displayMode,
    source: {
      engine: "latex",
      version: null,
      command,
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

// ---------------------------------------------------------------------------
// TeX file generation + compile
// ---------------------------------------------------------------------------

/** Build a .tex document that typeouts box metrics for every corpus entry. */
function buildTexDocument(corpus) {
  const lines = [
    "\\documentclass[10pt]{article}",
    "\\usepackage{amsmath}",
    "\\usepackage[margin=1in]{geometry}",
    "\\pagestyle{empty}",
    "\\begin{document}",
    "",
  ];

  for (const formula of corpus.formulas) {
    const texEscaped = escapeTexForHbox(formula.tex);
    // Inline
    lines.push(`% --- inline ${formula.id} ---`);
    lines.push(`\\setbox0=\\hbox{$${texEscaped}$}`);
    lines.push(`\\typeout{BOXMETRIC: inline ${formula.id} \\the\\wd0 \\the\\ht0 \\the\\dp0}`);
    // Display
    lines.push(`% --- display ${formula.id} ---`);
    lines.push(`\\setbox0=\\hbox{\\displaystyle $${texEscaped}$}`);
    lines.push(`\\typeout{BOXMETRIC: display ${formula.id} \\the\\wd0 \\the\\ht0 \\the\\dp0}`);
    lines.push("");
  }

  lines.push("\\end{document}");
  return lines.join("\n");
}

/**
 * Minimal TeX escaping for corpus formulas.
 *
 * Corpus entries use TeX math source only (no `\input`, no catcode tricks),
 * but we still brace-wrap to keep the hbox contents well-defined and escape
 * anything that would interact with outer TeX primitives unexpectedly.
 *
 * We do NOT try to sanitize malicious input — this harness runs on trusted
 * corpus data only.
 */
function escapeTexForHbox(tex) {
  return tex; // corpus is trusted, no escaping needed
}

/** Compile .tex with the selected engine; returns full log content. */
function compile(engineName, texPath, workDir) {
  const args = ["-interaction=nonstopmode", "-halt-on-error", "-no-shell-escape", texPath];
  const result = spawnSync(engineName, args, {
    cwd: workDir,
    encoding: "utf8",
    timeout: 300_000,
    windowsHide: true,
  });
  const logPath = resolve(workDir, "emit_boxes.log");
  let logContent = "";
  if (existsSync(logPath)) {
    try {
      logContent = readFileSync(logPath, "utf8");
    } catch (_e) {
      logContent = "";
    }
  }
  return {
    exit_code: result.status,
    stderr: result.stderr || "",
    log: logContent,
  };
}

// ---------------------------------------------------------------------------
// Log parsing
// ---------------------------------------------------------------------------

/**
 * Parse the TeX log for `\typeout{BOXMETRIC: <inline|display> <id> <wd>pt <ht>pt <dp>pt}` lines.
 *
 * TeX's `\the\wd0` emits e.g. `30.5pt` — possibly with a fractional part.
 * We parse with a regex that handles optional sign and fractional seconds.
 */
const BOXMETRIC_RE =
  /BOXMETRIC:\s+(inline|display)\s+(\S+)\s+([\d.]+)pt\s+([\d.]+)pt\s+([\d.]+)pt/gu;

function parseBoxMetrics(log) {
  const out = new Map(); // key = "<corpus_id>|<inline|display>"
  let m;
  while ((m = BOXMETRIC_RE.exec(log)) !== null) {
    const [, mode, id, wPt, hPt, dPt] = m;
    const key = `${id}|${mode}`;
    out.set(key, {
      width_pt: parseFloat(wPt),
      height_pt: parseFloat(hPt),
      depth_pt: parseFloat(dPt),
    });
  }
  return out;
}

/** Convert TeX pt to em (BASE_SIZE_PT = 10, normalsize = 10pt → 1em). */
function ptToEm(pt) {
  return pt / BASE_SIZE_PT;
}

// ---------------------------------------------------------------------------
// Available record (engine produced numbers)
// ---------------------------------------------------------------------------

function recordAvailable(formula, displayMode, metrics, fontInfo, engineVersion, command) {
  const key = `${formula.id}|${displayMode ? "display" : "inline"}`;
  const m = metrics.get(key);
  if (!m) return null;

  return {
    schema_version: "v0",
    corpus_id: formula.id,
    tex: formula.tex,
    display_mode: displayMode,
    source: {
      engine: "latex",
      version: engineVersion,
      command,
      status: "available",
      status_reason: null,
    },
    font: fontInfo,
    size: BASE_SIZE_PT,
    normalized_atoms: null,
    style_cramped: null,
    width: ptToEm(m.width_pt),
    height: ptToEm(m.height_pt),
    depth: ptToEm(m.depth_pt),
    axis: null,
    ink_bounds: null,
    rule_thickness: null,
    delimiter_size: null,
    script_positions: null,
    mathml: null,
  };
}

/**
 * Best-effort font identification: we cannot easily get TeX to tell us which
 * .otf it loaded for Latin Modern Math from the command line, so we only
 * populate family + units_per_em here and leave sha256/path null.  A future
 * improvement can use `\the\fontname` or inspect the log.
 */
function defaultFontInfo(engineFamily) {
  const family = engineFamily === "tectonic" ? "Latin Modern Math" : "Latin Modern Math";
  return {
    family,
    sha256: null,
    units_per_em: 1000,
    path: null,
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const corpus = JSON.parse(readFileSync(CORPUS_PATH, "utf8"));

  // Probe engine candidates.
  const engineProbes = ENGINE_CANDIDATES.map(probe);
  const treeProbes = TREE_PROBES.map(probe);
  const allProbes = [...engineProbes, ...treeProbes];
  const primary = engineProbes.find((p) => p.found && p.exit_code === 0) ?? null;
  const amsmath = probeAmsmath(treeProbes.find((p) => p.name === "kpsewhich"));

  // --- Blocked path ----------------------------------------------------------
  if (!primary) {
    const reason = blockedReason(allProbes, primary);
    const records = [];
    for (const formula of corpus.formulas) {
      for (const displayMode of [false, true]) {
        records.push(recordBlocked(formula, displayMode, reason, COMMAND));
      }
    }
    const doc = {
      schema_version: "v0",
      source: {
        engine: "latex",
        version: null,
        command: COMMAND,
        status: "blocked",
        status_reason: reason,
      },
      corpus_path: "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
      procedure: PROCEDURE,
      probes: allProbes,
      amsmath,
      records,
    };
    mkdirSync(dirname(OUT_PATH), { recursive: true });
    writeFileSync(OUT_PATH, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
    console.log(`wrote ${OUT_PATH}`);
    console.log(`latex oracle status: ${doc.source.status}`);
    console.log(`reason: ${reason}`);
    for (const p of allProbes) {
      console.log(`  ${p.name}: ${p.found ? `found (${p.version_line})` : `missing (${p.error})`}`);
    }
    return;
  }

  // --- Available path --------------------------------------------------------
  const workDir = mkdtempSync(resolve(platform() === "win32" ? process.env.TEMP || "." : "/tmp", "vutex-latex-"));
  try {
    const texPath = resolve(workDir, "emit_boxes.tex");
    writeFileSync(texPath, buildTexDocument(corpus), "utf8");

    const { exit_code, stderr, log } = compile(primary.name, texPath, workDir);

    if (exit_code !== 0) {
      const reason = [
        `\`${primary.name}\` ran but exited ${exit_code}.`,
        "First 20 lines of stderr:",
        stderr.split(/\r?\n/).slice(0, 20).join("\n"),
      ].join("\n");
      // emit blocked with the engine-failed reason
      const records = [];
      for (const formula of corpus.formulas) {
        for (const displayMode of [false, true]) {
          records.push(recordBlocked(formula, displayMode, reason, COMMAND));
        }
      }
      const doc = {
        schema_version: "v0",
        source: {
          engine: "latex",
          version: primary.version_line,
          command: COMMAND,
          status: "blocked",
          status_reason: reason,
        },
        corpus_path: "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
        procedure: PROCEDURE,
        probes: allProbes,
        amsmath,
        records,
      };
      mkdirSync(dirname(OUT_PATH), { recursive: true });
      writeFileSync(OUT_PATH, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
      console.log(`wrote ${OUT_PATH}`);
      console.log(`latex oracle status: blocked (engine exited ${exit_code})`);
      console.log(reason);
      return;
    }

    const metrics = parseBoxMetrics(log);
    const fontInfo = defaultFontInfo(primary.family);
    const engineVersion = primary.version_line;

    const records = [];
    let missingKeys = [];
    for (const formula of corpus.formulas) {
      for (const displayMode of [false, true]) {
        const rec = recordAvailable(
          formula,
          displayMode,
          metrics,
          fontInfo,
          engineVersion,
          COMMAND,
        );
        if (rec) records.push(rec);
        else missingKeys.push(`${formula.id}|${displayMode ? "display" : "inline"}`);
      }
    }

    if (records.length === 0) {
      const reason = [
        `\`${primary.name}\` ran successfully but the log contained zero BOXMETRIC lines.`,
        "The harness may need to be rebuilt for this engine.",
      ].join(" ");
      // fallback blocked
      const blockedRecords = [];
      for (const formula of corpus.formulas) {
        for (const displayMode of [false, true]) {
          blockedRecords.push(recordBlocked(formula, displayMode, reason, COMMAND));
        }
      }
      const doc = {
        schema_version: "v0",
        source: {
          engine: "latex",
          version: primary.version_line,
          command: COMMAND,
          status: "blocked",
          status_reason: reason,
        },
        corpus_path: "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
        procedure: PROCEDURE,
        probes: allProbes,
        amsmath,
        records: blockedRecords,
      };
      mkdirSync(dirname(OUT_PATH), { recursive: true });
      writeFileSync(OUT_PATH, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
      console.log(`wrote ${OUT_PATH}`);
      console.log(`latex oracle status: blocked (no metrics parsed)`);
      return;
    }

    const doc = {
      schema_version: "v0",
      source: {
        engine: "latex",
        version: engineVersion,
        command: COMMAND,
        status: "available",
        status_reason: null,
      },
      corpus_path: "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
      procedure: PROCEDURE,
      probes: allProbes,
      amsmath,
      records,
    };

    mkdirSync(dirname(OUT_PATH), { recursive: true });
    writeFileSync(OUT_PATH, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
    console.log(`wrote ${OUT_PATH}`);
    console.log(`latex oracle status: available`);
    console.log(`engine: ${primary.name} (${engineVersion})`);
    console.log(`records: ${records.length}`);
    if (missingKeys.length > 0) {
      console.log(`warning: missing metrics for ${missingKeys.join(", ")}`);
    }
  } finally {
    rmSync(workDir, { recursive: true, force: true });
  }
}

main();
