// Capture per-formula visual goldens for gate N7-6.
//   pnpm --filter @vutex/visual-golden capture            # record goldens
//   pnpm --filter @vutex/visual-golden verify             # re-render and compare hashes
//
// Rasterizer: headless Chrome at the pinned version recorded in the manifest.
// The SVG payload is pure outline paths (no host fonts), so the only moving
// part is Chrome's rasterizer; its version, flags, DPI (device scale factor 1)
// and white background are pinned here and recorded per golden. A hash
// mismatch under the pinned rasterizer is a rendering change and fails the
// gate; a rasterizer version change requires a deliberate re-capture.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

import { loadWaTeX } from "@vutex/watex";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..", "..", "..");
const corpusPath = resolve(repo, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const goldenDir = resolve(repo, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/visual");
const manifestPath = join(goldenDir, "manifest.json");

// Pinned rasterizer identity. Bumping Chrome is a deliberate act: re-capture
// every golden and record the new version here.
const RASTERIZER = {
  engine: "headless-chrome",
  version: "156.0.8078.80",
  executable: process.env.VUTEX_CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe",
  // No host fonts are involved (the SVG is outline paths only); these flags
  // fix the viewport scale, background and scrollbar behaviour.
  flags: ["--headless", "--disable-gpu", "--no-sandbox", "--hide-scrollbars", "--force-device-scale-factor=1"],
  deviceScaleFactor: 1,
  background: "#ffffff",
  paddingPx: 8,
};

const verify = process.argv.includes("--verify");

const corpus = JSON.parse(readFileSync(corpusPath, "utf8"));
const wasmEntry = fileURLToPath(import.meta.resolve("@vutex/vutex-unknown-wasm32"));
const wasmBytes = new Uint8Array(readFileSync(resolve(dirname(wasmEntry), "watex_wasm_bg.wasm")));
const watex = await loadWaTeX({ bytes: wasmBytes });

function chromeVersion(executable) {
  // `chrome --version` may refuse to print when a session is running; the
  // pinned identity above is authoritative, but a live probe catches a host
  // that silently runs a different binary.
  try {
    const out = execFileSync(executable, ["--version"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
    const match = out.match(/(\d+\.\d+\.\d+\.\d+)/);
    return match ? match[1] : null;
  } catch {
    return null;
  }
}

function screenshotPng(htmlPath, pngPath, width, height) {
  const args = [...RASTERIZER.flags, `--window-size=${width},${height}`, `--screenshot=${pngPath}`, `file:///${htmlPath.replace(/\\/g, "/")}`];
  execFileSync(RASTERIZER.executable, args, { stdio: ["ignore", "ignore", "pipe"] });
}

// Case-insensitive file systems (Windows) would collide ids like `vmatrix`
// and `Vmatrix`, so file names escape every uppercase letter as `!x`.
function fileSlug(id) {
  return id.replace(/[A-Z]/g, (c) => `!${c.toLowerCase()}`);
}

function svgViewport(svg) {
  const tag = svg.slice(0, svg.indexOf(">"));
  const attr = (name) => {
    const m = tag.match(new RegExp(`${name}="([0-9.]+)`));
    return m ? Number(m[1]) : null;
  };
  const width = attr("width");
  const height = attr("height");
  if (!width || !height) throw new Error("formula SVG must carry pixel width/height attributes");
  return { width, height };
}

function wrapperHtml(svg) {
  return [
    "<!doctype html>",
    `<html><head><meta charset="utf-8"><style>`,
    `html,body{margin:0;padding:0;background:${RASTERIZER.background}}`,
    `#frame{padding:${RASTERIZER.paddingPx}px;display:inline-block;line-height:0}`,
    `</style></head><body><div id="frame">${svg}</div></body></html>`,
  ].join("");
}

mkdirSync(goldenDir, { recursive: true });
const workDir = join(tmpdir(), "vutex-visual-golden");
rmSync(workDir, { recursive: true, force: true });
mkdirSync(workDir, { recursive: true });

const probed = chromeVersion(RASTERIZER.executable);
if (probed && probed !== RASTERIZER.version) {
  console.error(
    `rasterizer mismatch: pinned ${RASTERIZER.version}, host runs ${probed}. ` +
      `Re-capture deliberately with the new version instead of mixing pixels.`
  );
  process.exit(2);
}
if (!existsSync(RASTERIZER.executable)) {
  console.error(`rasterizer not found at ${RASTERIZER.executable} (set VUTEX_CHROME to the pinned build)`);
  process.exit(2);
}

const previous = verify && existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, "utf8")) : null;
if (verify && !previous) {
  console.error("no manifest.json to verify against; run capture first");
  process.exit(2);
}
if (verify && previous.rasterizer.version !== RASTERIZER.version) {
  console.error(`manifest pins ${previous.rasterizer.version}, this harness pins ${RASTERIZER.version}; refusing to mix`);
  process.exit(2);
}

const entries = {};
let mismatches = 0;
for (const formula of corpus.formulas) {
  for (const display of [false, true]) {
    const id = `${formula.id}-${display ? "display" : "inline"}`;
    const slug = fileSlug(id);
    const res = await watex.renderSvg(formula.tex, { displayMode: display ? "block" : "inline", fontSize: 16 });
    const errors = res.diagnostics.filter((d) => d.severity === "error");
    if (errors.length > 0) throw new Error(`${id}: render failed: ${JSON.stringify(errors)}`);
    const { width, height } = svgViewport(res.svg);
    const windowWidth = Math.max(64, Math.ceil(width) + 2 * RASTERIZER.paddingPx);
    const windowHeight = Math.max(64, Math.ceil(height) + 2 * RASTERIZER.paddingPx);
    const htmlPath = join(workDir, `${slug}.html`);
    const pngPath = join(workDir, `${slug}.png`);
    writeFileSync(htmlPath, wrapperHtml(res.svg));
    screenshotPng(htmlPath, pngPath, windowWidth, windowHeight);
    const png = readFileSync(pngPath);
    const sha256 = createHash("sha256").update(png).digest("hex");
    entries[id] = {
      tex: formula.tex,
      display,
      file: `${slug}.png`,
      window: [windowWidth, windowHeight],
      svgViewport: [width, height],
      bytes: png.length,
      sha256,
    };
    if (verify) {
      const recorded = previous.entries[id];
      if (!recorded) {
        console.error(`MISSING  ${id}: no recorded golden`);
        mismatches++;
      } else if (recorded.sha256 !== sha256) {
        console.error(`CHANGED  ${id}: recorded ${recorded.sha256.slice(0, 12)}…, now ${sha256.slice(0, 12)}…`);
        mismatches++;
      }
    } else {
      writeFileSync(join(goldenDir, `${slug}.png`), png);
    }
  }
}

const manifest = {
  schema_version: "v0",
  rasterizer: { engine: RASTERIZER.engine, version: RASTERIZER.version, flags: RASTERIZER.flags, deviceScaleFactor: RASTERIZER.deviceScaleFactor, background: RASTERIZER.background, paddingPx: RASTERIZER.paddingPx },
  renderer: "@vutex/watex renderSvg (real wasm artifact), fontSize 16px, default embedded family",
  formulas: corpus.formulas.length,
  entries,
};

if (verify) {
  for (const id of Object.keys(previous.entries)) {
    if (!entries[id]) {
      console.error(`DROPPED  ${id}: recorded golden has no current render`);
      mismatches++;
    }
  }
  if (mismatches > 0) {
    console.error(`visual golden verification failed: ${mismatches} mismatch(es) under ${RASTERIZER.engine} ${RASTERIZER.version}`);
    process.exit(1);
  }
  console.log(`visual golden verification passed: ${Object.keys(entries).length} renders byte-identical under ${RASTERIZER.engine} ${RASTERIZER.version}`);
} else {
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
  console.log(`captured ${Object.keys(entries).length} goldens under ${RASTERIZER.engine} ${RASTERIZER.version} -> ${goldenDir}`);
}
