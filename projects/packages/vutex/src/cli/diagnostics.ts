import type { VuTeXDiagnostic } from "../types.js";

export function emitHuman(message: string): void {
    console.error(message);
}

export function emitJson(value: unknown): void {
    console.log(JSON.stringify(value, null, 2));
}

export function presentDiagnostics(
    diagnostics: VuTeXDiagnostic[],
    json: boolean,
): void {
    if (json) {
        emitJson({ diagnostics });
        return;
    }
    for (const item of diagnostics) {
        const stage = item.stage ? `[${item.stage}] ` : "";
        emitHuman(`${stage}${item.severity}: ${item.code} — ${item.message}`);
    }
}
