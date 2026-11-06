import type { Cli, ParsedOptions } from "@vmz/commander";

import { createVuTeXRuntime } from "../context.js";
import { emitHuman, emitJson, presentDiagnostics } from "../diagnostics.js";
import { ExitCode } from "../exit-codes.js";
import { PackageResolveError } from "../../packages/manager.js";

function readName(options: ParsedOptions): string | undefined {
    const value = options._[0] ?? options.name;
    return typeof value === "string" ? value : undefined;
}

function readSlot(options: ParsedOptions): string | undefined {
    return typeof options.slot === "string" ? options.slot : undefined;
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
                    emitHuman(`${item.slot}\t${item.name}\t${item.version}\t${item.source}`);
                }
            }
            return ExitCode.Ok;
        });

    group
        .command("add", "cli.cmd.package.add")
        .option("--slot <slot>", "cli.opt.package-slot")
        .action(async (options: ParsedOptions) => {
            const name = readName(options);
            if (!name) {
                return ExitCode.InvalidArgs;
            }
            const runtime = await createVuTeXRuntime();
            try {
                await runtime.packages.add(name, { slot: readSlot(options) });
            } catch (error) {
                if (error instanceof PackageResolveError) {
                    presentDiagnostics(error.diagnostics, false);
                    return ExitCode.ActionFailed;
                }
                throw error;
            }
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
        .option("--slot <slot>", "cli.opt.package-slot")
        .action(async (options: ParsedOptions) => {
            const runtime = await createVuTeXRuntime();
            try {
                await runtime.packages.install({
                    name: typeof options.name === "string" ? options.name : undefined,
                    slot: readSlot(options),
                });
            } catch (error) {
                if (error instanceof PackageResolveError) {
                    presentDiagnostics(error.diagnostics, false);
                    return ExitCode.ActionFailed;
                }
                throw error;
            }
            return ExitCode.Ok;
        });

    group
        .command("update", "cli.cmd.package.update")
        .action(async () => {
            const runtime = await createVuTeXRuntime();
            try {
                await runtime.packages.update();
            } catch (error) {
                if (error instanceof PackageResolveError) {
                    presentDiagnostics(error.diagnostics, false);
                    return ExitCode.ActionFailed;
                }
                throw error;
            }
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
                    const code = check.code ? ` ${check.code}` : "";
                    emitHuman(`  ${check.ok ? "ok" : "fail"}  ${check.id}${code}: ${check.message}`);
                }
            }
            return report.ok ? ExitCode.Ok : ExitCode.ActionFailed;
        });
}
