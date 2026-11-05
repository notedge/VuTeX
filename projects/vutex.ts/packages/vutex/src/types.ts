export type VuTeXHost = "node" | "browser" | "wasm";

export type VuTeXBackend = "auto" | "native" | "wasm";

export type CreateVuTeXOptions = {
    projectRoot?: string;
    host?: VuTeXHost;
    backend?: VuTeXBackend;
};

export type DocumentOutputFormat = "svg" | "html" | "snapshot" | "pdf";

export type DocumentTheme = "baseline" | "traditional";

export type RenderDocumentOptions = {
    output?: DocumentOutputFormat | string;
    profile?: string;
    input?: string;
    theme?: DocumentTheme | string;
    baseDir?: string;
    resources?: Record<string, string>;
};

export type InspectDocumentOptions = {
    input?: string;
};

export type FormulaRenderOptions = {
    displayMode?: "inline" | "block";
};

export type FormulaDiagnostic = {
    code: string;
    severity: "info" | "warning" | "error";
    message: string;
    start?: number;
    end?: number;
};

export type FormulaMetrics = {
    width: number;
    height: number;
    depth: number;
    baseline: number;
};

export type FormulaRender = {
    metrics: FormulaMetrics;
    svg: string;
    mathml?: string;
    diagnostics: FormulaDiagnostic[];
};

export type VuTeXDiagnostic = {
    code: string;
    severity: "info" | "warning" | "error";
    message: string;
    stage?: string;
    path?: string;
    args?: Record<string, unknown>;
    cause?: string;
};

export type RenderDocumentResult = {
    outputFormat: string;
    svg?: string;
    html?: string;
    pdf?: Uint8Array;
    reportJson: string;
    diagnostics: VuTeXDiagnostic[];
};

export type InspectDocumentResult = {
    inputFormat: string;
    path: string;
    reportJson: string;
    diagnostics: VuTeXDiagnostic[];
};

export type PackageInstallOptions = {
    name?: string;
    version?: string;
};

export type PackageRecord = {
    name: string;
    version: string;
    source: string;
};

export type PackageDoctorReport = {
    ok: boolean;
    checks: Array<{ id: string; ok: boolean; message: string }>;
};

/**
 * Lowered document handle returned by `lowerDocument`.
 *
 * The handle carries the diagram and footnote resolution between the lowering
 * and layout primitives. It is a scene value, not a file or process handle.
 */
export type LoweredDocumentHandle = {
    graphJson: () => string;
    diagnosticsJson: () => string;
    footnotesJson: () => string;
};

/**
 * Paginated display scene returned by `buildDisplay`.
 *
 * Each method is one emission primitive. The SVG and HTML methods embed the
 * bundled fonts themselves, so callers never assemble a font list.
 */
export type DisplaySceneHandle = {
    pageCount: () => number;
    itemCount: () => number;
    themeId: () => string;
    diagnosticsJson: () => string;
    svg: () => string;
    snapshotHtml: () => string;
    pdf: (resourcesJson?: string) => { pdf: Uint8Array; diagnosticsJson: string };
};

/** Semantic reflow scene returned by `buildFlow`. */
export type FlowSceneHandle = {
    html: () => string;
};

/**
 * Native primitive surface.
 *
 * Exactly these calls exist on the library side. Reading input files, writing
 * output files, deciding exit codes, and printing belong to the CLI package.
 */
export type VuTeXBindings = {
    vutexVersion: () => string;
    measureFormula: (source: string, optionsJson?: string) => string;
    renderFormulaSvg: (source: string, optionsJson?: string) => string;
    renderFormulaMathml: (source: string, optionsJson?: string) => string;
    renderFormula: (source: string, optionsJson?: string) => string;
    parseDocument: (label: string, format: string, bytes: Uint8Array) => string;
    lowerDocument: (graphJson: string, label: string) => LoweredDocumentHandle;
    buildDisplay: (lowered: LoweredDocumentHandle, theme?: string) => DisplaySceneHandle;
    buildFlow: (graphJson: string, theme?: string) => FlowSceneHandle;
    embedSvgFonts: (svg: string, theme?: string) => string;
    subsetFace: (family: string, script: string, style: string, chars: string) => Uint8Array | null;
    summarizeDocument: (graphJson: string) => string;
    assembleReport: (
        operation: string,
        inputsJson: string,
        diagnosticsJson: string,
        outputJson?: string,
        extraJson?: string,
    ) => string;
    themeFacts: (theme?: string) => string;
};

export type VuTeXRuntime = {
    version: () => string;
    render: (inputPath: string, options?: RenderDocumentOptions) => Promise<RenderDocumentResult>;
    inspect: (inputPath: string, options?: InspectDocumentOptions) => Promise<InspectDocumentResult>;
    measureFormula: (source: string, options?: FormulaRenderOptions) => FormulaRender;
    renderFormula: (source: string, options?: FormulaRenderOptions) => FormulaRender;
    packages: {
        list: () => Promise<PackageRecord[]>;
        add: (name: string, options?: PackageInstallOptions) => Promise<void>;
        remove: (name: string) => Promise<void>;
        install: (options?: PackageInstallOptions) => Promise<void>;
        update: () => Promise<void>;
        doctor: () => Promise<PackageDoctorReport>;
    };
};
