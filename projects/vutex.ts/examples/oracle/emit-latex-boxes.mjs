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
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { platform } from "node:os";
import { basename, dirname, resolve } from "node:path";
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

/**
 * Locate `amsmath.sty` inside the Tectonic bundle cache.
 *
 * Tectonic is the reference engine here and ships its TeX tree as a bundle
 * snapshot, so the macro versions come from the bundle, not from a TeX Live
 * installation.  The cache unpacks every bundle file under
 * `<cache>/bundles/data/<bundle-sha256>/`; the directory name is the bundle
 * tar's SHA-256, which doubles as the bundle identifier this oracle pins.
 */
function tectonicBundleAmsmath() {
  const home = process.env.HOME ?? process.env.USERPROFILE ?? "";
  const candidates = [
    process.env.TECTONIC_CACHE_DIR,
    process.env.LOCALAPPDATA ? resolve(process.env.LOCALAPPDATA, "TectonicProject", "Tectonic", "cache") : null,
    home ? resolve(home, ".cache", "TectonicProject", "Tectonic", "cache") : null,
    home ? resolve(home, ".cache", "tectonic") : null,
  ].filter(Boolean);
  for (const cacheDir of candidates) {
    const dataDir = resolve(cacheDir, "bundles", "data");
    let entries = [];
    try {
      entries = readdirSync(dataDir);
    } catch (_err) {
      continue;
    }
    for (const entry of entries) {
      if (!/^[0-9a-f]{64}$/u.test(entry)) continue;
      const stylePath = resolve(dataDir, entry, "amsmath.sty");
      try {
        const style = readFileSync(stylePath, "utf8");
        const m = style.match(/\\ProvidesPackage\{amsmath\}\[([^\]]*)\]/);
        if (!m) continue;
        return {
          name: "amsmath",
          found: true,
          path: stylePath,
          version: m[1].trim(),
          reason: `located in the Tectonic bundle cache (bundle tar sha256 ${entry})`,
        };
      } catch (_err) {
        continue;
      }
    }
  }
  return null;
}

