import { readFile } from "node:fs/promises";
import { join } from "node:path";

import type { TemplateColorToken, TemplateGeometry, TemplateSectionChrome, VuTeXDiagnostic } from "../types.js";

const PAGE_SIZES: Record<string, { width: number; height: number }> = {
    a4: { width: 595.276, height: 841.89 },
    letter: { width: 612, height: 792 },
};

const COLOR_TOKENS: readonly TemplateColorToken[] = [
    "ink",
    "inkSecondary",
    "inkMuted",
    "inkFaint",
    "accent",
    "ruleStrong",
    "rule",
    "ruleFaint",
    "tint",
    "header",
    "folio",
    "headingRule",
];

function invalid(message: string, details: Record<string, unknown>): VuTeXDiagnostic {
    return {
        code: "VUTEX_PACKAGE_TEMPLATE_INVALID",
        severity: "error",
        message,
        stage: "package",
        args: details,
    };
}

function unsupported(message: string, details: Record<string, unknown>): VuTeXDiagnostic {
    return {
        code: "VUTEX_PACKAGE_TEMPLATE_UNSUPPORTED",
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

/** Trimmed running-header text, or `null` when it fails the 1–160 contract. */
function headerText(value: unknown): string | null {
    if (typeof value !== "string") {
        return null;
    }
    const text = value.trim();
    return text.length === 0 || text.length > 160 ? null : text;
}

/** A `#RRGGBB` colour token, or `null` when the value is malformed. */
function colorText(value: unknown): string | null {
    return typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value) ? value : null;
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
            const text = headerText(chrome.header);
            if (text === null) {
                return { diagnostic: invalid(`template.json has an invalid chrome header`, { directory }) };
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
        if (chrome.sections !== undefined) {
            if (chrome.sections === null || typeof chrome.sections !== "object" || Array.isArray(chrome.sections)) {
                return { diagnostic: invalid(`template.json has an invalid chrome sections block`, { directory }) };
            }
            const entries = chrome.sections as Record<string, unknown>;
            const sections: TemplateSectionChrome[] = [];
            for (const [key, value] of Object.entries(entries)) {
                if (!/^[1-4]$/.test(key)) {
                    return {
                        diagnostic: invalid(`template.json chrome section level is outside H1 to H4`, {
                            directory,
                            level: key,
                        }),
                    };
                }
                if (value === null || typeof value !== "object" || Array.isArray(value)) {
                    return { diagnostic: invalid(`template.json chrome section ${key} is not an object`, { directory }) };
                }
                const entry = value as { header?: unknown; folio?: unknown };
                if (entry.header === undefined && entry.folio === undefined) {
                    return {
                        diagnostic: invalid(`template.json chrome section ${key} needs a header or a folio`, {
                            directory,
                            level: key,
                        }),
                    };
                }
                const section: TemplateSectionChrome = { level: Number(key) };
                if (entry.header !== undefined) {
                    const text = headerText(entry.header);
                    if (text === null) {
                        return {
                            diagnostic: invalid(`template.json chrome section ${key} has an invalid header`, {
                                directory,
                                level: key,
                            }),
                        };
                    }
                    section.header = text;
                }
                if (entry.folio !== undefined) {
                    if (typeof entry.folio !== "boolean") {
                        return {
                            diagnostic: invalid(`template.json chrome section ${key} has an invalid folio flag`, {
                                directory,
                                level: key,
                            }),
                        };
                    }
                    section.folio = entry.folio;
                }
                sections.push(section);
            }
            sections.sort((left, right) => left.level - right.level);
            geometry.chromeSections = sections;
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
        const styles = record.styles as {
            bodySize?: unknown;
            lineHeight?: unknown;
            headingScales?: unknown;
            paragraphGap?: unknown;
            paragraphIndent?: unknown;
            colors?: unknown;
            fontFamily?: unknown;
            fontFamilies?: unknown;
        };
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
        for (const [field, name] of [
            ["paragraphGap", "paragraphGap"],
            ["paragraphIndent", "paragraphIndent"],
        ] as const) {
            const value = styles[field];
            if (value === undefined) {
                continue;
            }
            if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 48) {
                return { diagnostic: invalid(`template.json ${name} must be between 0 and 48 points`, { directory }) };
            }
            geometry[field] = value;
        }
        if (styles.colors !== undefined) {
            if (styles.colors === null || typeof styles.colors !== "object" || Array.isArray(styles.colors)) {
                return { diagnostic: invalid(`template.json has an invalid colors block`, { directory }) };
            }
            const colors: Partial<Record<TemplateColorToken, string>> = {};
            for (const [token, value] of Object.entries(styles.colors as Record<string, unknown>)) {
                if (!COLOR_TOKENS.includes(token as TemplateColorToken)) {
                    return { diagnostic: invalid(`template.json colors names an unknown token: ${token}`, { directory, token }) };
                }
                const color = colorText(value);
                if (color === null) {
                    return { diagnostic: invalid(`template.json color ${token} must be #RRGGBB`, { directory, token }) };
                }
                colors[token as TemplateColorToken] = color;
            }
            geometry.colors = colors;
        }
        if (styles.fontFamily !== undefined || styles.fontFamilies !== undefined) {
            return {
                diagnostic: unsupported(
                    `template.json declares a text font family but the bundled layout only has serif and monospace faces`,
                    { directory },
                ),
            };
        }
    }
    return { geometry };
}
