import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/** Repository root, five levels above `examples/crosscheck/lib`. */
export const repoRoot = resolve(here, "..", "..", "..", "..", "..");

/** Cross-check results tree, git-ignored. */
export const resultsRoot = resolve(here, "..", "results");

const require = createRequire(import.meta.url);

let native;

/**
 * Load the freshly built Node-API addon.
 *
 * Node refuses to load a `.node` addon unless the file carries that extension,
 * so the cargo artifact is staged into `results/.build` under a `.node` name.
 * Override the source with `VUTEX_NAPI`.
 */
export function loadNative() {
    if (native) {
        return native;
    }
    const candidates = [
        process.env.VUTEX_NAPI,
        resolve(repoRoot, "target/debug/vutex_napi.dll"),
        resolve(repoRoot, "target/release/vutex_napi.dll"),
    ].filter(Boolean);
    const source = candidates.find((path) => existsSync(path));
    if (!source) {
        throw new Error(
            "no vutex_napi artifact found, build it first with `cargo build -p vutex-napi`",
        );
    }
    const staged = resolve(resultsRoot, ".build", "vutex.node");
    mkdirSync(dirname(staged), { recursive: true });
    copyFileSync(source, staged);
    native = require(staged);
    return native;
}

let math;

/**
 * Load the WaTeX WASM binding, which is the product math surface.
 *
 * The Node-API formula exports panic before layout because that entry never
 * installs the embedded math faces, so the math comparison goes through the
 * surface hosts actually ship. Override the artifact with `VUTEX_WATEX_WASM`.
 */
export async function loadMath() {
    if (math) {
        return math;
    }
    const wasmPath = process.env.VUTEX_WATEX_WASM
        ?? resolve(repoRoot, "projects/vutex.ts/platforms/vutex-unknown-wasm32/lib/watex_wasm_bg.wasm");
    if (!existsSync(wasmPath)) {
        throw new Error(`no WaTeX wasm artifact at ${wasmPath}, build it with \`pnpm --filter @vutex/watex build\``);
    }
    const { loadWaTeX } = await import("@vutex/watex");
    math = await loadWaTeX({ bytes: readFileSync(wasmPath) });
    return math;
}
