// Regenerate the N7-7 visual review set with both colour schemes.
//   node n7-review.mjs --out <dir> [--before-wasm <path>]
//
// The current artifact comes from `@vutex/vutex-unknown-wasm32`. The optional
// `--before-wasm` artifact is used only for the four N7-7 regression frames so
// every before/after pair is produced by the same rasterizer build and the
// same wrapper geometry as the gate goldens.
//
// Rasterizer: the Chromium build Playwright installs, launched with
// `chromium.launch()` and no `channel` / `executablePath`, so no host browser
// and no host font enters the pixels.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "@playwright/test";
import { loadWaTeX } from "@vutex/watex";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..", "..", "..");
const corpusPath = resolve(repo, "projects/vutex.ts/examples/oracle/records/corpus.json");
const PADDING = 8;
const TARGETS = new Map([
  ["integral-limits", true],
  ["cases", false],
  ["radical-fraction", true],
  ["radical-nested", false],
]);
// Playwright selects the media query through the context, so the recorded
// scheme name is the CSS one rather than a Blink numeric flag.
const SCHEMES = ["light", "dark"];

const require = createRequire(import.meta.url);
const playwrightVersion = JSON.parse(readFileSync(require.resolve("@playwright/test/package.json"), "utf8")).version;
const chromiumRevision = /chromium-(\d+)/.exec(chromium.executablePath())?.[1] ?? null;


function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

const outDir = arg("--out");
if (!outDir) {
  console.error("usage: node n7-review.mjs --out <dir> [--before-wasm <path>]");
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });
const frameDir = join(outDir, "frames");
mkdirSync(frameDir, { recursive: true });

const beforePath = arg("--before-wasm");
const beforeOnly = process.argv.includes("--before-only");
if (beforePath && !existsSync(beforePath)) {
  console.error(`before wasm not found: ${beforePath}`);
  process.exit(2);
}

const corpus = JSON.parse(readFileSync(corpusPath, "utf8"));

// Case-insensitive file systems (Windows) would collide ids like `vmatrix`
// and `Vmatrix`, so file names escape every uppercase letter as `!x` — the
// same slug rule the gate capture uses.
function fileSlug(id) {
  return id.replace(/[A-Z]/g, (c) => `!${c.toLowerCase()}`);
}

