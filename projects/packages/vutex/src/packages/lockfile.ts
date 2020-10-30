import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import TOML from "@iarna/toml";

export type LockedPackage = {
    name: string;
    version: string;
    source: string;
    contentHash: string;
    dependencies: string[];
};

export type VuTeXLockfile = {
    version: number;
    packages: LockedPackage[];
};

export async function readLockfile(projectRoot: string): Promise<VuTeXLockfile | null> {
    const path = join(projectRoot, "vutex.lock");
    try {
        const raw = await readFile(path, "utf8");
        return TOML.parse(raw) as VuTeXLockfile;
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") {
            return null;
        }
        throw error;
    }
}

export async function writeLockfile(projectRoot: string, lockfile: VuTeXLockfile): Promise<void> {
    const path = join(projectRoot, "vutex.lock");
    await writeFile(path, `${TOML.stringify(lockfile)}\n`, "utf8");
}
