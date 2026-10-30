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
    overview: ["概览", "Overview"], gallery: ["公式画廊", "Gallery"], playground: ["试验台", "Playground"],
    menu: ["打开导航", "Open navigation"], language: ["选择语言", "Select language"],
    eyebrow: ["为表达，赋予形态", "A space for beautiful expression"],
    intro: ["让每一个想法，都有恰如其分的呈现。", "Give every idea the form it deserves."],
    description: ["从一个符号，到一页思考。VuTeX 探索文字、公式与留白之间的秩序。", "From a single symbol to a page of thought. Explore the quiet balance of type, mathematics and space."],
    explore: ["探索排版", "Explore the gallery"], try: ["写下你的公式", "Try your own formula"],
    live: ["实时呈现", "Rendered live"], loading: ["正在准备字形…", "Preparing the glyphs…"],
    error: ["样张暂时无法加载，请在试验台重试。", "The specimen could not load. Try the playground."],
    select: ["选择公式样张", "Choose a specimen"], euler: ["欧拉恒等式", "Euler’s identity"],
    gauss: ["高斯积分", "Gaussian integral"], quadratic: ["求根公式", "Quadratic formula"],
    scroll: ["向下，发现细节", "Scroll to discover"],
    collection: ["细节之中，自有秩序", "A study in the details"],
    collectionText: ["字形的弧度、分式的间距、每一条安静的基线。让内容自然地被看见。", "The curve of a glyph. The space around a fraction. A quietly perfect baseline. Room for the idea itself."],
    detail1: ["字形，有自己的语言", "Every glyph has a voice"], detail1Text: ["曲线与笔画，保留数学独有的韵律。", "Curves and strokes, with the rhythm of mathematics intact."],
    detail2: ["留白，也是表达", "Space is part of the story"], detail2Text: ["上下、远近，让复杂的关系清晰可读。", "Above, below, between. Complex relationships, clearly expressed."],
    detail3: ["一份想法，多种呈现", "One idea, many forms"], detail3Text: ["以 SVG、HTML 或 MathML，抵达不同的阅读场景。", "SVG, HTML or MathML. A considered form for wherever your work is read."],
    closing: ["下一页，属于你的想法。", "The next page is yours."], closingText: ["在浏览器里，感受 WeTeX 的公式排版。", "Find your flow with WeTeX, right in your browser."],
    footer: ["文字、公式与留白之间。", "Between type, mathematics and space."], site: ["探索", "Explore"], project: ["项目", "Project"],
    fullscreen: ["全屏样张", "Fullscreen specimen"], colophon: ["用心呈现，每一个细节。", "Considered, down to the last detail."],
    install: ["安装", "Install"], docs: ["文档", "Documentation"],
    installEyebrow: ["开始使用", "GET STARTED"],
    installTitle: ["把排版带入你的工作流。", "Typesetting, in your workflow."],
    installLead: ["选择独立公式门面，或安装 VuTeX runtime 与 CLI。", "Choose the standalone formula surface, or install the VuTeX runtime and CLI."],
    installWetex: ["公式排版", "Formula typesetting"],
    installWetexText: ["在 JavaScript 与 WASM 环境中渲染并测量数学公式。", "Render and measure mathematical expressions in JavaScript and WASM environments."],
    installRuntime: ["文档 runtime 与 CLI", "Document runtime and CLI"],
    installRuntimeText: ["安装官方命令行与 TypeScript facade。当前功能以公式和文档检查、渲染为主。", "Install the official CLI and TypeScript facade. Current capabilities center on formula and document inspection and rendering."],
    installRequirements: ["需要 Node.js 20+。文档 native 路径还需要当前平台对应的 binding 包。", "Requires Node.js 20+. Native document paths also require a binding package for your platform."],
    installDocsLink: ["阅读安装指南", "Read the installation guide"],
};
export function copy(key: string, locale: string): string {
    const entry = COPY[key];
    return entry ? entry[locale === "en-us" ? 1 : 0] : key;
}
