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
 * Read the page contract a template package declares in `template.json`.
 *
 * The file carries the template's first layer — a named page size or an
 * explicit width/height in PostScript points plus optional margins — and the
 * second layer: page chrome overrides (`chrome`), section page breaks
 * (`sections`), and a body style reflow (`styles`). Every second-layer block is
 * optional, so a template that declares only geometry keeps theme behaviour.
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
    const record = parsed as { page?: unknown; margin?: unknown; chrome?: unknown; sections?: unknown; styles?: unknown };
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
                    const key = `margin${side[0].toUpperCase()}${side.slice(1)}` as
                    | "marginTop"
                    | "marginRight"
                    | "marginBottom"
                    | "marginLeft";
                geometry[key] = value;
                }
            }
        } else {
            return { diagnostic: invalid(`template.json has an invalid margin`, { directory }) };
        }
    }
    if (record.chrome !== undefined) {
        if (record.chrome === null || typeof record.chrome !== "object") {
            return { diagnostic: invalid(`template.json has an invalid chrome block`, { directory }) };
        }
        const chrome = record.chrome as Record<string, unknown>;
        if (chrome.header !== undefined) {
            if (typeof chrome.header !== "string" || chrome.header.trim().length === 0) {
                return { diagnostic: invalid(`template.json has an invalid chrome header`, { directory }) };
            }
            const text = chrome.header.trim();
            if (text.length > 160) {
                return { diagnostic: invalid(`template.json chrome header is longer than 160 characters`, { directory }) };
            }
            geometry.chromeHeader = text;
        }
        if (chrome.folio !== undefined) {
            if (typeof chrome.folio !== "boolean") {
                return { diagnostic: invalid(`template.json has an invalid chrome folio flag`, { directory }) };
            }
            geometry.chromeFolio = chrome.folio;
        }
        if (chrome.startPage !== undefined) {
            if (!Number.isInteger(chrome.startPage) || (chrome.startPage as number) < 1 || (chrome.startPage as number) > 99) {
                return { diagnostic: invalid(`template.json has an invalid chrome startPage`, { directory }) };
            }
            geometry.chromeStartPage = chrome.startPage as number;
        }
        if (chrome.align !== undefined) {
            if (chrome.align !== "left" && chrome.align !== "center" && chrome.align !== "right") {
                return { diagnostic: invalid(`template.json has an invalid chrome alignment`, { directory }) };
            }
            geometry.headerAlign = chrome.align;
        }
    }
    if (record.sections !== undefined) {
        if (record.sections === null || typeof record.sections !== "object") {
            return { diagnostic: invalid(`template.json has an invalid sections block`, { directory }) };
        }
        const sections = record.sections as { pageBreakBefore?: unknown };
        if (sections.pageBreakBefore !== undefined) {
            if (!Array.isArray(sections.pageBreakBefore)) {
                return { diagnostic: invalid(`template.json sections.pageBreakBefore is not an array`, { directory }) };
            }
            const levels: number[] = [];
            for (const level of sections.pageBreakBefore) {
                if (!Number.isInteger(level) || (level as number) < 1 || (level as number) > 4) {
                    return {
                        diagnostic: invalid(`template.json page break level is outside H1 to H4`, { directory, level }),
                    };
                }
                if (!levels.includes(level as number)) {
                    levels.push(level as number);
                }
            }
            geometry.pageBreakBefore = levels;
        }
    }
    if (record.styles !== undefined) {
        if (record.styles === null || typeof record.styles !== "object") {
            return { diagnostic: invalid(`template.json has an invalid styles block`, { directory }) };
        }
        const styles = record.styles as { bodySize?: unknown; lineHeight?: unknown; headingScales?: unknown };
        if (styles.bodySize !== undefined) {
            if (typeof styles.bodySize !== "number" || styles.bodySize < 8 || styles.bodySize > 24) {
                return { diagnostic: invalid(`template.json bodySize must be between 8 and 24 points`, { directory }) };
            }
            geometry.bodySize = styles.bodySize;
        }
        if (styles.lineHeight !== undefined) {
            if (typeof styles.lineHeight !== "number" || styles.lineHeight < 1 || styles.lineHeight > 3) {
                return { diagnostic: invalid(`template.json lineHeight must be between 1 and 3`, { directory }) };
            }
            geometry.lineHeight = styles.lineHeight;
        }
        if (styles.headingScales !== undefined) {
            if (!Array.isArray(styles.headingScales) || styles.headingScales.length !== 4) {
                return { diagnostic: invalid(`template.json headingScales needs exactly four values`, { directory }) };
            }
            if (styles.headingScales.some((scale) => typeof scale !== "number" || scale < 0.8 || scale > 4)) {
                return { diagnostic: invalid(`template.json headingScales must be between 0.8 and 4`, { directory }) };
            }
            geometry.headingScales = styles.headingScales as number[];
        }
    }
    return { geometry };
}
