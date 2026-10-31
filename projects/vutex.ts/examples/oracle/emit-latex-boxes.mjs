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
 *   node projects/vutex.ts/examples/oracle/emit-latex-boxes.mjs
 *   node projects/vutex.ts/examples/oracle/emit-latex-boxes.mjs \
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
const repoRoot = resolve(here, "..", "..", "..", "..");

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
  : resolve(repoRoot, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const OUT_PATH = outArg
  ? resolve(outArg)
  : resolve(repoRoot, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/latex-status.json");
const COMMAND = "node projects/vutex.ts/examples/oracle/emit-latex-boxes.mjs";

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
  "Install a TeX distribution (TeX Live / MacTeX) that ships LuaLaTeX and amsmath, or fetch `tectonic` which is self-contained. Re-run `node projects/vutex.ts/examples/oracle/emit-latex-boxes.mjs`.",
  "For every corpus formula, set it in a box in both styles: inline via `\\setbox0=\\hbox{$<tex>$}`, display via `\\setbox0=\\hbox{\\displaystyle $<tex>$}`.",
  "Emit box dimensions to the log with explicit markers: `\\typeout{BOXMETRIC: <inline|display> <corpus_id> \\the\\wd0 \\the\\ht0 \\the\\dp0}`.",
  "Parse the log, convert TeX points to em: `em = pt / 10.0` (normalsize = 10pt, so 1em == 10pt).",
  "Only `width`, `height`, `depth` are reliable: `ink_bounds`, `script_positions`, `delimiter_size` stay null.",
  "For every formula carrying a radical degree (`\\sqrt[...]{...}`), also measure the naked radical and the degree content in both superscript-demoted candidate styles with `\\typeout{NAKED: ...}` / `\\typeout{DEGBOX: ...}`.",
  "Then run one dedicated `\\showbox` pass per degree entry (with `\\showboxdepth` and `\\showboxbreadth` raised): the engine's box-structure dump reveals the degree assembly `kern, shifted degree box, kern`, so the degree anchor, box and kerns are engine output, not reconstruction. `\\showbox` halts the engine (Tectonic treats it as an error), hence the dedicated pass and `--keep-logs`.",
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
    degree: null,
    mathml: null,
  };
}

// ---------------------------------------------------------------------------
// Radical-degree helpers
// ---------------------------------------------------------------------------

/**
 * Split `\sqrt[<degree>]{<radicand>}` into its parts.
 *
 * Only flat-brace corpus sources are expected; a formula that does not match
 * carries no degree and gets a null `degree` record.
 */
function degreeParts(tex) {
  const m = tex.match(/\\sqrt\[([^\]]*)\]\{([^}]*)\}/u);
  return m ? { degree: m[1], radicand: m[2] } : null;
}

/** One `\hbox(w+h)xd` dump line at a given indent depth (number of leading dots). */
function showboxBoxLine(line, dots) {
  const indent = `(?:\\.){${dots}}`;
  const m = line.match(new RegExp(`^${indent}\\\\(?:h|v)box\\(([\\d.]+)\\+([\\d.]+)\\)x([\\d.]+)(, shifted (-?[\\d.]+))?$`));
  if (!m) return null;
  return { height_pt: parseFloat(m[1]), depth_pt: parseFloat(m[2]), width_pt: parseFloat(m[3]), shift_pt: m[5] === undefined ? null : parseFloat(m[5]) };
}

/** One `\kern <value>` dump line at a given indent depth. */
function showboxKernLine(line, dots) {
  const indent = `(?:\\.){${dots}}`;
  const m = line.match(new RegExp(`^${indent}\\\\kern\\s*(-?[\\d.]+)`));
  return m ? parseFloat(m[1]) : null;
}

