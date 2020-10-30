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
                output: renderOptions.output ?? manifest?.document?.output ?? "svg",
                profile: renderOptions.profile ?? manifest?.profile?.name,
                input: renderOptions.input ?? manifest?.document?.input,
            });
            const response = bindings.renderDocument(inputPath, payload);
            return {
                exitCode: response.exitCode,
                outputFormat: response.outputFormat,
                svg: response.svg,
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
