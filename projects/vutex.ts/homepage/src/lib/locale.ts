export function readLocale(): string {
    if (typeof window === "undefined") return "zh-hans";
    return window.location.pathname.startsWith("/en-us/") || window.location.pathname === "/en-us"
        ? "en-us"
        : "zh-hans";
}
export function localizedHref(path: string, locale: string): string {
    return locale === "en-us" ? (path === "/" ? "/en-us/" : `/en-us${path}`) : path;
}
const COPY: Record<string, string[]> = {
    overview: ["概览", "Overview"],
    gallery: ["公式画廊", "Gallery"],
    playground: ["试验台", "Playground"],
    install: ["安装", "Install"],
    docs: ["文档", "Documentation"],
    menu: ["打开导航", "Open navigation"],
    language: ["选择语言", "Select language"],

    heroTitleA: ["语义文档进，", "Semantic documents in,"],
    heroTitleB: ["可测量的页面出。", "measurable pages out."],
    heroStandfirst: [
        "VuTeX is a typesetting engine: semantic documents in, measurable pages out.",
        "VuTeX is a typesetting engine: semantic documents in, measurable pages out.",
    ],
    heroBody: [
        "页有版心，字有基线。输入是章节、引用、脚注与公式；输出是版心、基线网格、字号与行距的数值，每一页的版面都逐页可复核。",
        "A page has a measure, and every glyph sits on a baseline. Chapters, citations, footnotes and formulas go in; the measure, the baseline grid, and the numeric values of size and leading come out. Every page can be verified, page by page.",
    ],
    readDocs: ["阅读文档", "Read the documentation"],
    openPlayground: ["打开试验台", "Open the playground"],
    sealLabel: ["印刷印记", "Press mark"],

    pipelineOver: ["渲染管线", "The pipeline"],
    pipelineTitleA: ["一次输入，三种格式", "One input, three formats"],
    pipelineTitleB: ["与一组度量。", "and a set of measurements."],
    pipelineText: [
        "WaTeX 内核以 Rust 编写并编译为 WebAssembly，排版在浏览器本地完成，不上传任何内容。同一份 TeX 输入输出 SVG、HTML 或 MathML，并回报宽度、高度、深度与基线位置。",
        "The WaTeX kernel is written in Rust and compiled to WebAssembly; typesetting runs locally in the browser and nothing is uploaded. The same TeX input renders to SVG, HTML or MathML, and reports width, height, depth and baseline position.",
    ],
    pipelineCli: ["命令行", "Command line"],
    pipelineLive: ["实测", "Measured live"],
    pipelineLiveNote: [
        "由本页浏览器中的 WaTeX 实时渲染并测量。",
        "Rendered and measured live by WaTeX in this browser.",
    ],
    metricWidth: ["宽度", "Width"],
    metricHeight: ["高度", "Height"],
    metricDepth: ["深度", "Depth"],
    metricBaseline: ["基线", "Baseline"],

    typeOver: ["版式纪律", "Typography"],
    typeTitle: ["本页即样张。", "This page is the specimen."],
    typeText: [
        "本页按 VuTeX 的版式纪律排布。下列数值不是装饰，是产品要交付的输出形态。",
        "This page is set to the discipline VuTeX ships. The values below are not decoration; they are the shape of the output.",
    ],
    specMeasure: ["版心", "Measure"],
    specMeasureNote: ["行宽上限 34em，中文行宽不超过 38em。", "The measure caps at 34em; CJK lines never exceed 38em."],
    specBaseline: ["基线", "Baseline"],
    specBaselineNote: ["正文 16px，行高 1.75，基线网格 28px。", "Body text is 16px at 1.75 line height — a 28px baseline grid."],
    specHead: ["书眉", "Running head"],
    specHeadNote: ["12px small caps，小字号下限 12px。", "12px small caps; 12px is the floor for secondary text."],
    specHairline: ["栏线", "Hairline"],
    specHairlineNote: ["1px，墨色 10%–34%，层次只靠线、间距与字重。", "1px at 10–34% ink; hierarchy comes from rules, spacing and weight."],
    specSeal: ["印记", "Seal"],
    specSealNote: ["朱砂 #B5382A，每页至多一处。", "Cinnabar #B5382A, at most once per page."],
    compareEngines: ["查看三引擎对照", "Compare three engines"],

    installOver: ["安装", "Install"],
    installTitle: ["两个包，各有分工。", "Two packages, two jobs."],
    installWatex: ["公式排版", "Formula typesetting"],
    installWatexText: [
        "在 JavaScript 与 WASM 环境中渲染并测量数学公式。",
        "Render and measure mathematical expressions in JavaScript and WASM environments.",
    ],
    installRuntime: ["文档 runtime 与 CLI", "Document runtime and CLI"],
    installRuntimeText: [
        "官方命令行与 TypeScript facade，当前覆盖公式与文档的检查、渲染。",
        "The official CLI and TypeScript facade, currently covering formula and document inspection and rendering.",
    ],
    installRequirements: [
        "需要 Node.js 20+。文档 native 路径还需要当前平台对应的 binding 包。",
        "Requires Node.js 20+. Native document paths also require a binding package for your platform.",
    ],
    installDocsLink: ["阅读安装指南", "Read the installation guide"],

    footerTagline: ["语义文档进，可测量的页面出。", "Semantic documents in, measurable pages out."],
    site: ["站点", "Site"],
    project: ["项目", "Project"],
    resources: ["资源", "Resources"],
    fullscreen: ["全屏样张", "Fullscreen specimen"],
    colophonType: ["Set in Latin serif and CJK sans", "Set in Latin serif and CJK sans"],
    colophonMeasure: ["Measure 34em · Baseline 28px", "Measure 34em · Baseline 28px"],

    installEyebrow: ["开始使用", "Get started"],
    installPageTitle: ["把排版带入你的工作流。", "Typesetting, in your workflow."],
    installLead: [
        "选择独立公式门面，或安装 VuTeX runtime 与 CLI。",
        "Choose the standalone formula surface, or install the VuTeX runtime and CLI.",
    ],
};
export function copy(key: string, locale: string): string {
    const entry = COPY[key];
    return entry ? entry[locale === "en-us" ? 1 : 0] : key;
}
