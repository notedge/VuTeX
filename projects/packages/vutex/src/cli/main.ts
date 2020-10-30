import { buildVuTeXCli } from "./cli.js";

export async function runCli(argv: string[]): Promise<number> {
    return await buildVuTeXCli().parse(argv);
}
