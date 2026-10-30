import { loadWeTeX } from "./wasm/load.js";
import type { FormulaRender, RenderOptions, WeTeXBindings, WeTeXWasmOptions } from "./types.js";

export type {
    DisplayMode,
    FontFamily,
    FormulaDiagnostic,
    FormulaMetrics,
    FormulaRender,
    RenderOptions,
    WeTeXBindings,
    WeTeXWasmOptions,
} from "./types.js";

export { loadWeTeX } from "./wasm/index.js";

let defaultBindings: WeTeXBindings | undefined;

async function bindings(options?: WeTeXWasmOptions): Promise<WeTeXBindings> {
    if (!defaultBindings) {
        defaultBindings = await loadWeTeX(options);
    }
    return defaultBindings;
}

/** Measure formula metrics through the default WASM binding. */
export async function measure(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WeTeXWasmOptions,
): Promise<FormulaRender> {
    const wetex = await bindings(wasmOptions);
    return wetex.measureFormula(source, options);
}

/** Render a formula to SVG through the default WASM binding. */
export async function renderSvg(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WeTeXWasmOptions,
): Promise<FormulaRender> {
    const wetex = await bindings(wasmOptions);
    return wetex.renderSvg(source, options);
}

/** Render a formula to MathML through the default WASM binding. */
export async function renderMathml(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WeTeXWasmOptions,
): Promise<FormulaRender> {
    const wetex = await bindings(wasmOptions);
    return wetex.renderMathml(source, options);
}

/** Render a formula with SVG and MathML through the default WASM binding. */
export async function render(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WeTeXWasmOptions,
): Promise<FormulaRender> {
    const wetex = await bindings(wasmOptions);
    return wetex.renderFormula(source, options);
}

/** Convenience helpers when bindings are already loaded. */
export function measureWith(
    wetex: WeTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return wetex.measureFormula(source, options);
}

export function renderSvgWith(
    wetex: WeTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return wetex.renderSvg(source, options);
}

export function renderMathmlWith(
    wetex: WeTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return wetex.renderMathml(source, options);
}

export function renderWith(
    wetex: WeTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return wetex.renderFormula(source, options);
}
