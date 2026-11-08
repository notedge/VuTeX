import { buildVuTeXCli } from "./cli.js";

export async function runCli(argv: string[]): Promise<number> {
    try {
        return await buildVuTeXCli().parse(argv);
    } catch (error) {
        // Font configuration failures carry a VUTEX_ code in their message,
        // for example VUTEX_FONT_ASSETS_FILE_MISSING with the expected file.
        console.error(error instanceof Error ? error.message : String(error));
        return 1;
    }
}
