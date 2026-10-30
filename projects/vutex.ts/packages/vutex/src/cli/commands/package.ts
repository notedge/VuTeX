import type { Cli, ParsedOptions } from "@vmz/commander";

import { createVuTeXRuntime } from "../context.js";
import { emitHuman, emitJson } from "../diagnostics.js";
import { ExitCode } from "../exit-codes.js";

function readName(options: ParsedOptions): string | undefined {
    const value = options._[0] ?? options.name;
    return typeof value === "string" ? value : undefined;
}

export function registerPackageCommands(cli: Cli): void {
    const group = cli.command("package", "cli.cmd.package");

    group
        .command("list", "cli.cmd.package.list")
        .option("--json", "cli.opt.json")
        .action(async (options: ParsedOptions) => {
            const runtime = await createVuTeXRuntime();
            const items = await runtime.packages.list();
            if (options.json === true) {
                emitJson({ packages: items });
            } else {
                for (const item of items) {
                    emitHuman(`${item.name}\t${item.version}\t${item.source}`);
                }
            }
            return ExitCode.Ok;
        });

    group
        .command("add", "cli.cmd.package.add")
        .action(async (options: ParsedOptions) => {
            const name = readName(options);
            if (!name) {
                return ExitCode.InvalidArgs;
            }
            const runtime = await createVuTeXRuntime();
            await runtime.packages.add(name, {
                version: typeof options.version === "string" ? options.version : undefined,
            });
            return ExitCode.Ok;
        });

    group
        .command("remove", "cli.cmd.package.remove")
        .action(async (options: ParsedOptions) => {
            const name = readName(options);
            if (!name) {
                return ExitCode.InvalidArgs;
            }
            const runtime = await createVuTeXRuntime();
            await runtime.packages.remove(name);
            return ExitCode.Ok;
        });

    group
        .command("install", "cli.cmd.package.install")
        .option("--name <name>", "cli.opt.package-name")
        .option("--version <version>", "cli.opt.package-version")
        .action(async (options: ParsedOptions) => {
            const runtime = await createVuTeXRuntime();
            await runtime.packages.install({
                name: typeof options.name === "string" ? options.name : undefined,
                version: typeof options.version === "string" ? options.version : undefined,
            });
            return ExitCode.Ok;
        });

    group
        .command("update", "cli.cmd.package.update")
        .action(async () => {
            const runtime = await createVuTeXRuntime();
            await runtime.packages.update();
            return ExitCode.Ok;
        });

    group
        .command("doctor", "cli.cmd.package.doctor")
        .option("--json", "cli.opt.json")
        .action(async (options: ParsedOptions) => {
            const runtime = await createVuTeXRuntime();
            const report = await runtime.packages.doctor();
            if (options.json === true) {
                emitJson(report);
            } else if (report.ok) {
                emitHuman("VuTeX resource packages look healthy");
                for (const check of report.checks) {
                    emitHuman(`  ok  ${check.id}: ${check.message}`);
                }
            } else {
                emitHuman("VuTeX resource packages have issues");
                for (const check of report.checks) {
                    emitHuman(`  ${check.ok ? "ok" : "fail"}  ${check.id}: ${check.message}`);
                }
            }
            return report.ok ? ExitCode.Ok : ExitCode.ActionFailed;
        });
}
