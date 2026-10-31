import assert from "node:assert/strict";
import test from "node:test";

import {
    measureWith,
    renderHtmlWith,
    renderMathmlWith,
    renderSvgWith,
    renderWith,
} from "../src/index.js";
import type { FormulaRender, RenderOptions, WeTeXBindings } from "../src/types.js";

type RecordedCall = {
    method: string;
    source: string;
    options: RenderOptions | undefined;
    returned: FormulaRender;
};

const emptyDiagnostics = [
    {
        code: "WETEX_EMPTY_SOURCE",
        severity: "error",
        message: "formula source must not be empty",
        start: 0,
        end: 0,
    },
] as const;

function buildResult(method: string, source: string): FormulaRender {
    const rendersSvg = method === "renderSvg" || method === "renderFormula";
    const rendersMathml = method === "renderMathml" || method === "renderFormula";
    return {
        metrics: {
            width: source.length * 6,
            height: 12,
            depth: 1,
            baseline: 11,
        },
        svg: rendersSvg && source ? `<svg data-method="${method}">${source}</svg>` : "",
        html: method === "renderHtml" && source ? `<span data-method="${method}">${source}</span>` : "",
        mathml: rendersMathml && source ? `<math data-method="${method}">${source}</math>` : undefined,
        diagnostics: source.length === 0 ? [...emptyDiagnostics] : [],
    };
}

function recordingStub(): { bindings: WeTeXBindings; calls: RecordedCall[] } {
    const calls: RecordedCall[] = [];
    const record = (method: string) =>
        (source: string, options?: RenderOptions): FormulaRender => {
            const returned = buildResult(method, source);
            calls.push({ method, source, options, returned });
            return returned;
        };

    return {
        calls,
        bindings: {
            wetexVersion: () => "stub-version",
            measureFormula: record("measureFormula"),
            renderSvg: record("renderSvg"),
            renderHtml: record("renderHtml"),
            renderMathml: record("renderMathml"),
            renderFormula: record("renderFormula"),
        },
    };
}

test("measureWith delegates to measureFormula and passes the options object by reference", () => {
    const { bindings, calls } = recordingStub();
    const options: RenderOptions = {
        displayMode: "block",
        fontSize: 20,
        fontFamily: "lm-math",
    };

    const result = measureWith(bindings, "\\frac{a}{b}", options);

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "measureFormula");
    assert.equal(calls[0].source, "\\frac{a}{b}");
    assert.equal(calls[0].options, options);
    assert.equal(result, calls[0].returned);
    assert.deepEqual(options, {
        displayMode: "block",
        fontSize: 20,
        fontFamily: "lm-math",
    });
});

test("every facade forwards undefined options unchanged", () => {
    const { bindings, calls } = recordingStub();

    measureWith(bindings, "x^2");
    renderSvgWith(bindings, "x^2");
    renderHtmlWith(bindings, "x^2");
    renderMathmlWith(bindings, "x^2");
    renderWith(bindings, "x^2");

    assert.equal(calls.length, 5);
    for (const call of calls) {
        assert.equal(call.options, undefined);
    }
});

test("each facade maps to its binding method and payload shape", () => {
    const { bindings, calls } = recordingStub();

    const measured = measureWith(bindings, "a+b");
    const svg = renderSvgWith(bindings, "a+b");
    const html = renderHtmlWith(bindings, "a+b");
    const mathml = renderMathmlWith(bindings, "a+b");
    const merged = renderWith(bindings, "a+b");

    assert.deepEqual(
        calls.map((call) => call.method),
        ["measureFormula", "renderSvg", "renderHtml", "renderMathml", "renderFormula"],
    );

    assert.equal(measured.svg, "");
    assert.equal(measured.html, "");
    assert.equal(measured.mathml, undefined);

    assert.match(svg.svg, /^<svg data-method="renderSvg">a\+b<\/svg>$/);
    assert.equal(svg.html, "");
    assert.equal(svg.mathml, undefined);

    assert.equal(html.svg, "");
    assert.match(html.html, /^<span data-method="renderHtml">a\+b<\/span>$/);
    assert.equal(html.mathml, undefined);

    assert.equal(mathml.svg, "");
    assert.equal(mathml.html, "");
    assert.match(mathml.mathml ?? "", /^<math data-method="renderMathml">a\+b<\/math>$/);

    assert.match(merged.svg, /data-method="renderFormula"/);
    assert.match(merged.mathml ?? "", /data-method="renderFormula"/);
    assert.equal(merged.html, "");
});

test("renderWith returns the exact object produced by renderFormula", () => {
    const { bindings, calls } = recordingStub();

    const result = renderWith(bindings, "\\sqrt{2}", { displayMode: "block" });

    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "renderFormula");
    assert.equal(result, calls[0].returned);
});

test("empty-source diagnostics propagate through every facade", () => {
    const { bindings, calls } = recordingStub();

    const results = [
        measureWith(bindings, ""),
        renderSvgWith(bindings, ""),
        renderHtmlWith(bindings, ""),
        renderMathmlWith(bindings, ""),
        renderWith(bindings, ""),
    ];

    assert.equal(calls.length, 5);
    for (const result of results) {
        assert.deepEqual(result.diagnostics, emptyDiagnostics);
        assert.equal(result.svg, "");
    }
});
