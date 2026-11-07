import { createVuTeX } from "@vutex/vutex";
import type { CreateVuTeXOptions, VuTeXRuntime } from "@vutex/vutex";

/**
 * Defaults the Typst-style input distribution applies when a project picks
 * nothing.
 *
 * The distribution owns the input language, not a typographic house style:
 * files without a recognised extension still reach the Typst adapter. Theme
 * and math face stay at the core defaults, so a manifest or render call picks
 * them exactly as it does without this package.
 */
export const TYPST_DEFAULTS = {
    input: "typst",
} as const;

/**
 * Create a VuTeX runtime that reads Typst input by default.
 *
 * The function is a thin facade over {@link createVuTeX}: it only fills the
 * fallback input format, and forwards every other option unchanged. Callers
 * that pass their own `defaults` override the Typst value key by key.
 */
export async function createTypst(options: CreateVuTeXOptions = {}): Promise<VuTeXRuntime> {
    return createVuTeX({
        ...options,
        defaults: { ...TYPST_DEFAULTS, ...options.defaults },
    });
}

export * from "@vutex/vutex";
