import { loadWaTeX } from "./wasm/load.js";
import type { FormulaRender, RenderOptions, WaTeXBindings, WaTeXWasmOptions } from "./types.js";

export type {
    DisplayMode,
    FontFamily,
    FormulaDiagnostic,
    FormulaMetrics,
    FormulaRender,
    RenderOptions,
    WaTeXBindings,
    WaTeXWasmOptions,
} from "./types.js";

export { loadWaTeX } from "./wasm/index.js";

let defaultBindings: WaTeXBindings | undefined;

async function bindings(options?: WaTeXWasmOptions): Promise<WaTeXBindings> {
    if (!defaultBindings) {
        defaultBindings = await loadWaTeX(options);
    }
    return defaultBindings;
}

/** Measure formula metrics through the default WASM binding. */
export async function measure(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WaTeXWasmOptions,
): Promise<FormulaRender> {
    const watex = await bindings(wasmOptions);
    return watex.measureFormula(source, options);
}

/** Render a formula to SVG through the default WASM binding. */
export async function renderSvg(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WaTeXWasmOptions,
): Promise<FormulaRender> {
    const watex = await bindings(wasmOptions);
    return watex.renderSvg(source, options);
}

/** Render a formula to an HTML fragment through the default WASM binding. */
export async function renderHtml(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WaTeXWasmOptions,
): Promise<FormulaRender> {
    const watex = await bindings(wasmOptions);
    return watex.renderHtml(source, options);
}

/** Render a formula to MathML through the default WASM binding. */
export async function renderMathml(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WaTeXWasmOptions,
): Promise<FormulaRender> {
    const watex = await bindings(wasmOptions);
    return watex.renderMathml(source, options);
}

/** Render a formula with SVG and MathML through the default WASM binding. */
export async function render(
    source: string,
    options?: RenderOptions,
    wasmOptions?: WaTeXWasmOptions,
): Promise<FormulaRender> {
    const watex = await bindings(wasmOptions);
    return watex.renderFormula(source, options);
}

/** Convenience helpers when bindings are already loaded. */
export function measureWith(
    watex: WaTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return watex.measureFormula(source, options);
}

export function renderSvgWith(
    watex: WaTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return watex.renderSvg(source, options);
}

export function renderHtmlWith(
    watex: WaTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return watex.renderHtml(source, options);
}

export function renderMathmlWith(
    watex: WaTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return watex.renderMathml(source, options);
}

export function renderWith(
    watex: WaTeXBindings,
    source: string,
    options?: RenderOptions,
): FormulaRender {
    return watex.renderFormula(source, options);
}
