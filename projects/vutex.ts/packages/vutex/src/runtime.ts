import { readFile } from "node:fs/promises";
import { dirname, extname, resolve } from "node:path";

import type {
    ActivatedPackage,
    CreateVuTeXOptions,
    FormulaRender,
    FormulaRenderOptions,
    InspectDocumentOptions,
    InspectDocumentResult,
    RenderDocumentOptions,
    RenderDocumentResult,
    TemplateGeometry,
    VuTeXBindings,
    VuTeXDiagnostic,
    VuTeXRuntime,
} from "./types.js";
import { PackageManager } from "./packages/manager.js";
import { readManifest } from "./packages/manifest.js";
import { readMathFontFace } from "./packages/math_font.js";
import { readTemplateGeometry } from "./packages/template.js";

type ParsedPayload = {
    ok: boolean;
    format: string;
    graph?: unknown;
    diagnostics?: VuTeXDiagnostic[];
};

function encodeFormulaOptions(options?: FormulaRenderOptions): string | undefined {
    if (!options) {
        return undefined;
    }
    return JSON.stringify({ displayMode: options.displayMode ?? "inline" });
}

function decodeFormulaRender(raw: string): FormulaRender {
    return JSON.parse(raw) as FormulaRender;
}

/**
 * Keep unknown format ids intact so the native side can report them.
 * Only a missing value falls back to the default `svg`.
 */
function normalizeOutputFormat(value: string | undefined): string {
    return typeof value === "string" && value.length > 0 ? value : "svg";
}

function formatFromPath(inputPath: string): string {
    const extension = extname(inputPath).slice(1).toLowerCase();
    switch (extension) {
        case "md":
        case "markdown":
            return "markdown";
        case "nd":
        case "notedown":
            return "notedown";
        case "html":
        case "htm":
            return "html";
        case "tex":
        case "latex":
        case "ltx":
            return "tex";
        case "typ":
        case "typst":
            return "typst";
        case "docx":
            return "docx";
        case "epub":
            return "epub";
        default:
            return "unknown";
    }
}

function diagnostic(code: string, message: string, stage: string, path?: string): VuTeXDiagnostic {
    return { code, severity: "error", message, stage, path };
}

function byteLength(value: string): number {
    return new TextEncoder().encode(value).length;
}

function readBytes(inputPath: string): Promise<Uint8Array> {
    return readFile(resolve(inputPath)).then((buffer) => new Uint8Array(buffer));
}

/** Theme presets the native scene knows how to lay out. */
const NATIVE_THEMES = new Set(["baseline", "traditional", "editorial", "classic"]);

/**
 * Built-in math packages that install one OpenType MATH family.
 *
 * The native binding owns the same mapping, so an id outside this set fails
 * closed instead of painting a face the document never declared.
 */
const NATIVE_MATH_FONTS = new Set(["latin-modern-math", "xits-math"]);

function activatedSlot(activation: ActivatedPackage[], slot: string): ActivatedPackage | undefined {
    return activation.find((item) => item.slot === slot);
}

/**
 * Compose the native primitives into one render call.
 *
 * The runtime owns orchestration only: it reads the input bytes, selects the
 * format, chains parse -> lower -> build -> emit, and assembles the report.
 * Font embedding happens inside the emission primitives, so this file never
 * names a face.
 */
