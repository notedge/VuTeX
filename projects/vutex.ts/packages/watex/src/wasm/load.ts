import type {
    FormulaRender,
    RenderOptions,
    WaTeXBindings,
    WaTeXFontAssets,
    WaTeXWasmOptions,
} from "../types.js";
import fontsPackage from "@vutex/watex-fonts/package.json" with { type: "json" };

type WasmBinding = {
    default: (input?: { module_or_path?: string | URL | Uint8Array }) => Promise<unknown>;
    watexVersion: () => string;
    installMathFonts: (latinModern: Uint8Array, xits: Uint8Array) => void;
    measureFormula: (source: string, optionsJson?: string) => string;
    renderSvg: (source: string, optionsJson?: string) => string;
    renderHtml: (source: string, optionsJson?: string) => string;
    renderMathml: (source: string, optionsJson?: string) => string;
    renderFormula: (source: string, optionsJson?: string) => string;
};

/** CDN directory carrying the two math faces at the installed exact version. */
const DEFAULT_FONT_BASE_URL = `https://cdn.jsdelivr.net/npm/@vutex/watex-fonts@${fontsPackage.version}/assets/fonts/`;

/** One resolved payload per base URL, so a repeated load reuses the fetch. */
const fontAssets = new Map<string, Promise<WaTeXFontAssets>>();

function encodeOptions(options?: RenderOptions): string | undefined {
    if (!options) {
        return undefined;
    }
    return JSON.stringify(options);
}

function decodeResult(raw: string): FormulaRender {
    return JSON.parse(raw) as FormulaRender;
}

function faceUrl(base: string | URL, name: string): URL {
    const href = typeof base === "string" ? base : base.href;
    return new URL(name, href.endsWith("/") ? href : `${href}/`);
}

function fetchFailure(url: URL, detail: string): Error {
    const error = new Error(`VUTEX_FONT_ASSETS_FETCH_FAILED: cannot read ${url.href}: ${detail}`);
    error.name = "VUTEX_FONT_ASSETS_FETCH_FAILED";
    return error;
}

async function fetchFace(url: URL): Promise<Uint8Array> {
    let response: Response;
    try {
        response = await fetch(url);
    } catch (error) {
        throw fetchFailure(url, String(error));
    }
    if (!response.ok) {
        throw fetchFailure(url, `HTTP ${response.status}`);
    }
    return new Uint8Array(await response.arrayBuffer());
}

async function fetchAssets(base: string | URL): Promise<WaTeXFontAssets> {
    const key = typeof base === "string" ? base : base.href;
    const cached = fontAssets.get(key);
    if (cached) {
        return cached;
    }
    const pending = Promise.all([
        fetchFace(faceUrl(base, "LatinModernMath-Regular.otf")),
        fetchFace(faceUrl(base, "XITSMath-Regular.otf")),
    ]).then(([latinModern, xits]) => ({ latinModern, xits }));
    fontAssets.set(key, pending);
    try {
        return await pending;
    } catch (error) {
        fontAssets.delete(key);
        throw error;
    }
}

/**
 * Resolve the math faces in host priority order: explicit bytes, an explicit
 * base URL, the installed package on Node, and the pinned CDN URL otherwise.
 */
async function resolveAssets(options: WaTeXWasmOptions): Promise<WaTeXFontAssets> {
    if (options.assets) {
        return options.assets;
    }
    if (options.assetBaseUrl) {
        return fetchAssets(options.assetBaseUrl);
    }
    const local = await import("@vutex/watex-fonts");
    if (typeof local.loadMathFonts === "function") {
        return local.loadMathFonts();
    }
    return fetchAssets(DEFAULT_FONT_BASE_URL);
}

function wrap(binding: WasmBinding): WaTeXBindings {
    return {
        watexVersion: () => binding.watexVersion(),
        measureFormula: (source, options) =>
            decodeResult(binding.measureFormula(source, encodeOptions(options))),
        renderSvg: (source, options) =>
            decodeResult(binding.renderSvg(source, encodeOptions(options))),
        renderHtml: (source, options) =>
            decodeResult(binding.renderHtml(source, encodeOptions(options))),
        renderMathml: (source, options) =>
            decodeResult(binding.renderMathml(source, encodeOptions(options))),
        renderFormula: (source, options) =>
            decodeResult(binding.renderFormula(source, encodeOptions(options))),
    };
}

/** Load the WASM artifact from `@vutex/vutex-unknown-wasm32` and install its fonts. */
export async function loadWaTeX(options: WaTeXWasmOptions = {}): Promise<WaTeXBindings> {
    const binding = (await import("@vutex/vutex-unknown-wasm32")) as WasmBinding;
    if (options.bytes) {
        await binding.default({ module_or_path: options.bytes });
    } else if (options.url) {
        await binding.default({ module_or_path: options.url });
    } else {
        await binding.default();
    }
    const assets = await resolveAssets(options);
    binding.installMathFonts(assets.latinModern, assets.xits);
    return wrap(binding);
}