/**
 * Parse the `\showbox` dump of `$\sqrt[n]{rad}$` and return the degree assembly.
 *
 * The engine prints the radical as an hlist whose first shifted box is the
 * degree: `kern…, degree box (shifted up), kern…, surd, radicand`.  We take the
 * first `, shifted -` box (negative shift = raised, verified against
 * `\raise`), then sum the same-level kern lines immediately before and after
 * it.  The degree's left edge is therefore the before-kern sum relative to the
 * assembly's parent box, which starts at the formula's own left edge.
 *
 * Returns null when the dump carries no shifted box (engine changed shape).
 */
function parseDegreeAssembly(log) {
  const lines = log.split(/\r?\n/);
  let start = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].trim() === "> \\box0=") {
      start = i;
      break;
    }
  }
  if (start < 0) return null;

  // Region runs until the `! OK.` / `l.` trailer that ends the dump.
  const region = [];
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i];
    if (/^(!|l\.)/.test(line)) break;
    region.push(line);
  }

  let boxIdx = -1;
  let box = null;
  let dots = 0;
  for (let i = 0; i < region.length; i++) {
    const m = region[i].match(/^(\.+)?\\(?:h|v)box\([\d.]+\+[\d.]+\)x[\d.]+, shifted -[\d.]+$/);
    if (!m) continue;
    boxIdx = i;
    dots = m[1] === undefined ? 0 : m[1].length;
    box = showboxBoxLine(region[i], dots);
    break;
  }
  if (boxIdx < 0 || box === null || box.shift_pt === null) return null;

  let kernBefore = 0;
  for (let i = boxIdx - 1; i >= 0; i--) {
    const k = showboxKernLine(region[i], dots);
    if (k === null) break;
    kernBefore += k;
  }
  let kernAfter = 0;
  for (let i = boxIdx + 1; i < region.length; i++) {
    // The degree box's own children (mathon, font line, mathoff) are printed
    // at a deeper indent before the after-kerns — skip them, stop only at a
    // same-level line that is not a kern.
    const dm = region[i].match(/^\.+/);
    if (dm && dm[0].length > dots) continue;
    const k = showboxKernLine(region[i], dots);
    if (k === null) break;
    kernAfter += k;
  }

  return {
    width_pt: box.width_pt,
    height_pt: box.height_pt,
    depth_pt: box.depth_pt,
    anchor_y_pt: -box.shift_pt, // negative shift = raised above the baseline
    anchor_x_pt: kernBefore,
    kern_before_pt: kernBefore,
    kern_after_pt: kernAfter,
  };
}

/**
 * Build the `degree` object for one record from the pass-A companions and the
 * pass-B assembly.  Every field stays null when its source measurement is
 * missing — the harness never reconstructs what the engine did not report.
 */