function viewport(svg) {
  const tag = svg.slice(0, svg.indexOf(">"));
  const width = Number(tag.match(/width="([0-9.]+)/)?.[1]);
  const height = Number(tag.match(/height="([0-9.]+)/)?.[1]);
  if (!Number.isFinite(width) || !Number.isFinite(height)) throw new Error("SVG has no pixel viewport");
  return { width, height };
}

function wrapper(svg) {
  return [
    "<!doctype html><html><head><meta charset=\"utf-8\"><style>",
    "html,body{margin:0;padding:0;background:#ffffff;color:#000000}",
    `#frame{padding:${PADDING}px;display:inline-block;line-height:0}`,
    "@media (prefers-color-scheme: dark){html,body{background:#161616;color:#f2f2f2}}",
    `</style></head><body><div id="frame">${svg}</div></body></html>`,
  ].join("");
}

const browser = await chromium.launch({ args: ["--hide-scrollbars", "--force-device-scale-factor=1"] });
const chromiumVersion = browser.version();
const context = await browser.newContext({ viewport: { width: 64, height: 64 }, deviceScaleFactor: 1, colorScheme: "light" });
const page = await context.newPage();

async function screenshot(svg, pngPath, scheme, width, height) {
  const w = Math.max(64, Math.ceil(width) + 2 * PADDING);
  const h = Math.max(64, Math.ceil(height) + 2 * PADDING);
  await page.setViewportSize({ width: w, height: h });
  await page.emulateMedia({ colorScheme: scheme });
  await page.setContent(wrapper(svg));
  await page.screenshot({ path: pngPath });
  return { window: [w, h], sha256: createHash("sha256").update(readFileSync(pngPath)).digest("hex") };
}

async function renderOne(watex, formula, display, tag) {
  const mode = display ? "display" : "inline";
  const id = `${formula.id}-${mode}`;
  const slug = fileSlug(id);
  const res = await watex.renderSvg(formula.tex, { displayMode: display ? "block" : "inline", fontSize: 16 });
  const errors = res.diagnostics.filter((d) => d.severity === "error");
  if (errors.length > 0) throw new Error(`${id}: ${JSON.stringify(errors)}`);
  const vp = viewport(res.svg);
  const measured = await watex.measureFormula(formula.tex, { displayMode: display ? "block" : "inline", fontSize: 16 });
  const metrics = measured.metrics ?? measured;
  const pngs = {};
  for (const scheme of SCHEMES) {
    const png = join(frameDir, `${tag}-${slug}-${scheme}.png`);
    pngs[scheme] = await screenshot(res.svg, png, scheme, vp.width, vp.height);
  }
  return { id, slug, tex: formula.tex, display, viewport: vp, metrics, pngs };
}

// wasm-bindgen caches the first module it initialises, so the pre-fix artifact
// must be rendered in its own process. The parent only loads the current
// artifact and merges the child's `before.json`.
if (beforeOnly) {
  const old = await loadWaTeX({ bytes: new Uint8Array(readFileSync(beforePath)) });
  const beforeEntries = {};
  for (const formula of corpus.formulas) {
    const display = TARGETS.get(formula.id);
    if (display === undefined) continue;
    const rendered = await renderOne(old, formula, display, "before");
    beforeEntries[rendered.id] = rendered;
  }
  writeFileSync(join(outDir, "before.json"), JSON.stringify(beforeEntries, null, 2) + "\n");
  console.log(`wrote ${Object.keys(beforeEntries).length} pre-fix renders -> ${frameDir}`);
  await browser.close();
  process.exit(0);
}

const wasmEntry = fileURLToPath(import.meta.resolve("@vutex/vutex-unknown-wasm32"));
const current = await loadWaTeX({
  bytes: new Uint8Array(readFileSync(resolve(dirname(wasmEntry), "watex_wasm_bg.wasm"))),
});

const entries = {};
for (const formula of corpus.formulas) {
  for (const display of [false, true]) {
    const rendered = await renderOne(current, formula, display, "after");
    entries[rendered.id] = rendered;
  }
}

const targets = {};
if (beforePath) {
  execFileSync(process.execPath, [
    fileURLToPath(import.meta.url),
    "--out", outDir,
    "--before-only",
    "--before-wasm", beforePath,
  ], { stdio: ["ignore", "ignore", "inherit"] });
  const beforeEntries = JSON.parse(readFileSync(join(outDir, "before.json"), "utf8"));
  for (const [id, b] of Object.entries(beforeEntries)) {
    targets[id] = { id, tex: b.tex, display: b.display, before: b, after: entries[id] };
  }
}

const manifest = {
  schema_version: "v0",
  generated_at: new Date().toISOString(),
  rasterizer: {
    engine: "playwright-bundled-chromium",
    playwrightVersion,
    chromiumVersion,
    chromiumRevision,
    flags: ["--hide-scrollbars", "--force-device-scale-factor=1"],
    schemes: SCHEMES,
    deviceScaleFactor: 1,
    paddingPx: PADDING,
  },
  before_wasm: beforePath ? resolve(beforePath) : null,
  formulas: corpus.formulas.length,
  renders: Object.keys(entries).length,
  targets: Object.keys(targets),
  entries,
  comparisons: targets,
};
writeFileSync(join(outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");

const comparisonRows = Object.values(targets).map((pair) => {
  const mode = pair.display ? "display" : "inline";
  const id = `${pair.id}-${mode}`;
  const slug = pair.after.slug;
  return `<section class="pair"><h3>${id}</h3><code>${escapeHtml(pair.tex)}</code>
  <div class="grid">
    <div><span>before · light</span><img src="frames/before-${slug}-light.png"></div>
    <div><span>after · light</span><img src="frames/after-${slug}-light.png"></div>
    <div><span>before · dark</span><img src="frames/before-${slug}-dark.png"></div>
    <div><span>after · dark</span><img src="frames/after-${slug}-dark.png"></div>
  </div>
  <p>metrics before ${JSON.stringify(pair.before.metrics)} / after ${JSON.stringify(pair.after.metrics)}</p></section>`;
}).join("\n");

const reviewRows = Object.values(entries).map((entry) => `<div class="cell"><span>${entry.id}</span>
  <div><img src="frames/after-${entry.slug}-light.png" title="light"><img src="frames/after-${entry.slug}-dark.png" title="dark"></div></div>`).join("\n");

function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

writeFileSync(join(outDir, "index.html"), `<!doctype html><html><head><meta charset="utf-8">
<title>VuTeX N7-7 visual review</title>
<style>
body{font:14px/1.5 system-ui,sans-serif;margin:24px;background:#fafafa;color:#222}
h1,h2{font-weight:600}
.pair{margin:16px 0;padding:12px;background:#fff;border:1px solid #ddd}
.grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:8px}
.grid>div{background:#eee;padding:4px}
.grid img,.cell img{max-width:100%;image-rendering:pixelated;background:#fff}
.cell{display:inline-block;width:300px;margin:4px;vertical-align:top;background:#fff;border:1px solid #ddd;padding:4px;font:12px monospace}
.cell>div{display:flex;gap:4px}
.cell img{width:calc(50% - 2px)}
</style></head><body>
<h1>VuTeX N7-7 visual review</h1>
<p>Rasterizer: Playwright ${playwrightVersion} bundled Chromium ${chromiumVersion} (revision ${chromiumRevision}). Preferred colour scheme is set per context, wrapper padding ${PADDING}px, device scale factor 1.</p>
<h2>Four regression frames: before / after</h2>
${comparisonRows}
<h2>Full corpus: inline / display, light / dark</h2>
<p>${Object.keys(entries).length} renders. Left image light, right image dark.</p>
${reviewRows}
</body></html>
`);

console.log(`wrote ${Object.keys(entries).length} renders (${Object.keys(targets).length} before/after pairs) -> ${outDir}`);
