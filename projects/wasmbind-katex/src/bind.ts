import * as katex from "katex";

export function render_to_string(tex: string, options?): string {
    return katex.default.renderToString(tex, options);
}

