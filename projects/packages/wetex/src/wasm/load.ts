import type { FormulaRender, RenderOptions, WeTeXBindings, WeTeXWasmOptions } from "../types.js";

const ARTIFACT_PACKAGE = "@vutex/vutex-unknown-wasm32";

type WasmBinding = {
    default: (input?: { module_or_path?: string | URL }) => Promise<unknown>;
    wetexVersion: () => string;
    measureFormula: (source: string, optionsJson?: string) => string;
    renderSvg: (source: string, optionsJson?: string) => string;
    renderMathml: (source: string, optionsJson?: string) => string;
    renderFormula: (source: string, optionsJson?: string) => string;
};

function encodeOptions(options?: RenderOptions): string | undefined {
    if (!options) {
        return undefined;
    }
    return JSON.stringify(options);
}

function decodeResult(raw: string): FormulaRender {
    return JSON.parse(raw) as FormulaRender;
}

function wrap(binding: WasmBinding): WeTeXBindings {
    return {
        wetexVersion: () => binding.wetexVersion(),
        measureFormula: (source, options) =>
            decodeResult(binding.measureFormula(source, encodeOptions(options))),
        renderSvg: (source, options) =>
            decodeResult(binding.renderSvg(source, encodeOptions(options))),
        renderMathml: (source, options) =>
            decodeResult(binding.renderMathml(source, encodeOptions(options))),
        renderFormula: (source, options) =>
            decodeResult(binding.renderFormula(source, encodeOptions(options))),
    };
}

/** Load the WASM artifact from `@vutex/vutex-unknown-wasm32`. */
export async function loadWeTeX(options: WeTeXWasmOptions = {}): Promise<WeTeXBindings> {
    const binding = (await import(ARTIFACT_PACKAGE)) as WasmBinding;
    if (options.url) {
        await binding.default({ module_or_path: options.url });
    } else {
        await binding.default();
    }
    return wrap(binding);
}
