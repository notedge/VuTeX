import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";

import { loadNative, resultsRoot } from "../lib/ours.mjs";
import { createReport } from "../lib/report.mjs";

const require = createRequire(import.meta.url);
const standardFontDataUrl = `${resolve(dirname(require.resolve("pdfjs-dist/package.json")), "standard_fonts")}${sep}`;

const here = dirname(fileURLToPath(import.meta.url));
const corpusDir = resolve(here, "corpus");
const cases = JSON.parse(readFileSync(resolve(corpusDir, "cases.json"), "utf8")).cases;
const pdfDir = resolve(resultsRoot, ".build", "pdf");
mkdirSync(pdfDir, { recursive: true });

const native = loadNative();

function renderPdf(markdown) {
    const path = resolve(corpusDir, markdown);
    const result = native.renderDocument(path, JSON.stringify({ output: "pdf" }));
    if (result.exitCode !== 0) {
        throw new Error(`renderDocument failed for ${markdown}: ${result.reportJson}`);
    }
    const output = resolve(pdfDir, markdown.replace(/\.md$/, ".pdf"));
    writeFileSync(output, result.pdf);
    return output;
}

function normalise(text) {
    return text.replace(/\u00a0/g, " ").replace(/\s+/g, " ").trim();
}

async function extract(path) {
    const data = new Uint8Array(readFileSync(path));
    const document = await pdfjs.getDocument({
        data,
        isEvalSupported: false,
        useWorkerFetch: false,
        standardFontDataUrl,
    }).promise;
    const pages = [];
    let pathOps = 0;
    for (let index = 1; index <= document.numPages; index += 1) {
        const page = await document.getPage(index);
        const content = await page.getTextContent();
        pages.push(normalise(content.items.map((item) => item.str).join(" ")));
        const operators = await page.getOperatorList();
        pathOps += operators.fnArray.filter((fn) => fn === pdfjs.OPS.constructPath).length;
    }
    return { pages: document.numPages, text: normalise(pages.join(" ")), pathOps };
}

const report = createReport("pdfjs");

for (const testCase of cases) {
    const pdfPath = renderPdf(testCase.markdown);
    const found = await extract(pdfPath);

    if (testCase.features.includes("page_count")) {
        report.compare({
            caseId: testCase.id,
            feature: "page_count",
            ours: found.pages,
            theirs: [{ engine: "pdfjs-dist", value: testCase.pages }],
            tolerance: 0,
        });
    }
    if (testCase.features.includes("text_normalised")) {
        report.compareText({
            caseId: testCase.id,
            feature: "text_normalised",
            ours: found.text,
            theirs: normalise(testCase.expected_text),
            classification: "real-defect",
            note: "pdfjs extracts the rendered text, the expectation is the authored prose",
        });
    }
    if (testCase.features.includes("math_searchable")) {
        const searchable = found.text.includes(testCase.formula.replace(/\s+/g, " "));
        report.compareText({
            caseId: testCase.id,
            feature: "math_searchable",
            ours: searchable,
            theirs: true,
            classification: "our-limitation",
            note: `formula "${testCase.formula}" is expected to be searchable after typesetting`,
        });
    }
    if (testCase.features.includes("math_typeset")) {
        report.record({
            case: testCase.id,
            feature: "math_typeset",
            ours: { path_ops: found.pathOps, extracted: found.text },
            theirs: null,
            tolerance: "record-only",
            classification: "real-defect",
            note: "the PDF contains no vector glyphs, the formula is emitted as literal source text",
        });
    }
    console.log(
        `${testCase.id}: pdfjs pages=${found.pages} expected=${testCase.pages} ` +
        `path_ops=${found.pathOps} text="${found.text.slice(0, 60)}"`,
    );
}

report.summary("pdfjs");

if (!existsSync(pdfDir)) {
    console.warn("no PDF directory");
}
