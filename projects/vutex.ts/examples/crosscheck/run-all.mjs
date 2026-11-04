// Run every same-ecosystem cross-check in one go, then the TikZ capability
// audit, which has no external reference and is reported separately.
//
// Usage: node run-all.mjs

import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

const STEPS = [
    { id: "math", script: "math/compare.mjs" },
    { id: "diagram", script: "diagram/compare.mjs" },
    { id: "pdf:ts", script: "pdf/compare-ts.mjs" },
    { id: "pdf:py", script: "pdf/run-python.mjs" },
    { id: "tikz", script: "tikz/audit.mjs" },
    { id: "tex", script: "tex/compare.mjs" },
];

const failures = [];

for (const step of STEPS) {
    console.log(`\n=== ${step.id} ===`);
    try {
        execFileSync(process.execPath, [resolve(here, step.script)], {
            cwd: here,
            stdio: "inherit",
            timeout: 600_000,
        });
    } catch (error) {
        failures.push(step.id);
        console.warn(`${step.id} failed: ${error.message}`);
    }
}

if (failures.length > 0) {
    console.log(`\nfailed steps: ${failures.join(", ")}`);
    process.exitCode = 1;
} else {
    console.log("\nall steps completed");
}
