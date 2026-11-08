#!/usr/bin/env node
// Assemble the two font asset packages from the single in-repo font source.
//
// The source manifest `projects/vutex.rs/fonts/fonts.json` only lists file names,
// roles and licence files. It carries no hashes: npm owns versioning and
// integrity. `assets/` inside each package is generated here and is gitignored.

import { access, mkdir, readFile, readdir, unlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..", "..");
const sourceDir = path.join(repoRoot, "projects", "vutex.rs", "fonts");
const sourceManifest = path.join(sourceDir, "fonts.json");

const targets = [
  {
    dir: path.join(repoRoot, "projects", "vutex.ts", "packages", "vutex-fonts"),
    packages: ["text", "math"],
  },
  {
    dir: path.join(repoRoot, "projects", "vutex.ts", "packages", "watex-fonts"),
    packages: ["math"],
  },
];

const knownPackages = new Set(["text", "math"]);
const checkOnly = process.argv.includes("--check");

async function loadSourceManifest() {
  const raw = JSON.parse(await readFile(sourceManifest, "utf8"));
  if (raw.schema !== "vutex.font-assets/v1") {
    throw new Error(`unexpected schema in ${sourceManifest}: ${raw.schema}`);
  }
  if (!Array.isArray(raw.files) || raw.files.length === 0) {
    throw new Error(`empty file list in ${sourceManifest}`);
  }
  const roles = new Set();
  for (const entry of raw.files) {
    for (const key of ["name", "role", "package", "license"]) {
      if (typeof entry[key] !== "string" || entry[key].length === 0) {
        throw new Error(`font entry is missing \`${key}\`: ${JSON.stringify(entry)}`);
      }
    }
    if (!knownPackages.has(entry.package)) {
      throw new Error(`font entry has unknown package \`${entry.package}\`: ${entry.name}`);
    }
    if (roles.has(entry.role)) {
      throw new Error(`duplicate role \`${entry.role}\` in ${sourceManifest}`);
    }
    roles.add(entry.role);
  }
  return raw;
}

function subsetManifest(manifest, packages) {
  return {
    schema: manifest.schema,
    files: manifest.files.filter((entry) => packages.includes(entry.package)),
  };
}

async function assertSourceFiles(files) {
  for (const entry of files) {
    for (const name of [entry.name, entry.license]) {
      try {
        await access(path.join(sourceDir, name));
      } catch {
        throw new Error(`missing source asset \`${name}\` referenced by ${entry.name}`);
      }
    }
  }
}

async function pruneStale(assetsDir, expected) {
  let entries;
  try {
    entries = await readdir(assetsDir, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return;
    throw error;
  }
  for (const entry of entries) {
    if (!entry.isFile()) {
      throw new Error(`unexpected directory inside generated assets: ${entry.name}`);
    }
    if (!expected.has(entry.name)) {
      await unlink(path.join(assetsDir, entry.name));
    }
  }
}

async function verifyTarget(targetDir, files, manifest) {
  const assetsDir = path.join(targetDir, "assets", "fonts");
  const expected = new Set(files.map((entry) => entry.name).concat(files.map((entry) => entry.license)));
  expected.add("fonts.json");
  const missing = [];
  for (const name of expected) {
    const target = path.join(assetsDir, name);
    try {
      if (name === "fonts.json") {
        const got = JSON.parse(await readFile(target, "utf8"));
        if (JSON.stringify(got) !== JSON.stringify(manifest)) missing.push(`${name} (content)`);
        continue;
      }
      const [want, got] = await Promise.all([
        readFile(path.join(sourceDir, name)),
        readFile(target),
      ]);
      if (!want.equals(got)) missing.push(`${name} (content)`);
    } catch {
      missing.push(name);
    }
  }
  if (missing.length > 0) {
    throw new Error(`${path.relative(repoRoot, targetDir)}: staged assets are stale or incomplete: ${missing.join(", ")}`);
  }
}

async function stageTarget(target, manifest) {
  const assetsDir = path.join(target.dir, "assets", "fonts");
  const files = manifest.files;
  const expected = new Set(files.map((entry) => entry.name).concat(files.map((entry) => entry.license)));
  expected.add("fonts.json");

  if (checkOnly) {
    await verifyTarget(target.dir, files, manifest);
    console.log(`ok ${path.relative(repoRoot, target.dir)} (${files.length} faces)`);
    return;
  }

  await mkdir(assetsDir, { recursive: true });
  await pruneStale(assetsDir, expected);
  for (const entry of files) {
    for (const name of new Set([entry.name, entry.license])) {
      await writeFile(path.join(assetsDir, name), await readFile(path.join(sourceDir, name)));
    }
  }
  await writeFile(path.join(assetsDir, "fonts.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`staged ${path.relative(repoRoot, target.dir)} (${files.length} faces)`);
}

const source = await loadSourceManifest();
await assertSourceFiles(source.files);
for (const target of targets) {
  await stageTarget(target, subsetManifest(source, target.packages));
}
