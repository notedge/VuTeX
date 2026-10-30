import type { Cli, ParsedOptions } from "@vmz/commander";

import { createVuTeXRuntime } from "../context.js";
import { emitJson, presentDiagnostics } from "../diagnostics.js";
import { ExitCode } from "../exit-codes.js";

export function registerInspectCommand(cli: Cli): void {
    cli.command("inspect", "cli.cmd.inspect")
        .option("--json", "cli.opt.json")
        .action(async (options: ParsedOptions) => {
            const inputPath = options._[0];
            if (!inputPath || typeof inputPath !== "string") {
                return ExitCode.InvalidArgs;
            }

            const runtime = await createVuTeXRuntime();
            const result = await runtime.inspect(inputPath);

            if (options.json === true) {
                emitJson(result);
            } else {
                console.log(`${result.inputFormat}\t${result.path}`);
            }

            if (result.diagnostics.length > 0) {
                presentDiagnostics(result.diagnostics, options.json === true);
            }

            return result.diagnostics.some((item) => item.severity === "error")
                ? ExitCode.ActionFailed
                : ExitCode.Ok;
        });
}
