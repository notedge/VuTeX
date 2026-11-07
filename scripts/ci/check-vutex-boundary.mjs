import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const rustRoot = join(root, "projects/vutex.rs");
const napiSrc = join(rustRoot, "exports/vutex-napi/src");

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    if (entry === "target" || entry === "node_modules" || entry === ".git") continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

const files = walk(rustRoot);
const failures = [];

function check(name, run) {
  const problems = run();
  if (problems.length === 0) {
    console.log(`PASS ${name}`);
  } else {
    console.log(`FAIL ${name}`);
    for (const problem of problems) console.log(`  ${problem}`);
    failures.push(name);
  }
}

const cargoFiles = files.filter((file) => file.endsWith("Cargo.toml"));
const sources = files.filter((file) => file.endsWith(".rs"));

check("vutex.boundary.no_rust_binaries", () =>
  cargoFiles
    .filter((file) => /^\[\[bin\]\]/m.test(readFileSync(file, "utf8")))
    .map((file) => `${relative(root, file)} declares [[bin]]`),
);

check("vutex.boundary.no_main_entry", () =>
  sources.filter((file) => file.endsWith("main.rs")).map((file) => `${relative(root, file)} is a binary entry`),
);

check("vutex.boundary.no_aggregate_exports", () => {
  const pattern = /\b(render_document|inspect_document|renderDocument|inspectDocument)\s*\(/;
  return sources
    .filter((file) => file.includes("vutex-napi") || file.includes("vutex-layout"))
    .filter((file) => pattern.test(readFileSync(file, "utf8")))
    .map((file) => `${relative(root, file)} exposes a pipeline export`);
});

check("vutex.boundary.no_cli_concepts_in_library", () => {
  const pattern = /\b(exit_code|exitCode|std::process::exit|std::fs::|Command::new)\b/;
  return sources
    .filter((file) => file.startsWith(napiSrc))
    .filter((file) => pattern.test(readFileSync(file, "utf8")))
    .map((file) => `${relative(root, file)} carries a CLI concept`);
});

check("vutex.boundary.font_embedding_is_shared", () => {
  const problems = [];
  for (const file of sources.filter((item) => item.startsWith(napiSrc))) {
    const text = readFileSync(file, "utf8");
    if (file.endsWith("svg_fonts.rs")) continue;
    if (text.includes("font-family")) problems.push(`${relative(root, file)} assembles a font family outside svg_fonts.rs`);
  }
  const document = readFileSync(join(napiSrc, "document.rs"), "utf8");
  if (!document.includes("embed_body_fonts(")) problems.push("document.rs has no shared SVG font injection call");
  if (!document.includes("embed_html_fonts(")) problems.push("document.rs has no shared HTML font injection call");
  return problems;
});

check("vutex.boundary.only_two_library_crates", () => {
  const problems = [];
  // The gate suite is a non-published test crate, never a shipped library:
  // it is allowed to exist beside the two export crates.
  const gateSuite = "tests/quality-gates/Cargo.toml";
  const crates = cargoFiles
    .map((file) => relative(rustRoot, file).replaceAll("\\", "/"))
    .filter((path) => !path.startsWith("exports/") && path !== "Cargo.toml");
  for (const path of crates) {
    if (path !== gateSuite) {
      problems.push(`${path} adds a third crate`);
      continue;
    }
    const manifest = readFileSync(join(rustRoot, path), "utf8");
    if (!/^publish\s*=\s*false$/m.test(manifest)) problems.push(`${path} is publishable`);
  }
  const rootManifest = readFileSync(join(root, "Cargo.toml"), "utf8");
  const defaultMembers = /^default-members\s*=\s*\[([\s\S]*?)\]/m.exec(rootManifest)?.[1] ?? "";
  if (defaultMembers.includes("tests/quality-gates")) {
    problems.push("tests/quality-gates/Cargo.toml is a default member and would ship with every build");
  }
  return problems;
});

if (failures.length > 0) {
  console.error(`\n${failures.length} boundary assertion(s) failed`);
  process.exit(1);
}
console.log("\nall boundary assertions passed");
