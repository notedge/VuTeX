// Render oracle corpus formulas to SVG via the workspace `@vutex/wetex` API.
//   pnpm --filter @vutex/oracle-tools render:wetex [--display] [--outdir PATH]
// (the script builds `@vutex/wetex` first, so its `dist` entry is current)

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { renderSvg } from "@vutex/wetex";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..", "..", "..");

// Node hosts cannot fetch the web-target artifact, so read its bytes and
// hand them to the standard `@vutex/wetex` loader (WeTeXWasmOptions.bytes).
const wasmEntry = fileURLToPath(import.meta.resolve("@vutex/vutex-unknown-wasm32"));
const wasmBytes = readFileSync(resolve(dirname(wasmEntry), "wetex_wasm_bg.wasm"));
const wasmOptions = { bytes: new Uint8Array(wasmBytes) };

let displayMode = false;
let outdir = resolve(repo, "out-wetex");

const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--display") displayMode = true;
  else if (a === "--inline") displayMode = false;
  else if (a === "--outdir") outdir = resolve(args[++i]);
}

const corpus = JSON.parse(
  readFileSync(resolve(repo, "projects/vutex.rs/crates/vutex-layout/tests/fixtures/oracle/corpus.json"), "utf8")
);

mkdirSync(outdir, { recursive: true });

// KaTeX default body is 16px × 1.21em ≈ 19.36px for inline formulas.
// Match that so side-by-side visual comparison is size-fair.
const KATEX_DEFAULT_PX = 19.36;

let ok = 0, fail = 0, skip = 0;
for (const formula of corpus.formulas) {
  if (formula.tex.includes("\\begin{")) { skip++; continue; }
  try {
    const res = await renderSvg(
      formula.tex,
      {
        displayMode: displayMode ? "block" : "inline",
        fontSize: KATEX_DEFAULT_PX,
      },
      wasmOptions,
    );
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
