import { readFile } from "node:fs/promises";
import { isAbsolute, join, relative, resolve } from "node:path";

import type { VuTeXDiagnostic } from "../types.js";

function invalid(message: string, details: Record<string, unknown>): VuTeXDiagnostic {
    return {
        code: "VUTEX_PACKAGE_MATH_FONT_INVALID",
        severity: "error",
        message,
        stage: "package",
        args: details,
    };
}

/**
 * Locate the OpenType MATH face a math-font package declares in `math-font.json`.
 *
 * The file names one font inside the package root and may name the TeX
 * parameter family the face's metrics follow. Whether the bytes actually carry
 * a parsable `MATH` table is the native registry's decision, and whether the
 * parameter family is known is the native list's decision; this reader only
 * resolves the paths and refuses to leave the package.
 */
export async function readMathFontFace(
    directory: string,
): Promise<{ file?: string; texParameters?: string; diagnostic?: VuTeXDiagnostic }> {
    const record = join(directory, "math-font.json");
    let parsed: unknown;
    try {
        parsed = JSON.parse(await readFile(record, "utf8"));
    } catch (error) {
        return {
            diagnostic: invalid(`math font package at ${directory} has no readable math-font.json`, {
                directory,
                reason: error instanceof Error ? error.message : String(error),
            }),
        };
    }
    if (parsed === null || typeof parsed !== "object") {
        return { diagnostic: invalid(`math-font.json at ${directory} is not an object`, { directory }) };
    }
    const declared = (parsed as { file?: unknown }).file;
    if (typeof declared !== "string" || declared.trim().length === 0) {
        return { diagnostic: invalid(`math-font.json at ${directory} names no font file`, { directory }) };
    }
    const texParameters = (parsed as { texParameters?: unknown }).texParameters;
    if (texParameters !== undefined && (typeof texParameters !== "string" || texParameters.trim().length === 0)) {
        return { diagnostic: invalid(`math-font.json at ${directory} declares an empty TeX parameter family`, { directory }) };
    }
    const root = resolve(directory);
    const file = resolve(root, declared);
    const inside = relative(root, file);
    if (isAbsolute(declared) || inside.startsWith("..") || isAbsolute(inside)) {
        return {
            diagnostic: invalid(`math-font.json at ${directory} points outside the package`, {
                directory,
                file: declared,
            }),
        };
    }
    return { file, texParameters: texParameters === undefined ? undefined : texParameters.trim() };
}
