import { createRequire } from "node:module";

import type { VuTeXBindings } from "../types.js";

type NativeBinding = {
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

const PLATFORM_PACKAGES: Record<string, string> = {
    "win32-x64": "@vutex/vutex-win32-x64",
    "linux-x64": "@vutex/vutex-linux-x64",
    "linux-arm64": "@vutex/vutex-linux-arm64",
    "darwin-x64": "@vutex/vutex-darwin-x64",
    "darwin-arm64": "@vutex/vutex-darwin-arm64",
};

/** Load the platform-specific Node-API binary from `@vutex/vutex-<platform>`. */
export function loadVuTeXNode(): VuTeXBindings {
    const key = `${process.platform}-${process.arch}`;
    const pkg = PLATFORM_PACKAGES[key];
    if (!pkg) {
        throw new Error(`Unsupported platform for VuTeX native bindings: ${key}`);
    }
    const require = createRequire(import.meta.url);
    const binding = require(pkg).default as NativeBinding;
    return {
        vutexVersion: () => binding.vutexVersion(),
        measureFormula: (source, optionsJson) => binding.measureFormula(source, optionsJson),
        renderFormulaSvg: (source, optionsJson) => binding.renderFormulaSvg(source, optionsJson),
        renderFormulaMathml: (source, optionsJson) => binding.renderFormulaMathml(source, optionsJson),
        renderFormula: (source, optionsJson) => binding.renderFormula(source, optionsJson),
        inspectDocument: (inputPath) => binding.inspectDocument(inputPath),
        renderDocument: (inputPath, optionsJson) => binding.renderDocument(inputPath, optionsJson),
    };
}
