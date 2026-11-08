#!/usr/bin/env node
// Font asset release gate.
//
// The two font packages are assembled from one in-repo source and npm owns
// versioning and integrity, so this gate only asserts existence and ownership:
// every production face and its licence live in the single source, no shipped
// package duplicates a font binary, nothing is embedded into Rust, and the two
// font tarballs carry the manifest, faces and licences they promise.

import { execFileSync, execSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const sourceDir = join(root, "projects/vutex.rs/fonts");
const manifestPath = join(sourceDir, "fonts.json");

/** Production faces, licences, and the split the two packages promise. */
const EXPECTED_FACES = 8;
const EXPECTED_LICENCES = 5;
const EXPECTED_TEXT_FACES = 6;
const EXPECTED_MATH_FACES = 2;

const MANIFEST_KEYS = ["name", "role", "package", "license"];
const FONT_EXTENSION = /\.(otf|ttf|woff2?)$/i;

const failures = [];

function check(name, run) {
  let problems;
  try {
    problems = run();
  } catch (error) {
    problems = [error instanceof Message ? error.message : String(error)];
  }
  if (problems.length === 0) {
    console.log(`PASS ${name}`);
  } else {
    console.log(`FAIL ${name}`);
    for (const problem of problems) console.log(`  ${problem}`);
    failures.push(name);
  }
}

class Message extends Error {}

function readManifest() {
  if (!existsSync(manifestPath)) {
    throw new Message(`missing ${relative(root, manifestPath)}`);
  }
  const raw = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (raw.schema !== "vutex.font-assets/v1") {
    throw new Message(`unexpected schema in fonts.json: ${raw.schema}`);
  }
  if (!Array.isArray(raw.files)) {
    throw new Message("fonts.json has no files array");
  }
  return raw;
}

function tracked(pattern) {
  const output = execFileSync("git", ["ls-files", ...pattern], { cwd: root, encoding: "utf8" });
  return output
    .split("\n")
    .map((line) => line.trim().replaceAll("\\", "/"))
    .filter(Boolean);
}

function packFiles(dir) {
  const output = execSync("npm pack --dry-run --json", { cwd: join(root, dir), encoding: "utf8" });
  const start = output.indexOf("[");
  if (start < 0) throw new Message(`npm pack returned no JSON for ${dir}`);
  const parsed = JSON.parse(output.slice(start));
  const entry = Array.isArray(parsed) ? parsed[0] : parsed;
  if (!entry || !Array.isArray(entry.files)) throw new Message(`npm pack returned no file list for ${dir}`);
  return entry.files.map((file) => file.path.replaceAll("\\", "/"));
}

check("font.source.faces_and_licences", () => {
  const manifest = readManifest();
  const problems = [];
  const licences = new Set();
  let text = 0;
  let math = 0;
  for (const entry of manifest.files) {
    for (const key of MANIFEST_KEYS) {
      if (typeof entry[key] !== "string" || entry[key].length === 0) {
        problems.push(`font entry is missing \`${key}\`: ${JSON.stringify(entry)}`);
      }
    }
    if (!existsSync(join(sourceDir, entry.name))) problems.push(`missing face ${entry.name}`);
    if (!existsSync(join(sourceDir, entry.license))) problems.push(`missing licence ${entry.license} for ${entry.name}`);
    licences.add(entry.license);
    if (entry.package === "text") text += 1;
    if (entry.package === "math") math += 1;
  }
  if (manifest.files.length !== EXPECTED_FACES) problems.push(`expected ${EXPECTED_FACES} faces, found ${manifest.files.length}`);
  if (licences.size !== EXPECTED_LICENCES) problems.push(`expected ${EXPECTED_LICENCES} licence files, found ${licences.size}`);
  if (text !== EXPECTED_TEXT_FACES) problems.push(`expected ${EXPECTED_TEXT_FACES} text faces, found ${text}`);
  if (math !== EXPECTED_MATH_FACES) problems.push(`expected ${EXPECTED_MATH_FACES} math faces, found ${math}`);
  return problems;
});

check("font.source.single_copy", () => {
  const shipped = [
    "projects/vutex.rs/exports/",
    "projects/vutex.ts/packages/",
    "projects/vutex.ts/platforms/",
  ];
  return tracked(["*.otf", "*.ttf", "*.woff", "*.woff2"])
    .filter((path) => !path.startsWith("projects/vutex.rs/fonts/"))
    .filter((path) => !path.includes("/test-fixtures/"))
    .filter((path) => shipped.some((prefix) => path.startsWith(prefix)))
    .map((path) => `${path} duplicates a font binary outside the single source`);
});

check("font.rust.no_embedded_bytes", () => {
  let output = "";
  try {
    output = execFileSync("git", ["grep", "-n", "-E", "include_bytes!\\(.*fonts/", "--", "projects/vutex.rs/exports"], {
      cwd: root,
      encoding: "utf8",
    });
  } catch (error) {
    if (error.status === 1) return [];
    throw error;
  }
  return output.split("\n").filter(Boolean);
});

check("font.packages.tarball_contents", () => {
  const manifest = readManifest();
  const targets = [
    { dir: "projects/vutex.ts/packages/vutex-fonts", packages: ["text", "math"] },
    { dir: "projects/vutex.ts/packages/watex-fonts", packages: ["math"] },
  ];
  const problems = [];
  for (const target of targets) {
    const files = packFiles(target.dir);
    const present = new Set(files);
    const expected = manifest.files.filter((entry) => target.packages.includes(entry.package));
    if (!present.has("assets/fonts/fonts.json")) problems.push(`${target.dir} tarball is missing assets/fonts/fonts.json`);
    for (const entry of expected) {
      if (!present.has(`assets/fonts/${entry.name}`)) problems.push(`${target.dir} tarball is missing ${entry.name}`);
      if (!present.has(`assets/fonts/${entry.license}`)) problems.push(`${target.dir} tarball is missing ${entry.license}`);
    }
  }
  return problems;
});

check("font.runtime.no_font_artifacts", () => {
  const licences = new Set(readManifest().files.map((entry) => entry.license));
  return tracked(["projects/vutex.ts/platforms", "projects/vutex.ts/packages/watex"])
    .filter((path) => FONT_EXTENSION.test(path) || licences.has(path.split("/").at(-1)));
});

if (failures.length > 0) {
  console.error(`\n${failures.length} font asset assertion(s) failed`);
  process.exit(1);
}
console.log("\nall font asset assertions passed");