/** Probe amsmath.sty via kpsewhich when it exists, else via the Tectonic bundle cache. */
function probeAmsmath(kpseProbe) {
  if (!kpseProbe.found) {
    const bundled = tectonicBundleAmsmath();
    if (bundled) return bundled;
    return {
      name: "amsmath",
      found: false,
      path: null,
      version: null,
      reason: "`kpsewhich` not on PATH and no Tectonic bundle cache found — cannot locate amsmath.sty",
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
  "`ink_bounds` stays null: TeX box metrics are advance boxes and the harness does not extract glyph outlines from the engine's PDF.",
  "For every formula carrying a radical degree (`\\sqrt[...]{...}`), also measure the naked radical and the degree content in both superscript-demoted candidate styles with `\\typeout{NAKED: ...}` / `\\typeout{DEGBOX: ...}`.",
  "Then run one dedicated `\\showbox` pass per degree entry (with `\\showboxdepth` and `\\showboxbreadth` raised): the engine's box-structure dump reveals the degree assembly `kern, shifted degree box, kern`, so the degree anchor, box and kerns are engine output, not reconstruction. `\\showbox` halts the engine (Tectonic treats it as an error), hence the dedicated pass and `--keep-logs`.",
  "For script entries whose script box is the trailing item of the formula box, measure the script content alone in its own style (`\\typeout{SCRIPTBOX: ...}`) and derive the script anchor from engine box differences: x = wd(full) - wd(script box), y = ht(full) - ht(script box) for a superscript.",
  "For fenced entries, measure the left delimiter stretched over a zero-width phantom of the body (`\\typeout{DELIMSIZE: ...}`): the phantom keeps the body's height and depth, so the delimiter grows exactly as in the full formula, and its height plus depth is the recorded `delimiter_size`.",
  "Probe the math axis once in math mode with `\\typeout{AXIS: \\the\\fontdimen22\\textfont2}`: TeX centres fractions, brackets and large operators on that height, and it is the engine's own number, not a design assumption.",
  "For every formula whose source draws a rule (fraction bar, radical vinculum, over/underbar), reuse the dedicated `\\showbox` pass: each bar appears as a `\\rule(<height>+<depth>)x<width>` node, so the recorded `rule_thickness` is the engine's drawn thickness (height plus depth), not a constant copied from a table.",
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

/**
 * A record the reference engine *ran* but rejected.
 *
 * Unlike a `blocked` record (no engine in this environment), an `unsupported`
 * record proves the engine executed and refused this specific corpus entry, so
 * the null measurement carries the engine's own message as evidence.  The
 * down-stream geometry gate must treat such a record as "not measurable by the
 * reference", never as "measurable and zero".
 */
function recordUnsupported(formula, displayMode, reason, command, engineVersion) {
  return {
    schema_version: "v0",
    corpus_id: formula.id,
    tex: formula.tex,
    display_mode: displayMode,
    source: {
      engine: "latex",
      version: engineVersion,
      command,
      status: "unsupported",
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

/**
 * Script entries whose script box is the trailing item of the formula box and
 * determines the formula's vertical extent.  The script anchor then derives
 * from pure engine box differences (no reconstruction): for a superscript
 * x = wd(full) - wd(script box) and y = ht(full) - ht(script box).  An entry
 * joins this table only while that shape holds for its source.
 */
const SCRIPT_BODIES = new Map([
  ["script-nested", { role: "sup", body: "a_b" }],
  ["script-tower", { role: "sup", body: "10^{10}" }],
  ["script-negative", { role: "sup", body: "-x^2" }],
]);

/**
 * Fenced entries whose left delimiter stretches over the body.  The companion
 * box sets the same delimiter over a zero-width `\vphantom` of the body — the
 * phantom keeps the body's height and depth, so TeX sizes the delimiter
 * exactly as in the full formula, and the companion box height plus depth is
 * the delimiter's stretched extent.
 */
const DELIMITER_BODIES = new Map([
  ["fenced-fraction", { left: "(", body: "\\frac{a}{b}" }],
  ["brace-fenced-fraction", { left: "\\{", body: "\\frac{a}{b}" }],
  ["fenced-norm", { left: "\\lVert", body: "x" }],
  ["angle-bracket", { left: "\\langle", body: "v" }],
]);

/**
 * Corpus entries the reference engine cannot set inside `\hbox{$...$}`.
 *
 * Every other environment (`matrix`, `cases`, `aligned`, `alignedat`,
 * `gathered`, `smallmatrix`, `bmatrix`, `vmatrix`, `Vmatrix` and a fenced
 * matrix) sets fine in both styles — amsmath builds those boxes from their own
 * alignment context, so a box is enough.
 *
 * `split` is the exception: amsmath defines it as a display-only environment
 * and rejects inner math outright (`\ifinner` → `\@badmath`).  A `\hbox`
 * is always inner math, in both `$...$` and `\displaystyle $...$`, so the
 * engine runs and refuses the entry while still measuring everything else.
 * The record keeps the engine's own message as `status: "unsupported"`.
 */
const UNMEASURABLE_IN_BOX = new Map([
  [
    "split",
    "Package amsmath Error: \\begin{split} won't work here. " +
      "(amsmath rejects `split` in inner math, and a box is always inner math, in both `$...$` and `\\displaystyle $...$`)",
  ],
]);

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

  // Math axis height: `\\fontdimen22` of the symbol font is the height TeX
  // centres fractions, brackets and large operators on.  Measured once, in
  // math mode, because the `\\textfont` registers only exist there.
  lines.push("\\setbox0=\\hbox{$\\typeout{AXIS: \\the\\fontdimen22\\textfont2}$}");

  for (const formula of corpus.formulas) {
    // Only entries the engine structurally refuses are skipped; a box gives
    // every amsmath alignment environment the context it needs.
    if (UNMEASURABLE_IN_BOX.has(formula.id)) continue;

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

    // Script companion: the script content alone in its own style.  Scripts
    // keep their style across inline/display, so one measurement serves both.
    const script = SCRIPT_BODIES.get(formula.id);
    if (script) {
      lines.push(`% --- script companion ${formula.id} ---`);
      lines.push(`\\setbox4=\\hbox{$\\scriptstyle ${script.body}$}`);
      lines.push(`\\typeout{SCRIPTBOX: ${formula.id} ${script.role} \\the\\wd4 \\the\\ht4 \\the\\dp4}`);
    }

    // Delimiter companion: the left fence stretched over a phantom body, in
    // both styles (display fractions grow taller and stretch the fence more).
    const delim = DELIMITER_BODIES.get(formula.id);
    if (delim) {
      lines.push(`% --- delimiter companion ${formula.id} ---`);
      lines.push(`\\setbox5=\\hbox{$\\left${delim.left}\\vphantom{${delim.body}}\\right.$}`);
      lines.push(`\\typeout{DELIMSIZE: inline ${formula.id} \\the\\ht5 \\the\\dp5}`);
      lines.push(`\\setbox5=\\hbox{$\\displaystyle\\left${delim.left}\\vphantom{${delim.body}}\\right.$}`);
      lines.push(`\\typeout{DELIMSIZE: display ${formula.id} \\the\\ht5 \\the\\dp5}`);
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
  const logPath = resolve(workDir, basename(texPath).replace(/\.tex$/u, ".log"));
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

// TeX swallows the space after each `\the\wdN` control word, so the emitted
// dimensions are glued together (`8.00432pt3.01389pt1.06111pt`) — the `pt`
// suffix is the only separator.
const SCRIPTBOX_RE = /^SCRIPTBOX: (\S+) (sup|sub) ([\d.]+pt)([\d.]+pt)([\d.]+pt)$/gm;
const DELIMSIZE_RE = /^DELIMSIZE: (inline|display) (\S+) ([\d.]+pt)([\d.]+pt)$/gm;

/** Parse `SCRIPTBOX` lines: the script content box in its own style. */
function parseScriptBoxes(log) {
  const out = new Map(); // key = corpus_id → { role, width, height, depth } (em)
  let m;
  SCRIPTBOX_RE.lastIndex = 0;
  while ((m = SCRIPTBOX_RE.exec(log)) !== null) {
    const [, id, role, wPt, hPt, dPt] = m;
    out.set(id, {
      role,
      width: ptToEm(parseFloat(wPt)),
      height: ptToEm(parseFloat(hPt)),
      depth: ptToEm(parseFloat(dPt)),
    });
  }
  return out;
}

/** Parse `DELIMSIZE` lines: the stretched left delimiter extent per mode. */
function parseDelimiterSizes(log) {
  const out = new Map(); // key = "<corpus_id>|<inline|display>" → extent (em)
  let m;
  DELIMSIZE_RE.lastIndex = 0;
  while ((m = DELIMSIZE_RE.exec(log)) !== null) {
    const [, mode, id, hPt, dPt] = m;
    out.set(`${id}|${mode}`, ptToEm(parseFloat(hPt) + parseFloat(dPt)));
  }
  return out;
}

/**
 * Derive the script anchor from engine box differences.  The script box is
 * the trailing item of the formula box and determines its vertical extent,
 * so for a superscript x = wd(full) - wd(script) and y = ht(full) - ht(script).
 */
function buildScriptPositions(script, scriptBox, full) {
  if (!script || !scriptBox || !full) return null;
  if (script.role === "sup") {
    return [{ role: "sup", x: full.width - scriptBox.width, y: full.height - scriptBox.height }];
  }
  return [{ role: "sub", x: full.width - scriptBox.width, y: -(full.depth - scriptBox.depth) }];
}

/** Convert TeX pt to em (BASE_SIZE_PT = 10, normalsize = 10pt → 1em). */
function ptToEm(pt) {
  return pt / BASE_SIZE_PT;
}

// ---------------------------------------------------------------------------
// Column grid (start offsets, slot widths and content offsets)
// ---------------------------------------------------------------------------

/**
 * Corpus entries whose engine structure is a plain `\array`: one padded cell
 * hbox per semantic column, no `\hskip-\arraycolsep` interleaving.  `aligned`,
 * `alignedat` and `split` are excluded on purpose — amsmath expands their
 * `rl` grid with `\tabskip` plus negative-width hboxes, so the dump's hboxes
 * do not map one-to-one onto the semantic columns.
 */
const COLUMN_GRID_IDS = new Set(["matrix", "smallmatrix", "cases", "gathered", "bmatrix", "vmatrix", "Vmatrix", "fenced-matrix"]);

/** `\hbox(6.9+1.0)x12.3, glue set 0.25fil` and friends. */
const DUMP_LINE_RE = /^(\.*)\\([A-Za-z@]+)(.*)$/u;
const DUMP_WIDTH_RE = /x(-?[\d.]+)/u;
const DUMP_GLUE_SET_RE = /glue set (-?[\d.]+)fil/u;
/** `\glue 5.0`, `\glue(\tabskip) 0.0` and `\glue -5.0` carry their natural width. */
const DUMP_GLUE_WIDTH_RE = /^\s*(?:\(\\?tabskip\)\s*)?(-?[\d.]+)/u;
/** `\kern 2.77695` carries its natural width the same way. */
const DUMP_KERN_WIDTH_RE = /^\s*(-?[\d.]+)/u;

/** Split a `\showbox` dump into indent-annotated nodes. */
function parseDumpNodes(log) {
  const nodes = [];
  for (const raw of log.split(/\r?\n/u)) {
    const m = DUMP_LINE_RE.exec(raw);
    if (!m) continue;
    const rest = m[3] ?? "";
    const boxWidth = DUMP_WIDTH_RE.exec(rest);
    const fixedWidth = m[2] === "glue" ? DUMP_GLUE_WIDTH_RE.exec(rest) : m[2] === "kern" ? DUMP_KERN_WIDTH_RE.exec(rest) : null;
    const glueSet = DUMP_GLUE_SET_RE.exec(rest);
    nodes.push({
      depth: m[1].length,
      type: m[2],
      width: boxWidth ? parseFloat(boxWidth[1]) : fixedWidth ? parseFloat(fixedWidth[1]) : null,
      glueSet: glueSet ? parseFloat(glueSet[1]) : null,
      tabskip: m[2] === "glue" && rest.includes("tabskip"),
      fil: m[2] === "glue" && rest.includes("fil"),
    });
  }
  return nodes;
}

/** Direct children of the node at `index` (same dump, deeper by one dot). */
function dumpChildren(nodes, index) {
  const children = [];
  const depth = nodes[index].depth + 1;
  for (let i = index + 1; i < nodes.length; i++) {
    if (nodes[i].depth < depth) break;
    if (nodes[i].depth === depth) children.push(i);
  }
  return children;
}

/**
 * Parse the engine's column grid out of one `\showbox` dump.
 *
 * Returns `{ columns, rows: [{ starts, widths, offsets }] }` in em, or null
 * when the dump carries no array-like vbox.  `starts` is the x of each padded
 * cell hbox inside its row, `widths` the padded slot widths and `offsets` the
 * content's x inside the slot (left `\arraycolsep` padding plus the alignment
 * slack the engine gave that cell).  The array is recognised by its
 * `\glue(\tabskip)` separators, which no other construct emits.
 */
function parseColumnGrid(log) {
  const nodes = parseDumpNodes(log);
  for (let i = 0; i < nodes.length; i++) {
    if (nodes[i].type !== "vbox") continue;
    const rowIndices = dumpChildren(nodes, i).filter((child) => nodes[child].type === "hbox");
    if (rowIndices.length === 0) continue;
    const isArray = rowIndices.every((rowIndex) => dumpChildren(nodes, rowIndex).some((child) => nodes[child].tabskip));
    if (!isArray) continue;

    const rows = [];
    for (const rowIndex of rowIndices) {
      const children = dumpChildren(nodes, rowIndex);
      const starts = [];
      const widths = [];
      const offsets = [];
      let cursor = 0.0;
      for (const child of children) {
        const node = nodes[child];
        if (node.type === "hbox") {
          const innerChildren = dumpChildren(nodes, child);
          const ratio = node.glueSet ?? 0.0;
          let offset = 0.0;
          for (const inner of innerChildren) {
            const innerNode = nodes[inner];
            if (innerNode.type === "glue") {
              if (innerNode.tabskip) continue;
              offset += innerNode.width ?? 0.0;
              if (innerNode.fil) offset += ratio;
            } else if (innerNode.type === "kern" || innerNode.type === "rule" || innerNode.type === "penalty") {
              offset += innerNode.width ?? 0.0;
            } else {
              break;
            }
          }
          starts.push(ptToEm(cursor));
          widths.push(ptToEm(node.width ?? 0.0));
          offsets.push(ptToEm(offset));
        }
        cursor += node.width ?? 0.0;
      }
      if (starts.length === 0) return null;
      rows.push({ starts, widths, offsets });
    }
    const columns = rows.reduce((most, row) => Math.max(most, row.starts.length), 0);
    return { columns, rows };
  }
  return null;
}

/** `\typeout{AXIS: <value>pt}` — the engine's own `\fontdimen22\textfont2`. */
const AXIS_RE = /AXIS:\s+(-?[\d.]+)pt/u;

/** Corpus sources whose markup makes the engine draw a rule. */
const RULE_SOURCE_RE = /\\(?:frac|dfrac|tfrac|sqrt|overline|underline|overbrace|underbrace)\b/u;

/**
 * Whether the formula's source contains a construct TeX draws with a rule
 * (fraction bar, radical vinculum, over/underbar).  Decided from the corpus
 * source, so the harness never guesses from a rendered picture.
 */
function ruleBearing(tex) {
  return RULE_SOURCE_RE.test(tex);
}

/**
 * Parse the engine's math axis height in em.  Returns null when the probe line
 * is missing, so a lost measurement stays null instead of becoming zero.
 */
function parseAxis(log) {
  const m = log.match(AXIS_RE);
  return m ? ptToEm(parseFloat(m[1])) : null;
}

// `\showbox` prints every rule node as `\rule(<height>+<depth>)x<width>`; the
// width is `*` when the rule was stretched to its enclosing box.
const RULE_RE = /\\rule\(([\d.]+)\+([\d.]+)\)x/gu;

/**
 * Parse the rule nodes of one `\showbox` dump and return the thickest rule in
 * em, or null when the dump carries none.
 *
 * TeX draws fraction bars, radical vinculums and over/underbars with the same
 * `\rule` primitive, so the dump is the engine's own statement about every
 * bar it drew.  A nested fraction legitimately carries two rules; the
 * thickest one is the outermost (base-style) bar, which is what the fixture's
 * single `rule_thickness` field records.  All observed thicknesses are
 * returned alongside so the parity gate can report the script-style value
 * instead of guessing it.
 */
function parseRuleThickness(log) {
  const thicknesses = [];
  RULE_RE.lastIndex = 0;
  let m;
  while ((m = RULE_RE.exec(log)) !== null) {
    thicknesses.push(ptToEm(parseFloat(m[1]) + parseFloat(m[2])));
  }
  if (thicknesses.length === 0) return null;
  return { max: Math.max(...thicknesses), all: thicknesses };
}

// ---------------------------------------------------------------------------
// Available record (engine produced numbers)
// ---------------------------------------------------------------------------

function recordAvailable(
  formula,
  displayMode,
  metrics,
  fontInfo,
  engineVersion,
  command,
  degree = null,
  scriptPositions = null,
  delimiterSize = null,
  axis = null,
  ruleThickness = null,
  columnAlignment = null,
) {
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
    axis,
    ink_bounds: null,
    rule_thickness: ruleThickness,
    column_alignment: columnAlignment,
    delimiter_size: delimiterSize,
    script_positions: scriptPositions,
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
    const scriptBoxes = parseScriptBoxes(combined);
    const delimiterSizes = parseDelimiterSizes(combined);
    const axisEm = parseAxis(combined);
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

    // Pass B: dedicated `\showbox` runs for every formula the engine draws a
    // radical degree or a rule for.  The dump reveals the assembly the engine
    // itself built (kerns, shifted degree box, surd, radicand) and every `\rule`
    // node it drew, so the degree anchor and the bar thickness are engine
    // output.  `\showbox` halts the engine, so each run lives in its own
    // directory and a non-zero exit is expected.
    const assemblies = new Map(); // key = "<corpus_id>|<inline|display>"
    const ruleThicknesses = new Map(); // key = "<corpus_id>|<inline|display>"
    const columnGrids = new Map(); // key = "<corpus_id>|<inline|display>"
    for (const formula of corpus.formulas) {
      const parts = degreeParts(formula.tex);
      if (!parts && !ruleBearing(formula.tex) && !COLUMN_GRID_IDS.has(formula.id)) continue;
      for (const displayMode of [false, true]) {
        const mode = displayMode ? "display" : "inline";
        const key = `${formula.id}|${mode}`;
        const subDir = resolve(workDir, `show_${formula.id}_${mode}`);
        mkdirSync(subDir, { recursive: true });
        const sbTexPath = resolve(subDir, "box_showbox.tex");
        writeFileSync(sbTexPath, buildShowboxDocument(formula.tex, displayMode), "utf8");
        const sb = compileShowbox(primary.name, sbTexPath, subDir);
        const dump = sb.log + "\n" + sb.stderr;
        const assembly = parseDegreeAssembly(dump);
        if (assembly) assemblies.set(key, assembly);
        else if (parts) console.log(`warning: showbox pass produced no degree assembly for ${formula.id} (${mode})`);
        const rules = ruleBearing(formula.tex) ? parseRuleThickness(dump) : null;
        if (rules) ruleThicknesses.set(key, rules);
        else if (ruleBearing(formula.tex)) console.log(`warning: showbox pass produced no rule for ${formula.id} (${mode})`);
        const grid = COLUMN_GRID_IDS.has(formula.id) ? parseColumnGrid(dump) : null;
        if (grid) columnGrids.set(key, grid);
        else if (COLUMN_GRID_IDS.has(formula.id)) console.log(`warning: showbox pass produced no column grid for ${formula.id} (${mode})`);
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
        const script = SCRIPT_BODIES.get(formula.id);
        const scriptPositions = m && script
          ? buildScriptPositions(script, scriptBoxes.get(formula.id) ?? null, {
              width: ptToEm(m.width_pt),
              height: ptToEm(m.height_pt),
              depth: ptToEm(m.depth_pt),
            })
          : null;
        const delimiterSize = delimiterSizes.get(key) ?? null;
        const ruleThickness = ruleThicknesses.get(key)?.max ?? null;
        const rec = recordAvailable(
          formula,
          displayMode,
          metrics,
          fontInfo,
          engineVersion,
          COMMAND,
          degree,
          scriptPositions,
          delimiterSize,
          axisEm,
          ruleThickness,
          columnGrids.get(key) ?? null,
        );
        if (rec) {
          records.push(rec);
          continue;
        }
        const rejected = UNMEASURABLE_IN_BOX.get(formula.id);
        if (rejected) {
          records.push(recordUnsupported(formula, displayMode, rejected, COMMAND, engineVersion));
        }
        else {
          missingKeys.push(key);
        }
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