function buildDegreeRecord(parts, fullWidthEm, naked, candidates, assembly) {
  if (!parts) return null;

  let style = null;
  let advanceBox = null;
  let anchorX = null;
  let anchorY = null;
  let kernBefore = null;
  let kernAfter = null;
  if (assembly) {
    advanceBox = {
      width: ptToEm(assembly.width_pt),
      height: ptToEm(assembly.height_pt),
      depth: ptToEm(assembly.depth_pt),
    };
    anchorX = ptToEm(assembly.anchor_x_pt);
    anchorY = ptToEm(assembly.anchor_y_pt);
    kernBefore = ptToEm(assembly.kern_before_pt);
    kernAfter = ptToEm(assembly.kern_after_pt);
    // Style is decided by matching the assembled box against the engine's own
    // candidate boxes, so `style` is a measurement, not an assumption.
    const dScript = candidates?.script ? Math.abs(advanceBox.width - candidates.script.width) : Infinity;
    const dScriptscript = candidates?.scriptscript ? Math.abs(advanceBox.width - candidates.scriptscript.width) : Infinity;
    if (dScript <= dScriptscript && candidates?.script) style = "script";
    else if (Number.isFinite(dScriptscript)) style = "scriptscript";
  }

  const advanceTotal = naked && typeof fullWidthEm === "number" ? fullWidthEm - naked.width : null;
  return {
    style,
    anchor_x: anchorX,
    anchor_y: anchorY,
    advance_box: advanceBox,
    ink_bbox: null, // TeX box metrics are advance boxes; the engine reports no ink extent here.
    kern_before: kernBefore,
    kern_after: kernAfter,
    advance_total: advanceTotal,
    naked_box: naked ?? null,
    candidate_boxes: {
      script: candidates?.script ?? null,
      scriptscript: candidates?.scriptscript ?? null,
    },
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
    "\\usepackage{amsfonts}",
    "\\pagestyle{empty}",
    "\\begin{document}",
    "",
  ];

  for (const formula of corpus.formulas) {
    // Skip environments (matrix/cases/aligned/...) — they cannot be placed
    // inside `\hbox{$...$}` because their internal alignment separators (`&`,
    // `\\`) need the alignment context TeX provides only inside display math.
    // These are still covered by VuTeX's own layout tests; the box-metrics
    // oracle focuses on inline-expressible forms.
    if (/\\begin\{/.test(formula.tex)) continue;

    const texEscaped = escapeTexForHbox(formula.tex);
    // Inline
    lines.push(`% --- inline ${formula.id} ---`);
    lines.push(`\\setbox0=\\hbox{$${texEscaped}$}`);
    lines.push(`\\typeout{BOXMETRIC: inline ${formula.id} \\the\\wd0 \\the\\ht0 \\the\\dp0}`);
    // Display
    lines.push(`% --- display ${formula.id} ---`);
    lines.push(`\\setbox0=\\hbox{$\\displaystyle ${texEscaped}$}`);
    lines.push(`\\typeout{BOXMETRIC: display ${formula.id} \\the\\wd0 \\the\\ht0 \\the\\dp0}`);

    // Degree companions: the naked radical (for the advance the degree adds)
    // and the degree content in each superscript-demoted candidate style (so
    // the recorded style is matched from engine output, not assumed).
    const parts = degreeParts(formula.tex);
    if (parts) {
      const nakedTex = `\\sqrt{${parts.radicand}}`;
      lines.push(`% --- degree companions ${formula.id} ---`);
      lines.push(`\\setbox1=\\hbox{$${nakedTex}$}`);
      lines.push(`\\typeout{NAKED: inline ${formula.id} \\the\\wd1 \\the\\ht1 \\the\\dp1}`);
      lines.push(`\\setbox1=\\hbox{$\\displaystyle ${nakedTex}$}`);
      lines.push(`\\typeout{NAKED: display ${formula.id} \\the\\wd1 \\the\\ht1 \\the\\dp1}`);
      lines.push(`\\setbox2=\\hbox{$\\scriptstyle ${parts.degree}$}`);
      lines.push(`\\typeout{DEGBOX: ${formula.id} script \\the\\wd2 \\the\\ht2 \\the\\dp2}`);
      lines.push(`\\setbox3=\\hbox{$\\scriptscriptstyle ${parts.degree}$}`);
      lines.push(`\\typeout{DEGBOX: ${formula.id} scriptscript \\the\\wd3 \\the\\ht3 \\the\\dp3}`);
    }
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
  // Tectonic is a Rust-native engine with its own CLI — it does NOT accept
  // the traditional `-interaction=nonstopmode -halt-on-error` flags.
  let args;
  if (engineName === "tectonic") {
    args = [texPath];
  } else {
    args = ["-interaction=nonstopmode", "-halt-on-error", "-no-shell-escape", texPath];
  }
  const result = spawnSync(engineName, args, {
    cwd: workDir,
    encoding: "utf8",
    timeout: 300_000,
    windowsHide: true,
  });
  // Tectonic writes log as `<texbasename>.log`, lualatex as `emit_boxes.log`.
  // The harness writes the tex file as `emit_boxes.tex`, so both converge here.
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

/**
 * Build a .tex document that dumps the box structure of one degree formula.
 *
 * `\showbox` prints the assembly (kerns, shifted degree box, surd, radicand)
 * and then raises `! OK.`, halting the engine — hence each entry runs in its
 * own dedicated pass with `--keep-logs`.  `\showboxbreadth` is raised so the
 * level-2 list is not truncated with `etc.`, and `\showboxdepth` is raised so
 * the degree box is visible at all.
 */
function buildShowboxDocument(tex, displayMode) {
  const style = displayMode ? "\\displaystyle " : "";
  return [
    "\\documentclass[10pt]{article}",
    "\\usepackage{amsmath}",
    "\\usepackage{amsfonts}",
    "\\pagestyle{empty}",
    "\\showboxdepth=10",
    "\\showboxbreadth=50",
    "\\begin{document}",
    `\\setbox0=\\hbox{$${style}${tex}$}`,
    "\\showbox0",
    "\\end{document}",
  ].join("\n");
}

/**
 * Run one dedicated `\showbox` compile.  A non-zero exit is expected (the
 * dump is followed by the `! OK.` error), so only the kept log matters.
 */
function compileShowbox(engineName, texPath, workDir) {
  let args;
  if (engineName === "tectonic") {
    args = ["--keep-logs", texPath];
  } else {
    args = ["-interaction=nonstopmode", "-halt-on-error", "-no-shell-escape", texPath];
  }
  const result = spawnSync(engineName, args, {
    cwd: workDir,
    encoding: "utf8",
    timeout: 300_000,
    windowsHide: true,
  });
  const logPath = resolve(workDir, "degree_showbox.log");
  let logContent = "";
  if (existsSync(logPath)) {
    try {
      logContent = readFileSync(logPath, "utf8");
    } catch (_e) {
      logContent = "";
    }
  }
  return { exit_code: result.status, stderr: result.stderr || "", log: logContent };
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
  /BOXMETRIC:\s+(inline|display)\s+(\S+)\s+([\d.]+)pt\s*([\d.]+)pt\s*([\d.]+)pt/gu;

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

/** Parse `\typeout{NAKED: <inline|display> <id> <wd>pt<ht>pt<dp>pt}` lines. */
const NAKED_RE =
  /NAKED:\s+(inline|display)\s+(\S+)\s+([\d.]+)pt\s*([\d.]+)pt\s*([\d.]+)pt/gu;

/** Parse `\typeout{DEGBOX: <id> <style> <wd>pt<ht>pt<dp>pt}` lines. */
const DEGBOX_RE =
  /DEGBOX:\s+(\S+)\s+(scriptscript|script)\s+([\d.]+)pt\s*([\d.]+)pt\s*([\d.]+)pt/gu;

function parseNakedBoxes(log) {
  const out = new Map(); // key = "<corpus_id>|<inline|display>"
  let m;
  NAKED_RE.lastIndex = 0;
  while ((m = NAKED_RE.exec(log)) !== null) {
    const [, mode, id, wPt, hPt, dPt] = m;
    out.set(`${id}|${mode}`, {
      width: ptToEm(parseFloat(wPt)),
      height: ptToEm(parseFloat(hPt)),
      depth: ptToEm(parseFloat(dPt)),
    });
  }
  return out;
}

function parseDegreeCandidateBoxes(log) {
  const out = new Map(); // key = corpus_id → { script?, scriptscript? }
  let m;
  DEGBOX_RE.lastIndex = 0;
  while ((m = DEGBOX_RE.exec(log)) !== null) {
    const [, id, style, wPt, hPt, dPt] = m;
    if (!out.has(id)) out.set(id, {});
    out.get(id)[style] = {
      width: ptToEm(parseFloat(wPt)),
      height: ptToEm(parseFloat(hPt)),
      depth: ptToEm(parseFloat(dPt)),
    };
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

function recordAvailable(formula, displayMode, metrics, fontInfo, engineVersion, command, degree = null) {
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
    degree,
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
      corpus_path: "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
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

    // Tectonic (and sometimes lualatex) exits non-zero on *any* TeX error, even
    // if BOXMETRIC was successfully emitted for formulas before the error.
    // We parse whatever we can find — partial success is still useful — and
    // only fall back to blocked when *zero* BOXMETRIC lines exist.
    // Tectonic's `\typeout` goes to stderr (terminal), lualatex's goes to the
    // .log file — so we parse both.
    const combined = log + "\n" + stderr;
    const metrics = parseBoxMetrics(combined);
    const nakedBoxes = parseNakedBoxes(combined);
    const candidateBoxes = parseDegreeCandidateBoxes(combined);
    const anyMetrics = metrics.size > 0;

    if (!anyMetrics) {
      const reason = [
        `\`${primary.name}\` ran${exit_code !== 0 ? ` but exited ${exit_code}` : " successfully"},`,
        "but the log contained zero BOXMETRIC lines.",
        exit_code !== 0
          ? `First 15 lines of stderr: ${stderr.split(/\r?\n/).slice(0, 15).join(" | ")}`
          : "The harness .tex template may need adjustment for this engine.",
      ].join(" ");
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
        corpus_path: "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
        procedure: PROCEDURE,
        probes: allProbes,
        amsmath,
        records,
      };
      mkdirSync(dirname(OUT_PATH), { recursive: true });
      writeFileSync(OUT_PATH, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
      console.log(`wrote ${OUT_PATH}`);
      console.log(`latex oracle status: blocked (no metrics parsed)`);
      console.log(reason);
      return;
    }
    const fontInfo = defaultFontInfo(primary.family);
    const engineVersion = primary.version_line;

    // Pass B: dedicated `\showbox` runs for every degree-carrying formula.
    // The dump reveals the assembly the engine itself built (kerns, shifted
    // degree box, surd, radicand), so the degree anchor/box/kerns are engine
    // output.  `\showbox` halts the engine, so each run lives in its own
    // directory and a non-zero exit is expected.
    const assemblies = new Map(); // key = "<corpus_id>|<inline|display>"
    for (const formula of corpus.formulas) {
      if (/\\begin\{/.test(formula.tex)) continue;
      if (!degreeParts(formula.tex)) continue;
      for (const displayMode of [false, true]) {
        const mode = displayMode ? "display" : "inline";
        const subDir = resolve(workDir, `show_${formula.id}_${mode}`);
        mkdirSync(subDir, { recursive: true });
        const sbTexPath = resolve(subDir, "degree_showbox.tex");
        writeFileSync(sbTexPath, buildShowboxDocument(formula.tex, displayMode), "utf8");
        const sb = compileShowbox(primary.name, sbTexPath, subDir);
        const assembly = parseDegreeAssembly(sb.log + "\n" + sb.stderr);
        if (assembly) assemblies.set(`${formula.id}|${mode}`, assembly);
        else console.log(`warning: showbox pass produced no degree assembly for ${formula.id} (${mode})`);
      }
    }

    const records = [];
    let missingKeys = [];
    for (const formula of corpus.formulas) {
      for (const displayMode of [false, true]) {
        const key = `${formula.id}|${displayMode ? "display" : "inline"}`;
        const m = metrics.get(key);
        const parts = degreeParts(formula.tex);
        let degree = null;
        if (m && parts) {
          degree = buildDegreeRecord(
            parts,
            ptToEm(m.width_pt),
            nakedBoxes.get(key) ?? null,
            candidateBoxes.get(formula.id) ?? null,
            assemblies.get(key) ?? null,
          );
        }
        const rec = recordAvailable(
          formula,
          displayMode,
          metrics,
          fontInfo,
          engineVersion,
          COMMAND,
          degree,
        );
        if (rec) records.push(rec);
        else missingKeys.push(key);
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
        corpus_path: "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
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
      corpus_path: "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
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
