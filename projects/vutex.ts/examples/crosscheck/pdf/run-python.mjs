import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { resultsRoot } from "../lib/ours.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const pythonDir = resolve(here, "python");
const venv = resolve(pythonDir, ".venv");
const venvPython = process.platform === "win32"
    ? resolve(venv, "Scripts", "python.exe")
    : resolve(venv, "bin", "python");
const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const pdfDir = resolve(resultsRoot, ".build", "pdf");

mkdirSync(pdfDir, { recursive: true });

if (!existsSync(venvPython)) {
    console.log("creating python venv in pdf/python/.venv");
    execFileSync(process.env.PYTHON ?? "python", ["-m", "venv", venv], { cwd: pythonDir, stdio: "inherit" });
}

console.log("installing python requirements");
execFileSync(venvPython, ["-m", "pip", "install", "--disable-pip-version-check", "-r", "requirements.txt"], {
    cwd: pythonDir,
    stdio: "inherit",
});

execFileSync(venvPython, [
    "compare.py",
    "--results", resultsRoot,
    "--corpus", resolve(here, "corpus"),
    "--pdf", pdfDir,
    "--stamp", stamp,
], { cwd: pythonDir, stdio: "inherit" });