export function createVuTeXRuntime(
    bindings: VuTeXBindings,
    options: CreateVuTeXOptions = {},
): VuTeXRuntime {
    const projectRoot = options.projectRoot ?? process.cwd();
    const packages = new PackageManager(projectRoot);

    async function render(inputPath: string, renderOptions: RenderDocumentOptions = {}): Promise<RenderDocumentResult> {
        const manifest = await readManifest(projectRoot);
        const outputFormat = normalizeOutputFormat(renderOptions.output ?? manifest?.document?.output);
        const inputFormat = renderOptions.input ?? manifest?.document?.input ?? formatFromPath(inputPath);
        const explicitTheme = typeof renderOptions.theme === "string" && renderOptions.theme.length > 0
            ? renderOptions.theme
            : undefined;
        const inputs = JSON.stringify([{ path: inputPath, format: inputFormat }]);
        const activation = await packages.activation();
        const activationErrors = activation.diagnostics.filter((item) => item.severity === "error");
        if (activationErrors.length > 0) {
            return {
                outputFormat,
                reportJson: bindings.assembleReport("render", inputs, JSON.stringify(activation.diagnostics)),
                diagnostics: activation.diagnostics,
                packages: [],
            };
        }

        const fail = (item: VuTeXDiagnostic): RenderDocumentResult => {
            const items = [...activation.diagnostics, item];
            return {
                outputFormat,
                reportJson: bindings.assembleReport("render", inputs, JSON.stringify(items)),
                diagnostics: items,
                packages: activation.packages,
            };
        };

        // A declared slot must change what the renderer does. `theme` and
        // `math-font` have native consumers, so an unusable value fails the
        // render instead of silently painting the built-in baseline.
        const themePackage = activatedSlot(activation.packages, "theme");
        const theme = explicitTheme ?? themePackage?.name;
        if (theme !== undefined && !NATIVE_THEMES.has(theme)) {
            return fail(
                diagnostic(
                    "VUTEX_PACKAGE_THEME_UNSUPPORTED",
                    `theme package has no native preset: ${theme}`,
                    "package",
                    inputPath,
                ),
            );
        }

        const mathPackage = activatedSlot(activation.packages, "math-font");
        const bundledMath = mathPackage !== undefined
            && mathPackage.source === "builtin"
            && NATIVE_MATH_FONTS.has(mathPackage.name);
        if (mathPackage !== undefined && !bundledMath) {
            if (mathPackage.path === undefined) {
                return fail(
                    diagnostic(
                        "VUTEX_PACKAGE_MATH_FONT_UNSUPPORTED",
                        `math font package has no resource directory: ${mathPackage.name}`,
                        "package",
                        inputPath,
                    ),
                );
            }
            // Reflowed html keeps formulas as markup, so the warning below is
            // the whole record for that format and the face is never installed.
            if (outputFormat !== "html") {
                const face = await readMathFontFace(mathPackage.path);
                if (face.diagnostic !== undefined) {
                    return fail(face.diagnostic);
                }
                if (face.file === undefined) {
                    return fail(
                        diagnostic(
                            "VUTEX_PACKAGE_MATH_FONT_INVALID",
                            `math font package ${mathPackage.name} names no font file`,
                            "package",
                            inputPath,
                        ),
                    );
                }
                let faceBytes: Uint8Array;
                try {
                    faceBytes = await readBytes(face.file);
                } catch {
                    return fail(
                        diagnostic(
                            "VUTEX_PACKAGE_MATH_FONT_INVALID",
                            `math font package ${mathPackage.name} has no readable font file`,
                            "package",
                            inputPath,
                        ),
                    );
                }
                try {
                    bindings.installMathFontFace(mathPackage.name, mathPackage.artifactHash ?? "", faceBytes);
                } catch (failure) {
                    return fail(
                        diagnostic(
                            "VUTEX_PACKAGE_MATH_FONT_INVALID",
                            failure instanceof Error ? failure.message : String(failure),
                            "package",
                            inputPath,
                        ),
                    );
                }
            }
        }
        const mathFont = mathPackage?.name;

        let bytes: Uint8Array;
        try {
            bytes = await readBytes(inputPath);
        } catch {
            return fail(diagnostic("VUTEX_INPUT_NOT_FOUND", `input path does not exist: ${inputPath}`, "read", inputPath));
        }

        let parsed: ParsedPayload;
        try {
            parsed = JSON.parse(bindings.parseDocument(inputPath, inputFormat, bytes)) as ParsedPayload;
        } catch (failure) {
            const message = failure instanceof Error ? failure.message : String(failure);
            return fail(diagnostic("VUTEX_IMPORT_FAILED", message, "import", inputPath));
        }
        if (!parsed.ok || parsed.graph === undefined) {
            const items = parsed.diagnostics ?? [];
            const first = items[0] ?? diagnostic("VUTEX_IMPORT_FAILED", "import produced no graph", "import", inputPath);
            return {
                outputFormat,
                reportJson: bindings.assembleReport("render", inputs, JSON.stringify(items)),
                diagnostics: items.length > 0 ? items : [first],
                packages: activation.packages,
            };
        }

        if (!["svg", "html", "snapshot", "pdf"].includes(outputFormat)) {
            return fail(
                diagnostic("VUTEX_OUTPUT_FORMAT_UNSUPPORTED", `unsupported output format: ${outputFormat}`, "render", inputPath),
            );
        }

        const graphJson = JSON.stringify(parsed.graph);
        const diagnostics: VuTeXDiagnostic[] = [
            ...(parsed.diagnostics ?? []),
            ...activation.diagnostics,
        ];
        const templatePackage = activatedSlot(activation.packages, "template");
        let templateApplied: string | undefined;
        let templateGeometry: TemplateGeometry | undefined;
        if (templatePackage !== undefined) {
            // Reflowed html has no fixed pages, so a page template has nothing
            // to act on and only the warning records the gap.
            if (outputFormat === "html") {
                diagnostics.push({
                    code: "VUTEX_PACKAGE_TEMPLATE_INACTIVE",
                    severity: "warning",
                    message: `reflowed html has no fixed pages, so template ${templatePackage.name} does not apply`,
                    stage: "package",
                    args: { slot: "template", package: templatePackage.name },
                });
            } else {
                if (templatePackage.path === undefined) {
                    return fail(
                        diagnostic(
                            "VUTEX_PACKAGE_TEMPLATE_UNSUPPORTED",
                            `template package has no resource directory: ${templatePackage.name}`,
                            "package",
                            inputPath,
                        ),
                    );
                }
                const read = await readTemplateGeometry(templatePackage.path);
                if (read.diagnostic !== undefined) {
                    return fail(read.diagnostic);
                }
                templateGeometry = read.geometry;
                templateApplied = templatePackage.name;
            }
        }
        let mathFontApplied: string | undefined;
        try {
            const lowered = bindings.lowerDocument(graphJson, inputPath);
            const footnotes = JSON.parse(lowered.footnotesJson()) as Record<string, number>;
            diagnostics.push(...(JSON.parse(lowered.diagnosticsJson()) as VuTeXDiagnostic[]));

            let svg: string | undefined;
            let html: string | undefined;
            let pdf: Uint8Array | undefined;
            let outputFormatLabel = outputFormat;
            let pages: number | undefined;
            let items: number | undefined;

            if (outputFormat === "html") {
                const flow = bindings.buildFlow(lowered.graphJson(), theme);
                html = flow.html();
                if (mathFont !== undefined) {
                    diagnostics.push({
                        code: "VUTEX_PACKAGE_MATH_FONT_INACTIVE",
                        severity: "warning",
                        message: `reflowed html keeps formulas as markup, so ${mathFont} does not apply`,
                        stage: "package",
                        args: { slot: "math-font", package: mathFont },
                    });
                }
            } else {
                const scene = bindings.buildDisplay(lowered, theme, mathFont, templateGeometry);
                mathFontApplied = scene.mathFontId();
                pages = scene.pageCount();
                items = scene.itemCount();
                diagnostics.push(...(JSON.parse(scene.diagnosticsJson()) as VuTeXDiagnostic[]));
                if (outputFormat === "svg") {
                    svg = scene.svg();
                } else if (outputFormat === "snapshot") {
                    html = scene.snapshotHtml();
                } else {
                    const rendered = scene.pdf(JSON.stringify({
                        base_dir: renderOptions.baseDir ?? dirname(resolve(inputPath)),
                        resources: renderOptions.resources,
                    }));
                    pdf = new Uint8Array(rendered.pdf);
                    diagnostics.push(...(JSON.parse(rendered.diagnosticsJson) as VuTeXDiagnostic[]));
                }
            }

            const bodyBytes = outputFormat === "pdf" ? (pdf?.byteLength ?? 0) : byteLength(svg ?? html ?? "");
            const reportJson = bindings.assembleReport(
                "render",
                inputs,
                JSON.stringify(diagnostics),
                JSON.stringify({
                    format: outputFormatLabel,
                    bytes: bodyBytes,
                    ...(pages === undefined ? {} : { pages, items }),
                }),
                JSON.stringify({
                    footnotes,
                    theme: theme ?? "baseline",
                    mathFont: mathFontApplied,
                    template: templateApplied,
                    packages: activation.packages,
                }),
            );

            return {
                outputFormat: outputFormatLabel,
                svg,
                html,
                pdf,
                reportJson,
                diagnostics,
                packages: activation.packages,
            };
        } catch (failure) {
            const message = failure instanceof Error ? failure.message : String(failure);
            const item = diagnostic("VUTEX_RENDER_FAILED", message, "render", inputPath);
            diagnostics.push(item);
            return {
                outputFormat,
                reportJson: bindings.assembleReport("render", inputs, JSON.stringify(diagnostics)),
                diagnostics,
                packages: activation.packages,
            };
        }
    }

    async function inspect(inputPath: string, _options: InspectDocumentOptions = {}): Promise<InspectDocumentResult> {
        const inputFormat = formatFromPath(inputPath);
        const inputs = JSON.stringify([{ path: inputPath, format: inputFormat }]);
        let bytes: Uint8Array;
        try {
            bytes = await readBytes(inputPath);
        } catch {
            const item = diagnostic("VUTEX_INPUT_NOT_FOUND", `input path does not exist: ${inputPath}`, "inspect", inputPath);
            return {
                inputFormat,
                path: inputPath,
                reportJson: bindings.assembleReport("inspect", inputs, JSON.stringify([item])),
                diagnostics: [item],
            };
        }

        const parsed = JSON.parse(bindings.parseDocument(inputPath, inputFormat, bytes)) as ParsedPayload;
        const diagnostics = parsed.diagnostics ?? [];
        const extra = parsed.ok && parsed.graph !== undefined
            ? bindings.summarizeDocument(JSON.stringify(parsed.graph))
            : undefined;
        return {
            inputFormat: parsed.format,
            path: inputPath,
            reportJson: bindings.assembleReport("inspect", inputs, JSON.stringify(diagnostics), undefined, extra),
            diagnostics,
        };
    }

    return {
        version: () => bindings.vutexVersion(),
        render,
        inspect,
        measureFormula(source, formulaOptions) {
            return decodeFormulaRender(bindings.measureFormula(source, encodeFormulaOptions(formulaOptions)));
        },
        renderFormula(source, formulaOptions) {
            return decodeFormulaRender(bindings.renderFormula(source, encodeFormulaOptions(formulaOptions)));
        },
        packages: {
            list: () => packages.list(),
            add: (name, opts) => packages.add(name, opts),
            remove: (name) => packages.remove(name),
            install: (opts) => packages.install(opts),
            update: () => packages.update(),
            doctor: () => packages.doctor(),
        },
    };
}
