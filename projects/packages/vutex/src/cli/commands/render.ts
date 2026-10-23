import type { Cli, ParsedOptions } from "@vmz/commander";

import { createVuTeXRuntime } from "../context.js";
import { emitJson, presentDiagnostics } from "../diagnostics.js";
import { ExitCode } from "../exit-codes.js";

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

            if (options.json === true) {
                emitJson(result);
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
