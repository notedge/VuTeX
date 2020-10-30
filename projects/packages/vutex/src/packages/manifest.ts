import { readFile } from "node:fs/promises";
import { join } from "node:path";

import TOML from "@iarna/toml";

export type VuTeXManifest = {
    document?: {
        input?: string;
        output?: string;
    };
    profile?: {
        name?: string;
    };
    packages?: Record<string, string>;
};

export async function readManifest(projectRoot: string): Promise<VuTeXManifest | null> {
    const path = join(projectRoot, "vutex.toml");
    try {
        const raw = await readFile(path, "utf8");
        return TOML.parse(raw) as VuTeXManifest;
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") {
            return null;
        }
        throw error;
    }
}
