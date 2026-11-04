// Capture per-formula visual goldens for gate N7-6.
//   pnpm --filter @vutex/visual-golden capture            # record goldens
//   pnpm --filter @vutex/visual-golden verify             # re-render and compare hashes
//
// Rasterizer: the Chromium build Playwright installs, never a browser already
// present on the host. `chromium.launch()` is called without `channel` and
// without `executablePath`, so the binary is whatever `@playwright/test`
// resolves, and `npx playwright install chromium` provisions it on a clean
// machine. The SVG payload is pure outline paths (no host fonts), so the only
// moving part is Chromium's rasterizer; its Playwright version, Chromium
// revision, device scale factor, colour scheme and white background are pinned
// here and recorded in the manifest. A hash mismatch under the pinned
// rasterizer is a rendering change and fails the gate; a rasterizer version
// change requires a deliberate re-capture.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

import { chromium } from "@playwright/test";
import { loadWaTeX } from "@vutex/watex";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..", "..", "..");
const corpusPath = resolve(repo, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json");
const goldenDir = resolve(repo, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/visual");
const manifestPath = join(goldenDir, "manifest.json");

// Pinned rasterizer identity. Bumping Playwright is a deliberate act: install
// the new Chromium, re-capture every golden and record the new revision.
const RASTERIZER = {
  engine: "playwright-bundled-chromium",
  // No host fonts are involved (the SVG is outline paths only); these settings
  // fix the viewport scale, background and scrollbar behaviour.
  flags: ["--hide-scrollbars", "--force-device-scale-factor=1"],
  deviceScaleFactor: 1,
  colorScheme: "light",
  background: "#ffffff",
  paddingPx: 8,
};

const require = createRequire(import.meta.url);
const playwrightVersion = JSON.parse(readFileSync(require.resolve("@playwright/test/package.json"), "utf8")).version;
const chromiumRevision = /chromium-(\d+)/.exec(chromium.executablePath())?.[1] ?? null;

const verify = process.argv.includes("--verify");

const corpus = JSON.parse(readFileSync(corpusPath, "utf8"));
const wasmEntry = fileURLToPath(import.meta.resolve("@vutex/vutex-unknown-wasm32"));
const wasmBytes = new Uint8Array(readFileSync(resolve(dirname(wasmEntry), "watex_wasm_bg.wasm")));
const watex = await loadWaTeX({ bytes: wasmBytes });

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
const workDir = mkdtempSync(join(tmpdir(), "vutex-visual-golden-"));

const browser = await chromium.launch({ args: RASTERIZER.flags });
const chromiumVersion = browser.version();
console.log(
  `META playwright=${playwrightVersion} chromium=${chromiumVersion} revision=${chromiumRevision} ` +
    `dpr=${RASTERIZER.deviceScaleFactor} color-scheme=${RASTERIZER.colorScheme}`,
);
const context = await browser.newContext({
  viewport: { width: 64, height: 64 },
  deviceScaleFactor: RASTERIZER.deviceScaleFactor,
  colorScheme: RASTERIZER.colorScheme,
});
const page = await context.newPage();

function identity() {
  return {
    engine: RASTERIZER.engine,
    playwrightVersion,
    chromiumVersion,
    chromiumRevision,
    flags: RASTERIZER.flags,
    deviceScaleFactor: RASTERIZER.deviceScaleFactor,
    colorScheme: RASTERIZER.colorScheme,
    background: RASTERIZER.background,
    paddingPx: RASTERIZER.paddingPx,
  };
}

async function screenshotPng(html, pngPath, width, height) {
  await page.setViewportSize({ width, height });
  await page.setContent(html);
  await page.screenshot({ path: pngPath });
}

const previous = verify && existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, "utf8")) : null;
if (verify && !previous) {
  console.error("no manifest.json to verify against; run capture first");
  await browser.close();
  process.exit(2);
}
if (verify && previous.rasterizer.engine !== RASTERIZER.engine) {
  console.error(`manifest pins ${previous.rasterizer.engine}, this harness pins ${RASTERIZER.engine}; refusing to mix pixels`);
  await browser.close();
  process.exit(2);
}
if (verify) {
  const recorded = previous.rasterizer;
  if (recorded.playwrightVersion !== playwrightVersion || recorded.chromiumVersion !== chromiumVersion || recorded.chromiumRevision !== chromiumRevision) {
    console.error(
      `rasterizer mismatch: manifest pins Playwright ${recorded.playwrightVersion} / Chromium ${recorded.chromiumVersion} (revision ${recorded.chromiumRevision}), ` +
        `this harness runs Playwright ${playwrightVersion} / Chromium ${chromiumVersion} (revision ${chromiumRevision}). ` +
        `Re-capture deliberately with the new bundled build instead of mixing pixels.`
    );
    await browser.close();
    process.exit(2);
  }
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
    const pngPath = join(workDir, `${slug}.png`);
    await screenshotPng(wrapperHtml(res.svg), pngPath, windowWidth, windowHeight);
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

await browser.close();

const manifest = {
  schema_version: "v0",
  rasterizer: identity(),
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
    console.error(
      `visual golden verification failed: ${mismatches} mismatch(es) under ${RASTERIZER.engine} ` +
        `(Playwright ${playwrightVersion}, Chromium ${chromiumVersion}, revision ${chromiumRevision})`
    );
    process.exit(1);
  }
  console.log(
    `visual golden verification passed: ${Object.keys(entries).length} renders byte-identical under ${RASTERIZER.engine} ` +
      `(Playwright ${playwrightVersion}, Chromium ${chromiumVersion}, revision ${chromiumRevision})`
  );
} else {
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
  console.log(
    `captured ${Object.keys(entries).length} goldens under ${RASTERIZER.engine} ` +
      `(Playwright ${playwrightVersion}, Chromium ${chromiumVersion}, revision ${chromiumRevision}) -> ${goldenDir}`
  );
}
