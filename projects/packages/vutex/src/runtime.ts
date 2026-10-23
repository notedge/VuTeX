import type {
    CreateVuTeXOptions,
    FormulaRender,
    FormulaRenderOptions,
    InspectDocumentOptions,
    InspectDocumentResult,
    RenderDocumentOptions,
    RenderDocumentResult,
    VuTeXBindings,
    VuTeXDiagnostic,
    VuTeXRuntime,
} from "./types.js";
import { PackageManager } from "./packages/manager.js";
import { readManifest } from "./packages/manifest.js";

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

function diagnosticsFromReport(reportJson: string): VuTeXDiagnostic[] {
    try {
        const report = JSON.parse(reportJson) as {
            diagnostics?: VuTeXDiagnostic[];
        };
        return report.diagnostics ?? [];
    } catch {
        return [];
    }
}

export function createVuTeXRuntime(
    bindings: VuTeXBindings,
    options: CreateVuTeXOptions = {},
): VuTeXRuntime {
    const projectRoot = options.projectRoot ?? process.cwd();
    const packages = new PackageManager(projectRoot);

    return {
        version: () => bindings.vutexVersion(),
        async render(inputPath, renderOptions = {}): Promise<RenderDocumentResult> {
            const manifest = await readManifest(projectRoot);
            const payload = JSON.stringify({
                output: normalizeOutputFormat(renderOptions.output ?? manifest?.document?.output),
                profile: renderOptions.profile ?? manifest?.profile?.name,
                input: renderOptions.input ?? manifest?.document?.input,
            });
            const response = bindings.renderDocument(inputPath, payload);
            return {
                exitCode: response.exitCode,
                outputFormat: response.outputFormat,
                svg: response.svg,
                html: response.html,
                pdf: response.pdf,
                reportJson: response.reportJson,
                diagnostics: diagnosticsFromReport(response.reportJson),
            };
        },
        async inspect(inputPath, _options: InspectDocumentOptions = {}): Promise<InspectDocumentResult> {
            const response = bindings.inspectDocument(inputPath);
            return {
                inputFormat: response.inputFormat,
                path: response.path,
                reportJson: response.reportJson,
                diagnostics: diagnosticsFromReport(response.reportJson),
            };
        },
        measureFormula(source, options) {
            return decodeFormulaRender(bindings.measureFormula(source, encodeFormulaOptions(options)));
        },
        renderFormula(source, options) {
            return decodeFormulaRender(bindings.renderFormula(source, encodeFormulaOptions(options)));
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
