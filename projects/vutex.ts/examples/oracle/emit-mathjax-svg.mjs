#!/usr/bin/env node
/**
 * Emit MathJax v3 SVG geometric summaries for the shared oracle corpus.
 *
 * ---------------------------------------------------------------------------
 * Why MathJax SVG and not KaTeX SVG?
 *
 * KaTeX 0.16.x's official `output` option only accepts `htmlAndMathml`,
 * `html` and `mathml` — there is no SVG output backend.  `svgGeometry.js`
 * inside KaTeX is an internal helper for delimiter sizing, not a renderer.
 * MathJax v3, by contrast, has a mature SVG output backend
 * (`output/svg`) that directly emits a well-formed `<svg>` per formula.
 *
 * Both engines are independent TeX implementations, so MathJax SVG serves
 * exactly the same oracle role a KaTeX SVG *would* serve: VuTeX's SVG
 * output must point in the same *structural* direction as an independent
 * engine's SVG output — positive width/height, fractions carry a rule,
 * radicals carry a path, element counts in the same ballpark.
 * ---------------------------------------------------------------------------
 *
 * This script does NOT store the full SVG markup (large, structurally
 * unstable between MathJax minor versions).  Instead it extracts a compact
 * geometric summary per formula so the Rust test can verify structural
 * agreement.  Geometric precision deliberately belongs to the LaTeX
 * box-metrics gate (`emit-latex-boxes.mjs`), which measures the *same*
 * TeX engine on both sides.
 *
 * Usage, from this directory:
 *
 *   node emit-mathjax-svg.mjs
 *
 * The mathjax version is pinned in `projects/vutex.ts/examples/oracle/package.json` and the
 * fixture records the version that actually produced it.
 */

import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..", "..", "..");

const CORPUS_PATH = resolve(repoRoot, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const OUT_PATH = resolve(repoRoot, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/mathjax-svg.json");

const require = createRequire(import.meta.url);
const MathJax = require("mathjax");
const mathjaxVersion = require("mathjax/package.json").version;

const COMMAND = "node projects/vutex.ts/examples/oracle/emit-mathjax-svg.mjs";

// ---------------------------------------------------------------------------
// Initialize MathJax with TeX input → SVG output, using the same minimal
// loader config as emit-mathjax-mathml.mjs (plus `output/svg`).
// ---------------------------------------------------------------------------
const MJ = await MathJax.init({
  loader: { load: ["input/tex", "adaptors/liteDOM", "startup", "output/svg"] },
  startup: { typeset: false },
  tex: { packages: ["base", "ams", "cases"] },
});

// ---------------------------------------------------------------------------
// liteDOM → SVG string serializer
//
// MathJax's node-main returns liteDOM nodes ({ kind, attributes, children })
// with no built-in serializer.  We only serialize the SVG subtree (children
// of the mjx-container), and only elements that affect geometry — groups,
// paths, rects, and text nodes.  Unknown elements pass through as-is.
// ---------------------------------------------------------------------------

/** Serialize one liteDOM node (and its descendants) to an SVG string. */
function serialize(node) {
  if (!node || typeof node !== "object") return "";
  if (node.kind === "text") {
    // Text content inside <text> elements.
    return escapeXmlText(node.value || "");
  }
  if (node.kind === "comment" || node.kind === "instruction") {
    return "";
  }
  const tag = node.kind;
  const attrs = serializeAttrs(node.attributes);
  const children = (node.children || []).map(serialize).join("");
  const selfClosing = ["path", "rect", "circle", "line", "ellipse"].includes(tag);
  if (selfClosing && children === "") {
    return `<${tag}${attrs} />`;
  }
  return `<${tag}${attrs}>${children}</${tag}>`;
}

/** Serialize an attributes map.  MathJax liteDOM stores these as plain objects. */
function serializeAttrs(attrs) {
  if (!attrs) return "";
  const parts = [];
  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined) continue;
    parts.push(`${escapeAttrName(key)}="${escapeAttrValue(String(value))}"`);
  }
  return parts.length ? " " + parts.join(" ") : "";
}

function escapeAttrName(name) {
  return name; // SVG attribute names are ASCII in practice
}

function escapeAttrValue(value) {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
}

function escapeXmlText(text) {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

// ---------------------------------------------------------------------------
// SVG summary extraction — lightweight string-based, same helpers as the
// (hypothetical) KaTeX SVG harness would use.
// ---------------------------------------------------------------------------

/** Parse `viewBox="x y w h"` into four floats, or return null on failure. */
function parseViewBox(svg) {
  const match = svg.match(/viewBox\s*=\s*"([^"]+)"/);
  if (!match) return null;
  const parts = match[1].trim().split(/\s+/).map(Number);
  if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n))) return null;
  const [minX, minY, w, h] = parts;
  return { min_x: minX, min_y: minY, width: w, height: h };
}

/** Count how many `<path`, `<rect`, `<text` start-tags exist. */
function countTag(svg, tagName) {
  const re = new RegExp(`<${tagName}(?:\\s|/)`, "g");
  const matches = svg.match(re);
  return matches ? matches.length : 0;
}

/**
 * Extract up to `limit` glyph descriptors from `<path>` elements.
 * Each glyph gets a rough center (from transform translate) and the
 * path data length as a proxy for stroke complexity.
 */
