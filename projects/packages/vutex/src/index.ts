import { loadVuTeXNode } from "./node/load.js";
import { createVuTeXRuntime } from "./runtime.js";
import type { CreateVuTeXOptions, VuTeXRuntime } from "./types.js";

export type {
    CreateVuTeXOptions,
    DocumentOutputFormat,
    FormulaDiagnostic,
    FormulaMetrics,
    FormulaRender,
    FormulaRenderOptions,
    InspectDocumentResult,
    PackageDoctorReport,
    PackageInstallOptions,
    PackageRecord,
    RenderDocumentOptions,
    RenderDocumentResult,
    VuTeXBackend,
    VuTeXBindings,
    VuTeXDiagnostic,
    VuTeXHost,
    VuTeXRuntime,
} from "./types.js";

export { loadVuTeXNode } from "./node/index.js";
export { createVuTeXRuntime } from "./runtime.js";

/** Create a VuTeX runtime for Node hosts (native binding). */
export async function createVuTeX(options: CreateVuTeXOptions = {}): Promise<VuTeXRuntime> {
    const backend = options.backend ?? "auto";
    const host = options.host ?? "node";
    if (host !== "node" && backend !== "native") {
        throw new Error("createVuTeX scaffold currently supports Node native hosts only");
    }
    const bindings = loadVuTeXNode();
    return createVuTeXRuntime(bindings, options);
}
