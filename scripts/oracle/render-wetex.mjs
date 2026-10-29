// Render oracle corpus formulas to SVG via @vutex/wetex WASM (pre-built).
//   node scripts/oracle/render-wetex.mjs [--display] [--outdir PATH]

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..");

let displayMode = false;
let outdir = resolve(repo, "out-wetex");

const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--display") displayMode = true;
  else if (a === "--inline") displayMode = false;
  else if (a === "--outdir") outdir = resolve(args[++i]);
}

// Load pre-built WASM binding directly (no TS, no tsx needed).
const wasmPkg = resolve(repo, "projects", "packages", "vutex-unknown-wasm32", "lib");
const wasmBytes = readFileSync(resolve(wasmPkg, "wetex_wasm_bg.wasm"));
const wetexBinding = await import(`file://${resolve(wasmPkg, "wetex_wasm.js")}`);
await wetexBinding.initSync(wasmBytes); // sync init with bytes

const renderSvg = (source, display, fontSize) => {
  const options = JSON.stringify({
    displayMode: display ? "block" : "inline",
    fontSize: fontSize ?? null,
  });
  return JSON.parse(wetexBinding.renderSvg(source, options));
};

const corpus = JSON.parse(
  readFileSync(resolve(repo, "projects/crates/vutex-layout/tests/fixtures/oracle/corpus.json"), "utf8")
);

mkdirSync(outdir, { recursive: true });

// KaTeX default body is 16px × 1.21em ≈ 19.36px for inline formulas.
// Match that so side-by-side visual comparison is size-fair.
const KA_TEX_DEFAULT_PX = 19.36;

let ok = 0, fail = 0, skip = 0;
for (const formula of corpus.formulas) {
  if (formula.tex.includes("\\begin{")) { skip++; continue; }
  try {
    const res = renderSvg(formula.tex, displayMode, KA_TEX_DEFAULT_PX);
    if (!res.svg) {
      console.log(`FAIL  ${formula.id}  empty svg`);
      fail++;
      continue;
    }
    const fname = `${formula.id}-${displayMode ? "display" : "inline"}.svg`;
    writeFileSync(resolve(outdir, fname), res.svg);
    console.log(`OK    ${fname}  ${res.svg.length} bytes`);
    ok++;
  } catch (e) {
    console.log(`FAIL  ${formula.id}  ${e?.message ?? e}`);
    fail++;
  }
}

console.log(`\nWetex render: ${ok} OK, ${fail} failed, ${skip} skipped (env bodies), mode=${displayMode ? "display" : "inline"}`);
