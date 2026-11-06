import { readFile } from "node:fs/promises";
import { join } from "node:path";

import type { TemplateGeometry, VuTeXDiagnostic } from "../types.js";

const PAGE_SIZES: Record<string, { width: number; height: number }> = {
    a4: { width: 595.276, height: 841.89 },
    letter: { width: 612, height: 792 },
};

function invalid(message: string, details: Record<string, unknown>): VuTeXDiagnostic {
    return {
        code: "VUTEX_PACKAGE_TEMPLATE_INVALID",
        severity: "error",
        message,
        stage: "package",
        args: details,
    };
}

function positive(value: unknown): value is number {
    return typeof value === "number" && Number.isFinite(value) && value > 0;
}

function margin(value: unknown): number | undefined | null {
    if (value === undefined) {
        return undefined;
    }
    if (typeof value === "number" && Number.isFinite(value) && value >= 0) {
        return value;
    }
    return null;
}

/**
 * Read the page geometry a template package declares in `template.json`.
 *
 * The file is the whole template contract for now: a named page size or an
 * explicit width/height in PostScript points, plus optional margins.
 */
export async function readTemplateGeometry(
    directory: string,
): Promise<{ geometry?: TemplateGeometry; diagnostic?: VuTeXDiagnostic }> {
    const file = join(directory, "template.json");
    let parsed: unknown;
    try {
        parsed = JSON.parse(await readFile(file, "utf8"));
    } catch (error) {
        return {
            diagnostic: invalid(
                `template package at ${directory} has no readable template.json`,
                { directory, reason: error instanceof Error ? error.message : String(error) },
            ),
        };
    }
    if (parsed === null || typeof parsed !== "object") {
        return { diagnostic: invalid(`template.json at ${directory} is not an object`, { directory }) };
    }
    const record = parsed as { page?: unknown; margin?: unknown };
    if (record.page === null || typeof record.page !== "object") {
        return { diagnostic: invalid(`template.json at ${directory} has no page object`, { directory }) };
    }
    const page = record.page as { size?: unknown; width?: unknown; height?: unknown };

    let width: number;
    let height: number;
    if (typeof page.size === "string") {
        const named = PAGE_SIZES[page.size];
        if (named === undefined) {
            return {
                diagnostic: invalid(
                    `template.json names an unknown page size: ${page.size}`,
                    { directory, size: page.size },
                ),
            };
        }
        width = named.width;
        height = named.height;
    } else if (positive(page.width) && positive(page.height)) {
        width = page.width;
        height = page.height;
    } else {
        return {
            diagnostic: invalid(`template.json needs a page size or a positive width and height`, {
                directory,
            }),
        };
    }

    const geometry: TemplateGeometry = { pageWidth: width, pageHeight: height };
    if (record.margin !== undefined) {
        if (typeof record.margin === "number") {
            const uniform = margin(record.margin);
            if (uniform === null || uniform === undefined) {
                return {
                    diagnostic: invalid(`template.json has an invalid margin`, { directory }),
                };
            }
            geometry.marginTop = uniform;
            geometry.marginRight = uniform;
            geometry.marginBottom = uniform;
            geometry.marginLeft = uniform;
        } else if (record.margin !== null && typeof record.margin === "object") {
            const sides = record.margin as Record<string, unknown>;
            for (const side of ["top", "right", "bottom", "left"]) {
                const value = margin(sides[side]);
                if (value === null) {
                    return {
                        diagnostic: invalid(`template.json has an invalid ${side} margin`, { directory }),
                    };
                }
                if (value !== undefined) {
                    geometry[`margin${side[0].toUpperCase()}${side.slice(1)}` as keyof TemplateGeometry] = value;
                }
            }
        } else {
            return { diagnostic: invalid(`template.json has an invalid margin`, { directory }) };
        }
    }
    return { geometry };
}
