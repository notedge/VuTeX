import { createRequire } from "node:module";

import type { LoweredDocumentHandle, DisplaySceneHandle, FlowSceneHandle, TemplateGeometry, VuTeXBindings } from "../types.js";

type NativeBinding = VuTeXBindings & {
    lowerDocument: (graphJson: string, label: string) => LoweredDocumentHandle;
    buildDisplay: (lowered: LoweredDocumentHandle, theme?: string, mathFont?: string, template?: TemplateGeometry) => DisplaySceneHandle;
    buildFlow: (graphJson: string, theme?: string) => FlowSceneHandle;
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
        parseDocument: (label, format, bytes) => binding.parseDocument(label, format, bytes),
        lowerDocument: (graphJson, label) => binding.lowerDocument(graphJson, label),
        buildDisplay: (lowered, theme, mathFont, template) => binding.buildDisplay(lowered, theme, mathFont, template),
        buildFlow: (graphJson, theme) => binding.buildFlow(graphJson, theme),
        installMathFontFace: (name, artifactHash, bytes, texParameters) =>
            binding.installMathFontFace(name, artifactHash, bytes, texParameters),
        mathFontTexParameterFamilies: () => binding.mathFontTexParameterFamilies(),
        embedSvgFonts: (svg, theme) => binding.embedSvgFonts(svg, theme),
        subsetFace: (family, script, style, chars) => binding.subsetFace(family, script, style, chars),
        summarizeDocument: (graphJson) => binding.summarizeDocument(graphJson),
        assembleReport: (operation, inputsJson, diagnosticsJson, outputJson, extraJson) =>
            binding.assembleReport(operation, inputsJson, diagnosticsJson, outputJson, extraJson),
        themeFacts: (theme) => binding.themeFacts(theme),
    };
}
