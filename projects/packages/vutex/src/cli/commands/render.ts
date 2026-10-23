import { writeFile } from "node:fs/promises";
import { basename, dirname, extname, join, resolve } from "node:path";

import type { Cli, ParsedOptions } from "@vmz/commander";

import type { RenderDocumentResult } from "../../types.js";
import { createVuTeXRuntime } from "../context.js";
import { emitJson, presentDiagnostics } from "../diagnostics.js";
import { ExitCode } from "../exit-codes.js";

/**
 * Write PDF bytes beside the input file and build the CLI payload.
 *
 * The text mode prints `outputPath`; the JSON payload carries the render
 * result without the binary body plus the absolute `outputPath`.
 */
export async function writePdfOutput(
    inputPath: string,
    bytes: Uint8Array,
    result: RenderDocumentResult,
): Promise<{ outputPath: string; payload: Record<string, unknown> }> {
    const source = resolve(inputPath);
    const outputPath = join(dirname(source), `${basename(source, extname(source))}.pdf`);
    await writeFile(outputPath, bytes);
    const { pdf: _bytes, ...rest } = result;
    return { outputPath, payload: { ...rest, outputPath } };
}

export function registerRenderCommand(cli: Cli): void {
    cli.command("render", "cli.cmd.render")
        .option("-o, --output <format>", "cli.opt.output")
        .option("--profile <name>", "cli.opt.profile")
        .option("--input <format>", "cli.opt.input-format")
        .option("--json", "cli.opt.json")
        .action(async (options: ParsedOptions) => {
            const inputPath = options._[0];
            if (!inputPath || typeof inputPath !== "string") {
                return ExitCode.InvalidArgs;
            }

            const runtime = await createVuTeXRuntime();
            const result = await runtime.render(inputPath, {
                output: typeof options.output === "string" ? options.output : undefined,
                profile: typeof options.profile === "string" ? options.profile : undefined,
                input: typeof options.input === "string" ? options.input : undefined,
            });

            const pdf = result.outputFormat === "pdf" ? result.pdf : undefined;
            const written = pdf ? await writePdfOutput(inputPath, pdf, result) : null;

            if (options.json === true) {
                emitJson(written ? written.payload : result);
            } else if (written) {
                console.log(written.outputPath);
            } else {
                const body = result.outputFormat === "svg"
                    ? result.svg ?? result.html
                    : result.html ?? result.svg;
                console.log(body ?? result.reportJson);
            }

            if (result.diagnostics.length > 0) {
                presentDiagnostics(result.diagnostics, options.json === true);
            }

            return result.exitCode === 0 ? ExitCode.Ok : ExitCode.ActionFailed;
        });
}