function extractGlyphs(svg, limit = 10) {
  const glyphs = [];
  const pathRe = /<path\b([^>]*)>/g;
  let match;
  while ((match = pathRe.exec(svg)) && glyphs.length < limit) {
    const attrs = match[1];
    const transform = (attrs.match(/transform\s*=\s*"([^"]*)"/) || [])[1] || "";
    const d = (attrs.match(/\bd\s*=\s*"([^"]*)"/) || [])[1] || "";

    // MathJax uses `transform="translate(x y)"` (space-separated, no comma).
    let cx = null;
    let cy = null;
    const tMatch = transform.match(/translate\(\s*([-\d.eE+]+)\s+([-\d.eE+]+)\s*\)/);
    if (tMatch) {
      cx = Number(tMatch[1]);
      cy = Number(tMatch[2]);
    }

    glyphs.push({ cx, cy, path_length: d.length });
  }
  return glyphs;
}

/** Extract every `<rect>` as { x1, y1, x2, y2 } where (x2,y2) is the opposite corner. */
function extractRects(svg) {
  const rects = [];
  const rectRe = /<rect\b([^>]*)>/g;
  let match;
  while ((match = rectRe.exec(svg))) {
    const attrs = match[1];
    const x = Number((attrs.match(/\bx\s*=\s*"([^"]*)"/) || [])[1] || "0");
    const y = Number((attrs.match(/\by\s*=\s*"([^"]*)"/) || [])[1] || "0");
    const w = Number((attrs.match(/\bwidth\s*=\s*"([^"]*)"/) || [])[1] || "0");
    const h = Number((attrs.match(/\bheight\s*=\s*"([^"]*)"/) || [])[1] || "0");
    if ([x, y, w, h].every(Number.isFinite)) {
      rects.push({ x1: x, y1: y, x2: x + w, y2: y + h });
    }
  }
  return rects;
}

/**
 * Summarize one MathJax SVG string into a compact geometric record.
 * Returns null when the SVG has no viewBox (should not happen with real MathJax).
 */
function summarizeSvg(svg) {
  const vb = parseViewBox(svg);
  if (!vb) return null;

  // MathJax's SVG y grows upward (TeX convention), so the viewBox's min_y is
  // typically negative — that IS the formula's depth below the baseline.
  const depth = vb.min_y < 0 ? -vb.min_y : 0;

  return {
    box: { width: vb.width, height: vb.height, depth },
    glyphs: extractGlyphs(svg, 10),
    rules: extractRects(svg),
    paths_count: countTag(svg, "path"),
    rects_count: countTag(svg, "rect"),
    texts_count: countTag(svg, "text"),
  };
}

// ---------------------------------------------------------------------------
// Corpus iteration & fixture document
// ---------------------------------------------------------------------------

/**
 * Render one formula to a MathJax SVG summary, or capture why MathJax refused it.
 * MathJax's tex2svg returns a liteDOM tree under an `<mjx-container>` wrapper;
 * we serialize just the inner `<svg>`.
 */
function renderSvgSummary(tex, displayMode) {
  try {
    const container = MJ.tex2svg(tex, { display: displayMode });
    const svgNode = container?.children?.find((c) => c.kind === "svg");
    if (!svgNode) {
      return { status: "unsupported", reason: "MathJax returned container without <svg>", summary: null };
    }
    const svg = serialize(svgNode);
    const summary = summarizeSvg(svg);
    if (!summary) {
      return { status: "unsupported", reason: "MathJax SVG has no viewBox", summary: null };
    }
    return { status: "available", reason: null, summary };
  } catch (error) {
    const message = error && (error.message || String(error)) || String(error);
    return { status: "unsupported", reason: message, summary: null };
  }
}

/** Build one fixture record. Fields MathJax SVG cannot measure stay null. */
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
    // Geometric summary — the whole point of this oracle.
    box: rendered.summary ? rendered.summary.box : null,
    glyphs: rendered.summary ? rendered.summary.glyphs : null,
    rules: rendered.summary ? rendered.summary.rules : null,
    paths_count: rendered.summary ? rendered.summary.paths_count : null,
    rects_count: rendered.summary ? rendered.summary.rects_count : null,
    texts_count: rendered.summary ? rendered.summary.texts_count : null,
    // Fields MathJax SVG cannot produce — must stay null.
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
  const records = [];
  for (const formula of corpus.formulas) {
    for (const displayMode of [false, true]) {
      records.push(record(formula, displayMode, renderSvgSummary(formula.tex, displayMode)));
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
    corpus_path: "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json",
    records,
  };

  mkdirSync(dirname(OUT_PATH), { recursive: true });
  writeFileSync(OUT_PATH, `${JSON.stringify(document, null, 2)}\n`, "utf8");

  const unsupported = records.filter((r) => r.source.status !== "available");
  console.log(`wrote ${OUT_PATH}`);
  console.log(`${records.length} records from mathjax ${mathjaxVersion}, ${unsupported.length} unsupported`);

  for (const rec of records) {
    if (rec.source.status === "available") {
      const b = rec.box;
      console.log(
        `  ${rec.corpus_id} display=${rec.display_mode}: ` +
        `box=${b.width.toFixed(1)}×${b.height.toFixed(1)} d=${b.depth.toFixed(1)} ` +
        `paths=${rec.paths_count} rects=${rec.rects_count} texts=${rec.texts_count}`
      );
    } else {
      console.log(`  ${rec.corpus_id} display=${rec.display_mode}: UNSUPPORTED — ${rec.source.status_reason}`);
    }
  }
}

main();
