import { render as wasmRender, Options } from '../lib/rutex_wit.js';

export interface KaTeXOptions extends Partial<Options> {}

const defaultOptions: Options = {
    displayMode: false,
    leqno: false,
    fleqn: false,
    throwOnError: false,
    minRuleThickness: 0.04,
    maxSize: Infinity,
    maxExpand: 1000,
    strict: false,
    trust: false,
    globalGroup: false,
    output: 'html',
    errorColor: '#cc0000',
};

/**
 * Renders a TeX expression to an HTML string using the RuTeX WASM component.
 * 
 * @param expr The TeX expression to render.
 * @param options KaTeX options.
 * @returns The rendered HTML string.
 */
export function render(expr: string, options: KaTeXOptions = {}): string {
    const finalOptions: Options = {
        ...defaultOptions,
        ...options,
    };
    return wasmRender(expr, finalOptions);
}

/**
 * A convenient class for rendering TeX with persistent options.
 */
export class RuTeX {
    private options: Options;

    constructor(options: KaTeXOptions = {}) {
        this.options = { ...defaultOptions, ...options };
    }

    /**
     * Renders a TeX expression.
     */
    render(expr: string, overrideOptions: KaTeXOptions = {}): string {
        return wasmRender(expr, { ...this.options, ...overrideOptions });
    }

    /**
     * Set whether to use display mode.
     */
    setDisplayMode(enabled: boolean): this {
        this.options.displayMode = enabled;
        return this;
    }

    /**
     * Set the output format.
     */
    setOutputFormat(format: 'html' | 'mathml' | 'htmlAndMathml'): this {
        this.options.output = format;
        return this;
    }
}
