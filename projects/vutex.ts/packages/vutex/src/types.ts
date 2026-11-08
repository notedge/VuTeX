export type VuTeXHost = "node" | "browser" | "wasm";

export type VuTeXBackend = "auto" | "native" | "wasm";

export type CreateVuTeXOptions = {
    projectRoot?: string;
    host?: VuTeXHost;
    backend?: VuTeXBackend;
    /**
     * Directory the native side reads fonts from (path P2). It may be the
     * installed package directory, an offline directory, or a system font
     * directory. When set it wins over `VUTEX_FONT_DIR` and the packaged
     * assets, and an invalid directory fails instead of falling through.
     */
    assetsDir?: string;
    /**
     * Fallback theme, math face and input format when neither the render call
     * nor the manifest picks one. Distributions such as `@vutex/latex` set
     * these so a preset never has to write into the user's project.
     */
    defaults?: {
        theme?: string;
        mathFont?: string;
        /**
         * Input format used when the render call and the manifest stay silent.
         * An input distribution such as `@vutex/typst` sets this so files with
         * no recognised extension still reach the right adapter.
         */
        input?: string;
    };
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
    span?: { start: number; end: number };
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
    packages?: ActivatedPackage[];
};

export type InspectDocumentResult = {
    inputFormat: string;
    path: string;
    reportJson: string;
    diagnostics: VuTeXDiagnostic[];
};

export type PackageSlot =
    | "font"
    | "math-font"
    | "template"
    | "theme"
    | "profile"
    | "macros"
    | "resources"
    | "fixture";

export type PackageInstallOptions = {
    name?: string;
    version?: string;
    slot?: string;
};

export type PackageRecord = {
    slot: string;
    name: string;
    version: string;
    source: string;
    artifactHash?: string;
};

export type ActivatedPackage = {
    slot: string;
    name: string;
    version: string;
    source: string;
    artifactHash?: string;
    /** Directory holding the package resources, absent for byte-less presets. */
    path?: string;
};

/** Colour tokens a template may override, each written as `#RRGGBB`. */
export type TemplateColorToken =
    | "ink"
    | "inkSecondary"
    | "inkMuted"
    | "inkFaint"
    | "accent"
    | "ruleStrong"
    | "rule"
    | "ruleFaint"
    | "tint"
    | "header"
    | "folio"
    | "headingRule";

/** Chrome a template switches on when a heading of one level starts. */
export type TemplateSectionChrome = {
    /** Heading level, H1 to H4. */
    level: number;
    /** Header source: `title`, `none`, or literal text. */
    header?: string;
    /** Whether pages in this section carry a folio. */
    folio?: boolean;
};

/** Page geometry a template package contributes to the native layout pass. */
export type TemplateGeometry = {
    pageWidth: number;
    pageHeight: number;
    marginTop?: number;
    marginRight?: number;
    marginBottom?: number;
    marginLeft?: number;
    /** Running header source: `title`, `none`, or literal text. */
    chromeHeader?: string;
    /** Whether pages carry a folio. */
    chromeFolio?: boolean;
    /** 1-based page index where the running header and folio start. */
    chromeStartPage?: number;
    /** Horizontal alignment of the running header. */
    headerAlign?: "left" | "center" | "right";
    /** Per-level chrome overrides, H1 to H4. */
    chromeSections?: TemplateSectionChrome[];
    /** Heading levels that start a new page, H1 to H4. */
    pageBreakBefore?: number[];
    /** Body text size in points. */
    bodySize?: number;
    /** Line box as a multiple of the body size. */
    lineHeight?: number;
    /** Heading size as a multiple of the body size, H1 to H4. */
    headingScales?: number[];
    /** Space after a paragraph, in points. */
    paragraphGap?: number;
    /** First-line paragraph indent, in points. */
    paragraphIndent?: number;
    /** Colour token overrides. */
    colors?: Partial<Record<TemplateColorToken, string>>;
};

export type PackageCheck = {
    id: string;
    ok: boolean;
    message: string;
    code?: string;
};

export type PackageDoctorReport = {
    ok: boolean;
    checks: PackageCheck[];
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
    mathFontId: () => string | undefined;
    diagnosticsJson: () => string;
    svg: () => string;
    snapshotHtml: () => string;
    pdf: (resourcesJson?: string) => { pdf: Uint8Array; diagnosticsJson: string };
};

/** Semantic reflow scene returned by `buildFlow`. */
export type FlowSceneHandle = {
    html: () => string;
    /** Reflow HTML plus the diagnostics formula typesetting raised. */
    htmlWithDiagnostics: () => { html: string; diagnosticsJson: string };
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
    buildDisplay: (
        lowered: LoweredDocumentHandle,
        theme?: string,
        mathFont?: string,
        template?: TemplateGeometry,
    ) => DisplaySceneHandle;
    buildFlow: (graphJson: string, theme?: string, mathFont?: string) => FlowSceneHandle;
    installMathFontFace: (name: string, artifactHash: string, bytes: Uint8Array, texParameters?: string) => void;
    mathFontTexParameterFamilies: () => string[];
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
