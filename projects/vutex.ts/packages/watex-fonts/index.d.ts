/** Two math faces the package ships, already read into bytes. */
export type MathFontAssets = {
    latinModern: Uint8Array;
    xits: Uint8Array;
};

/**
 * Read both math faces from the installed package. Node only: the browser
 * entry exposes no loader, so `loadWaTeX` falls back to its CDN base URL.
 */
export declare const loadMathFonts: (() => Promise<MathFontAssets>) | undefined;
