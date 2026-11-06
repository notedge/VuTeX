import { createVuTeX } from "@vutex/vutex";
import type { CreateVuTeXOptions, VuTeXRuntime } from "@vutex/vutex";

/**
 * Defaults the neutral TeX-style distribution applies when a project picks nothing.
 *
 * `baseline` is VuTeX's neutral theme and `latin-modern-math` is its
 * Computer-Modern-metrical OpenType MATH face, so the pair names the engine's
 * own TeX lineage rather than a Times-style house style. They are runtime
 * fallbacks only: neither is written into `vutex.toml` or `vutex.lock`, and
 * whichever theme or math face the manifest or the render call names still wins.
 */
export const TEX_DEFAULTS = {
    theme: "baseline",
    mathFont: "latin-modern-math",
} as const;

/**
 * Create a VuTeX runtime with neutral TeX-style typography defaults.
 *
 * The function is a thin facade over {@link createVuTeX}: it only fills the
 * fallback theme and math face, and forwards every other option unchanged.
 * Callers that pass their own `defaults` override the TeX values key by key.
 */
export async function createTeX(options: CreateVuTeXOptions = {}): Promise<VuTeXRuntime> {
    return createVuTeX({
        ...options,
        defaults: { ...TEX_DEFAULTS, ...options.defaults },
    });
}

export * from "@vutex/vutex";
