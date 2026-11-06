import { readFile } from "node:fs/promises";
import { join } from "node:path";

import TOML from "@iarna/toml";

import type { PackageSlot } from "../types.js";

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

/** Slots are the keys of `[packages]`, the values are package names. */
export const PACKAGE_SLOTS: PackageSlot[] = [
    "font",
    "math-font",
    "template",
    "theme",
    "profile",
    "macros",
    "resources",
    "fixture",
];

export function isPackageSlot(value: string): value is PackageSlot {
    return (PACKAGE_SLOTS as string[]).includes(value);
}

const SLOT_PREFIXES: Array<[string[], PackageSlot]> = [
    [["math-font-"], "math-font"],
    [["font-"], "font"],
    [["template-"], "template"],
    [["theme-"], "theme"],
    [["profile-"], "profile"],
    [["macros-", "macro-"], "macros"],
    [["resources-", "resource-"], "resources"],
    [["fixture-"], "fixture"],
];

/** Infer the slot from a package name such as `@vutex/font-libertinus`. */
export function inferSlot(packageName: string): PackageSlot | null {
    const bare = packageName.startsWith("@vutex/")
        ? packageName.slice("@vutex/".length)
        : packageName;
    for (const [prefixes, slot] of SLOT_PREFIXES) {
        if (prefixes.some((prefix) => bare.startsWith(prefix))) {
            return slot;
        }
    }
    return null;
}

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
