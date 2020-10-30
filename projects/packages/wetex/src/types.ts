export type DisplayMode = "inline" | "block";

export type RenderOptions = {
    displayMode?: DisplayMode;
};

export type FormulaMetrics = {
    width: number;
    height: number;
    depth: number;
    baseline: number;
};

export type FormulaDiagnostic = {
    code: string;
    severity: "info" | "warning" | "error";
    message: string;
    start?: number;
    end?: number;
};

export type FormulaRender = {
    metrics: FormulaMetrics;
    svg: string;
    mathml?: string;
    diagnostics: FormulaDiagnostic[];
};

export type WeTeXWasmOptions = {
    url?: string | URL;
};

export type WeTeXBindings = {
    wetexVersion: () => string;
    measureFormula: (source: string, options?: RenderOptions) => FormulaRender;
    renderSvg: (source: string, options?: RenderOptions) => FormulaRender;
    renderMathml: (source: string, options?: RenderOptions) => FormulaRender;
    renderFormula: (source: string, options?: RenderOptions) => FormulaRender;
};
