// Fail when a test or script drives a browser that Playwright did not bundle.
//
// Every visual artefact in this repo is rasterized by the Chromium that
// `@playwright/test` installs (`npx playwright install chromium`), so a captured
// pixel is reproducible on any machine and never depends on what the host
// happens to have installed. The usual ways to break that are a `channel`
// selector, an `executablePath` override, a hard-coded Chrome path or a shell
// invocation of `chrome`; this scan turns each of them into a red gate.
//
//   node scripts/ci/check-browser-launch.mjs
//
// Exit code 0 means every harness launches `chromium.launch()` bare. Exit code
// 1 lists the offending files and lines.

import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(import.meta.url), "..", "..", "..");

// Directory names never scanned: build output, dependencies and caches.
const SKIPPED_DIRS = new Set(["node_modules", "target", ".git", "dist", "lib", ".cache", ".idea"]);

// Source kinds that can launch a browser or configure a rasterizer.
const SCANNED = /\.(mjs|cjs|js|ts|tsx|rs|yml|yaml|ps1|sh|py)$/;

const RULES = [
    {
        name: "a browser channel instead of the bundled build",
        pattern: /\bchannel\s*:\s*['"`]\s*(chrome|msedge|chromium|chrome-beta|chrome-dev|msedge-beta)\s*['"`]/i,
        fix: "drop `channel` so `chromium.launch()` uses the Playwright bundle",
    },
    {
        name: "an explicit browser executable",
        pattern: /\bexecutablePath\s*:/,
        fix: "drop `executablePath` so `chromium.launch()` uses the Playwright bundle",
    },
    {
        name: "a host browser binary",
        pattern: /chrome\.exe|chrome-headless-shell\.exe|Chromium\.app/i,
        fix: "never name a host binary: run `npx playwright install chromium` and use the Playwright bundle",
    },
    {
        name: "a host browser install path",
        pattern: /Google[\\/]Chrome|google-chrome|Google Chrome\.app/i,
        fix: "never name a host install path: run `npx playwright install chromium` and use the Playwright bundle",
    },
    {
        name: "a shell launch of the host browser",
        pattern: /\bstart chrome|Start-Process\s+[^\n]*chrome|open\s+-a\s+[^\n]*Chrom/i,
        fix: "launch through `chromium.launch()` instead of the host shell",
    },
    {
        name: "an environment override for the rasterizer",
        pattern: /VUTEX_CHROME|PLAYWRIGHT_CHROMIUM_EXECUTABLE|CHROME_PATH/,
        fix: "the rasterizer version comes from `@playwright/test`, not from the environment",
    },
];

/** Every scannable file under `dir`, as paths relative to the repo root. */
function* walk(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
            if (SKIPPED_DIRS.has(entry.name)) continue;
            yield* walk(full);
            continue;
        }
        if (SCANNED.test(entry.name)) yield full;
    }
}

// This file names the patterns it bans, so it is skipped by name.
const GATE_FILE = "scripts/ci/check-browser-launch.mjs";

const hits = [];
const scratch = [];
for (const file of walk(repo)) {
    const relativePath = relative(repo, file).replace(/\\/g, "/");
    if (relativePath === GATE_FILE) continue;

    // Untracked `.tmp-*` scratch trees are deliberately excluded from this
    // scan; they are reported instead of silently ignored.
    const isScratch = relativePath.split("/").some((segment) => segment.startsWith(".tmp-"));
    const lines = readFileSync(file, "utf8").split("\n");
    for (const [index, line] of lines.entries()) {
        for (const rule of RULES) {
            if (!rule.pattern.test(line)) continue;
            const entry = { file: relativePath, line: index + 1, rule: rule.name, fix: rule.fix, text: line.trim() };
            (isScratch ? scratch : hits).push(entry);
        }
    }
}

if (scratch.length > 0) {
    console.warn(`warning: ${scratch.length} match(es) sit in untracked \`.tmp-*\` scratch trees and are not gated:`);
    for (const entry of scratch) console.warn(`  ${entry.file}:${entry.line}  ${entry.rule}`);
}

if (hits.length > 0) {
    console.error("browser harness gate failed: these harnesses must launch Playwright's bundled Chromium, not a host browser.\n");
    for (const hit of hits) {
        console.error(`  ${hit.file}:${hit.line}  ${hit.rule}`);
        console.error(`    ${hit.text}`);
        console.error(`    fix: ${hit.fix}\n`);
    }
    console.error(`${hits.length} violation(s). Install the bundle with \`npx playwright install chromium\` and launch it with \`chromium.launch()\`.`);
    process.exit(1);
}

console.log("browser harness gate passed: every harness launches Playwright's bundled Chromium.");
