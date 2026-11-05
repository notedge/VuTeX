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

function canonicalPackage(item: LockedPackage): LockedPackage {
    return {
        name: item.name,
        version: item.version,
        source: item.source,
        contentHash: item.contentHash,
        dependencies: [...item.dependencies].sort(),
    };
}

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

/**
 * Write the lockfile in canonical order.
 *
 * Package entries are sorted by name and every field keeps a fixed position so
 * that installing the same manifest twice produces byte-identical output.
 */
export async function writeLockfile(projectRoot: string, lockfile: VuTeXLockfile): Promise<void> {
    const path = join(projectRoot, "vutex.lock");
    const packages = lockfile.packages
        .map(canonicalPackage)
        .sort((left, right) => left.name.localeCompare(right.name, "en"));
    const canonical: VuTeXLockfile = { version: lockfile.version, packages };
    await writeFile(path, `${TOML.stringify(canonical)}\n`, "utf8");
}
