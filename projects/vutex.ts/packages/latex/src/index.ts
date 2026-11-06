import { createVuTeX } from "@vutex/vutex";
import type { CreateVuTeXOptions, VuTeXRuntime } from "@vutex/vutex";

/**
 * Defaults the LaTeX-style distribution applies when a project picks nothing.
 *
 * `traditional` selects the Times-metrical theme, `xits-math` the Times-family
 * OpenType MATH face. They are runtime fallbacks only: neither is written into
 * `vutex.toml` or `vutex.lock`, and whichever theme or math face the manifest
 * or the render call names still wins.
 */
export const LATEX_DEFAULTS = {
    theme: "traditional",
    mathFont: "xits-math",
} as const;

/**
 * Create a VuTeX runtime with LaTeX-style typography defaults.
 *
 * The function is a thin facade over {@link createVuTeX}: it only fills the
 * fallback theme and math face, and forwards every other option unchanged.
 * Callers that pass their own `defaults` override the LaTeX values key by key.
 */
export async function createLaTeX(options: CreateVuTeXOptions = {}): Promise<VuTeXRuntime> {
    return createVuTeX({
        ...options,
        defaults: { ...LATEX_DEFAULTS, ...options.defaults },
    });
}

export * from "@vutex/vutex";
