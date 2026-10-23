export type VuTeXHost = "node" | "browser" | "wasm";

export type VuTeXBackend = "auto" | "native" | "wasm";

export type CreateVuTeXOptions = {
    projectRoot?: string;
    host?: VuTeXHost;
    backend?: VuTeXBackend;
};

export type DocumentOutputFormat = "svg" | "html" | "snapshot";

export type RenderDocumentOptions = {
    output?: DocumentOutputFormat | string;
    profile?: string;
    input?: string;
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
    exitCode: number;
    outputFormat: string;
    svg?: string;
    html?: string;
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

export type VuTeXBindings = {
    vutexVersion: () => string;
    measureFormula: (source: string, optionsJson?: string) => string;
    renderFormulaSvg: (source: string, optionsJson?: string) => string;
    renderFormulaMathml: (source: string, optionsJson?: string) => string;
    renderFormula: (source: string, optionsJson?: string) => string;
    inspectDocument: (inputPath: string) => {
        inputFormat: string;
        path: string;
        reportJson: string;
    };
    renderDocument: (inputPath: string, optionsJson?: string) => {
        exitCode: number;
        outputFormat: string;
        svg?: string;
        html?: string;
        reportJson: string;
    };
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
