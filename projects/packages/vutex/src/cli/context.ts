import { createVuTeX, type VuTeXRuntime } from "../index.js";
import { loadVuTeXNode } from "../node/load.js";

export type CliContext = {
    runtime: VuTeXRuntime | null;
    toolVersion: string;
};

export async function createVuTeXRuntime(projectRoot?: string): Promise<VuTeXRuntime> {
    return createVuTeX({ projectRoot: projectRoot ?? process.cwd(), host: "node", backend: "native" });
}

export function loadBindings(): ReturnType<typeof loadVuTeXNode> | null {
    try {
        return loadVuTeXNode();
    } catch {
        return null;
    }
}

export async function createContext(projectRoot?: string): Promise<CliContext> {
    const bindings = loadBindings();
    return {
        runtime: bindings ? await createVuTeX({ projectRoot: projectRoot ?? process.cwd() }) : null,
        toolVersion: bindings?.vutexVersion() ?? "0.0.0",
    };
}
