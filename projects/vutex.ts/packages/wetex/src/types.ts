export type DisplayMode = "inline" | "block";

/** Math font family selector (Rust `FontFamily`, kebab-case serde). */
export type FontFamily = "lm-math" | "xits-math" | "stix-two-math";

export type RenderOptions = {
    displayMode?: DisplayMode;
    /**
     * Strict path-only SVG output. Defaults to `true` on the Rust side, so
     * the public render path never falls back to a host-font `<text>` run.
     * Passing `false` opts into that fallback explicitly.
     */
    outline?: boolean;
    /**
     * Explicit base font size in CSS pixels. When omitted the renderer uses
     * its display-mode default (14px inline, 16px block).
     */
    fontSize?: number;
    /** Math font family. Defaults to `"lm-math"` — the only embedded family today. */
    fontFamily?: FontFamily;
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
    /** Fetch the wasm artifact from this URL (browser hosts). */
    url?: string | URL;
    /**
     * Instantiate from already-read wasm bytes (Node hosts, where the
     * artifact has no fetchable URL). Takes precedence over `url`.
     */
    bytes?: Uint8Array;
};

export type WeTeXBindings = {
    wetexVersion: () => string;
    measureFormula: (source: string, options?: RenderOptions) => FormulaRender;
    renderSvg: (source: string, options?: RenderOptions) => FormulaRender;
    renderMathml: (source: string, options?: RenderOptions) => FormulaRender;
    renderFormula: (source: string, options?: RenderOptions) => FormulaRender;
};
