import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { createCli } from "@vmz/commander";

import { registerInspectCommand } from "./commands/inspect.js";
import { registerPackageCommands } from "./commands/package.js";
import { registerRenderCommand } from "./commands/render.js";

export function buildVuTeXCli() {
    const localesRoot = join(dirname(fileURLToPath(import.meta.url)), "../../locales");
    const cli = createCli("vutex")
        .locales(localesRoot, { envKeys: ["VUTEX_LOCALE", "LOCALE", "LANG", "LC_ALL"] })
        .intro("cli.intro");

    registerRenderCommand(cli);
    registerInspectCommand(cli);
    registerPackageCommands(cli);

    return cli;
}
