import assert from "node:assert/strict";
import test from "node:test";

import { measureWith, renderSvgWith } from "../src/index.js";
import type { WeTeXBindings } from "../src/types.js";

const stub: WeTeXBindings = {
    wetexVersion: () => "0.0.0",
    measureFormula: (source, options) => ({
        metrics: {
            width: source.length * 8,
            height: 20,
            depth: 2,
            baseline: 18,
        },
        svg: "",
        diagnostics: source.trim() ? [] : [
            {
                code: "WETEX_EMPTY_SOURCE",
                severity: "error",
                message: "formula source must not be empty",
            },
        ],
    }),
    renderSvg: (source, options) => ({
        ...stub.measureFormula(source, options),
        svg: source.trim() ? `<svg data-wetex="stub">${source}</svg>` : "",
    }),
    renderMathml: (source, options) => ({
        ...stub.measureFormula(source, options),
        mathml: source.trim() ? `<math>${source}</math>` : undefined,
    }),
    renderFormula: (source, options) => ({
        ...stub.renderSvg(source, options),
        mathml: source.trim() ? `<math>${source}</math>` : undefined,
    }),
};

test("measureWith returns metrics for non-empty source", () => {
    const result = measureWith(stub, "x^2");
    assert.equal(result.metrics.width, "x^2".length * 8);
    assert.equal(result.diagnostics.length, 0);
});

test("renderSvgWith returns svg payload", () => {
    const result = renderSvgWith(stub, "a+b");
    assert.match(result.svg, /data-wetex="stub"/);
});
