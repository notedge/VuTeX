import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { instance } from "@viz-js/viz";

import { loadNative, resultsRoot } from "../lib/ours.mjs";
import { createReport } from "../lib/report.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const corpusDir = resolve(here, "corpus");
const cases = JSON.parse(readFileSync(resolve(corpusDir, "cases.json"), "utf8")).cases;
const native = loadNative();
const viz = await instance();

const buildDir = resolve(resultsRoot, ".build", "mermaid");
mkdirSync(buildDir, { recursive: true });
const puppeteerConfig = resolve(buildDir, "puppeteer.json");
// No `executablePath`: mermaid-cli rasterizes with the Chromium its Puppeteer
// dependency provisioned, so the reference does not vary with the host browser.
writeFileSync(puppeteerConfig, JSON.stringify({
    args: ["--no-sandbox", "--disable-gpu"],
}, null, 2));

// ---------------------------------------------------------------------------
// Ours: `renderDocument` wraps the diagram in `<g data-diagram="...">` and the
// nested `<svg>` is the diagram itself. Node and edge groups are marked with
// `data-role`, so the counts are structural, not heuristic.
// ---------------------------------------------------------------------------
function oursFeature(markdown) {
    const path = resolve(corpusDir, markdown);
    const result = native.renderDocument(path, JSON.stringify({ output: "svg" }));
    if (result.exitCode !== 0) {
        throw new Error(`renderDocument failed for ${markdown}: ${result.reportJson}`);
    }
    const svg = result.svg ?? "";
    const diagram = svg.match(/<g data-diagram="[^"]+"[^>]*>\s*<svg[^>]*width="([\d.]+)" height="([\d.]+)" viewBox="[^"]+"/);
    const width = diagram ? Number(diagram[1]) : null;
    const height = diagram ? Number(diagram[2]) : null;
    return {
        nodes: (svg.match(/data-role="node"/g) ?? []).length,
        edges: (svg.match(/data-role="edge"/g) ?? []).length,
        bbox: { width, height, aspect: width && height ? width / height : null },
    };
}

// ---------------------------------------------------------------------------
// Graphviz through @viz-js/viz, the npm build of the same engine.
// ---------------------------------------------------------------------------
function graphvizFeature(source) {
    const markup = readFileSync(resolve(corpusDir, source), "utf8");
    const svg = viz.renderString(markup, { format: "svg" });
    const opening = svg.match(/<svg\s+width="([\d.]+)pt"\s+height="([\d.]+)pt"/);
    const width = opening ? Number(opening[1]) : null;
    const height = opening ? Number(opening[2]) : null;
    return {
        nodes: (svg.match(/class="node"/g) ?? []).length,
        edges: (svg.match(/class="edge"/g) ?? []).length,
        bbox: { width, height, aspect: width && height ? width / height : null },
    };
}

// ---------------------------------------------------------------------------
// Mermaid through mermaid-cli. The reference SVG carries node groups and
// `flowchart-link` edges.
// ---------------------------------------------------------------------------
function mermaidFeature(source, id) {
    const binary = resolve(here, "..", "node_modules", ".bin", "mmdc.CMD");
    const input = resolve(corpusDir, source);
    const output = resolve(buildDir, `${id}.svg`);
    try {
        execFileSync(binary, ["-i", input, "-o", output, "--puppeteerConfigFile", puppeteerConfig], {
            cwd: here,
            shell: true,
            timeout: 120_000,
        });
    } catch (error) {
        console.warn(`mermaid-cli failed for ${id}: ${error.message}`);
        return null;
    }
    const svg = readFileSync(output, "utf8");
    const box = svg.match(/viewBox="0 0 ([\d.]+) ([\d.]+)"/);
    const width = box ? Number(box[1]) : null;
    const height = box ? Number(box[2]) : null;
    return {
        nodes: (svg.match(/<g class="node[ "]/g) ?? []).length,
        edges: (svg.match(/class="[^"]*flowchart-link[^"]*"/g) ?? []).length,
        bbox: { width, height, aspect: width && height ? width / height : null },
    };
}

// ---------------------------------------------------------------------------

const report = createReport("diagram");

for (const testCase of cases) {
    const ours = oursFeature(testCase.markdown);
    let theirs = null;
    let engine = null;
    try {
        if (testCase.kind === "dot") {
            engine = "graphviz";
            theirs = graphvizFeature(testCase.source);
        } else if (testCase.kind === "mermaid") {
            engine = "mermaid-cli";
            theirs = mermaidFeature(testCase.source, testCase.id);
        }
    } catch (error) {
        console.warn(`${testCase.id} reference failed: ${error.message}`);
    }

    report.compare({
        caseId: testCase.id,
        feature: "node_count",
        ours: ours.nodes,
        theirs: theirs ? [{ engine, value: theirs.nodes }] : [],
        tolerance: 0,
    });
    report.compare({
        caseId: testCase.id,
        feature: "edge_count",
        ours: ours.edges,
        theirs: theirs ? [{ engine, value: theirs.edges }] : [],
        tolerance: 0,
    });
    report.compare({
        caseId: testCase.id,
        feature: "bbox_aspect",
        ours: ours.bbox.aspect,
        theirs: theirs ? [{ engine, value: theirs.bbox.aspect }] : [],
        tolerance: 0.08,
    });
    report.compare({
        caseId: testCase.id,
        feature: "bbox_area",
        ours: ours.bbox.width && ours.bbox.height ? ours.bbox.width * ours.bbox.height : null,
        theirs: theirs
            ? [{ engine, value: theirs.bbox.width * theirs.bbox.height }]
            : [],
        tolerance: 0,
        judgment: "record-only",
    });
    console.log(
        `${testCase.id}: ours n=${ours.nodes} e=${ours.edges} ` +
        `bbox=${ours.bbox.width}x${ours.bbox.height} | ` +
        `${engine} n=${theirs?.nodes} e=${theirs?.edges} bbox=${theirs?.bbox.width}x${theirs?.bbox.height}`,
    );
}

report.summary("diagram");
