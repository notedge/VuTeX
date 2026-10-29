/**
 * Engine adapters. Every engine is a real package dependency resolved by the
 * VMZ build: bare specifiers are packed into `dist/vendor/` and rewritten to
 * relative imports, so nothing here may point at hand-copied files.
 */

export type EngineId = "wetex" | "katex" | "mathjax";
export type Backend = "svg" | "mathml" | "html";

export interface BackendMeta {
    id: Backend;
    label: string;
}

export interface EngineMeta {
    id: EngineId;
    name: string;
    vendor: string;
    detail: string;
    backends: Backend[];
}

export interface EngineOutput {
    engine: EngineId;
    backend: Backend;
    ok: boolean;
    markup: string;
    ms: number;
    note: string;
}

export const BACKENDS: BackendMeta[] = [
    { id: "mathml", label: "MathML" },
    { id: "svg", label: "SVG" },
    { id: "html", label: "HTML" },
];

export const ENGINES: EngineMeta[] = [
    {
        id: "wetex",
        name: "WeTeX",
        vendor: "VuTeX",
        detail: "Rust/WASM 内核，直接产出 SVG 与 MathML，并回报宽度、高度与基线深度。",
        backends: ["svg", "mathml"],
    },
    {
        id: "katex",
        name: "KaTeX",
        vendor: "0.18",
        detail: "HTML 与 MathML 双输出，字形与间距由 KaTeX 自带样式表与字体提供。",
        backends: ["html", "mathml"],
    },
    {
        id: "mathjax",
        name: "MathJax",
        vendor: "4.1",
        detail: "完整 TeX 语义，输出 SVG 或 MathML，覆盖扩展语法最广。",
        backends: ["svg", "mathml"],
    },
];

export function engineMeta(id: EngineId): EngineMeta {
    return ENGINES.find((engine) => engine.id === id) || ENGINES[0];
}

export function supportsBackend(id: EngineId, backend: Backend): boolean {
    return engineMeta(id).backends.includes(backend);
}

let wetexLoad: any = null;
let katexLoad: any = null;
let mathjaxLoad: any = null;

function displayOptions(display: boolean) {
    return { displayMode: display ? "block" : "inline" };
}

async function loadWetex(): Promise<any> {
    if (!wetexLoad) {
        wetexLoad = (async () => {
            const { loadWeTeX } = await import("@vutex/wetex");
            return await loadWeTeX();
        })();
    }
    return wetexLoad;
}

async function loadKatex(): Promise<any> {
    if (!katexLoad) {
        katexLoad = (async () => {
            const loaded = await Promise.all([
                import("katex"),
                import("katex/dist/katex.min.css"),
            ]);
            await loaded[1].default;
            return loaded[0].default;
        })();
    }
    return katexLoad;
}

async function loadMathjax(): Promise<any> {
    if (!mathjaxLoad) {
        mathjaxLoad = (async () => {
            const scope: any = globalThis;
            scope.MathJax = {
                startup: { typeset: false },
                svg: { fontCache: "local" },
            };
            await import("@mathjax/src/bundle/tex-svg.js");
            const api = scope.MathJax;
            if (!api) throw new Error("MathJax bundle did not expose a global");
            if (api.startup && api.startup.promise) await api.startup.promise;
            return api;
        })();
    }
    return mathjaxLoad;
}

export async function preloadEngines(ids: EngineId[]): Promise<void> {
    await Promise.all(
        ids.map((id) => {
            if (id === "wetex") return loadWetex();
            if (id === "katex") return loadKatex();
            return loadMathjax();
        }),
    );
}

function wetexNote(render: any): string {
    const diagnostics = render.diagnostics || [];
    const errors = diagnostics.filter((item: any) => item.severity === "error");
    if (errors.length > 0) return errors[0].message;
    return "";
}

export async function renderFormula(
    engine: EngineId,
    backend: Backend,
    tex: string,
    display = true,
): Promise<EngineOutput> {
    const clock = () => (typeof performance === "undefined" ? Date.now() : performance.now());
    const started = clock();
    const done = (ok: boolean, markup: string, note = ""): EngineOutput => ({
        engine,
        backend,
        ok,
        markup,
        ms: Math.round(clock() - started),
        note,
    });

    if (!supportsBackend(engine, backend)) {
        return done(false, "", `${engineMeta(engine).name} 不产出此格式`);
    }

    try {
        if (engine === "wetex") {
            const wetex = await loadWetex();
            const render =
                backend === "svg"
                    ? wetex.renderSvg(tex, displayOptions(display))
                    : wetex.renderMathml(tex, displayOptions(display));
            const markup = backend === "svg" ? render.svg : render.mathml || render.svg;
            return done(String(markup || "").trim().length > 0, markup || "", wetexNote(render));
        }
        if (engine === "katex") {
            const katex = await loadKatex();
            const markup = katex.renderToString(tex, {
                displayMode: display,
                throwOnError: false,
                strict: false,
                output: backend === "mathml" ? "mathml" : "html",
            });
            return done(true, markup);
        }
        const mathjax = await loadMathjax();
        const markup =
            backend === "mathml"
                ? mathjax.tex2mml(tex, { display })
                : mathjax.tex2svg(tex, { display }).outerHTML;
        return done(true, markup);
    } catch (error) {
        return done(false, "", describeError(error));
    }
}

function describeError(error: unknown): string {
    if (error instanceof Error) return error.message;
    return String(error);
}
