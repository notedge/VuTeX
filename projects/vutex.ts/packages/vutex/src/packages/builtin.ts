import { createHash } from "node:crypto";

import type { PackageSlot } from "../types.js";

export type BuiltinFile = {
    /** Path relative to `projects/vutex.rs/fonts`. */
    path: string;
    digest: string;
};

export type BuiltinPackage = {
    id: string;
    slot: PackageSlot;
    version: string;
    files: BuiltinFile[];
    artifactHash?: string;
};

function treeDigest(files: BuiltinFile[]): string | undefined {
    if (files.length === 0) {
        return undefined;
    }
    const lines = [...files]
        .sort((left, right) => left.path.localeCompare(right.path, "en"))
        .map((file) => `${file.path}\u0000${file.digest}`)
        .join("\n");
    return `sha256:${createHash("sha256").update(lines).digest("hex")}`;
}

/**
 * Built-in resource registry.
 *
 * Font digests are the SHA-256 of the bundled files under
 * `projects/vutex.rs/fonts`. A test recomputes them so the registry cannot
 * drift from the files shipped by the repository.
 */
const BUILTIN_SEEDS: Array<Pick<BuiltinPackage, "id" | "slot" | "version" | "files">> = [
    {
        id: "xits-math",
        slot: "math-font",
        version: "builtin",
        files: [
            {
                path: "XITSMath-Regular.otf",
                digest: "3025792adb0b7072ace08bf5726d341de32f2254612ddec6dd98271a8dd29689",
            },
        ],
    },
    {
        id: "latin-modern-math",
        slot: "math-font",
        version: "builtin",
        files: [
            {
                path: "LatinModernMath-Regular.otf",
                digest: "6075562b771f8b82f0c179e363389684f2dd09de30038269e2628e504bd7be0f",
            },
        ],
    },
    {
        id: "noto-serif-sc",
        slot: "font",
        version: "builtin",
        files: [
            {
                path: "NotoSerifSC-Regular.ttf",
                digest: "043a8c85f8ff69eaff7304f99e18f09e9207b5f409646e03774d95203a61407e",
            },
            {
                path: "NotoSerifSC-Bold.ttf",
                digest: "d0a964ff4d2c31d6b2f6bc7fd9ff03f8d780fb4ed9cba829fc08521bb2e579c8",
            },
        ],
    },
    {
        id: "noto-sans-mono",
        slot: "font",
        version: "builtin",
        files: [
            {
                path: "NotoSansMono-Regular.ttf",
                digest: "98bbd03bea509b41148f06d545e6522e2cdd63cd27ff68173e3fe6af6f7fcfd1",
            },
        ],
    },
    {
        id: "tinos",
        slot: "font",
        version: "builtin",
        files: [
            {
                path: "Tinos-Regular.ttf",
                digest: "3bb337e701eaee742ae90748f83e49e0c6a9427c361bde7fd89ea81b24895c8c",
            },
            {
                path: "Tinos-Bold.ttf",
                digest: "3a94ee4124abc908dc14f0f961f3653682ede1b6e1f9098d8dba4b90e1c2e3e9",
            },
            {
                path: "Tinos-Italic.ttf",
                digest: "23640368a2325ab8e5a272de59a16098050d3912997d7b031e00719651eaf6c4",
            },
        ],
    },
    { id: "baseline", slot: "theme", version: "builtin", files: [] },
    { id: "traditional", slot: "theme", version: "builtin", files: [] },
    { id: "editorial", slot: "theme", version: "builtin", files: [] },
    { id: "classic", slot: "theme", version: "builtin", files: [] },
    { id: "latex", slot: "profile", version: "builtin", files: [] },
    { id: "typst", slot: "profile", version: "builtin", files: [] },
    { id: "notedown", slot: "profile", version: "builtin", files: [] },
    { id: "markdown", slot: "profile", version: "builtin", files: [] },
    { id: "html", slot: "profile", version: "builtin", files: [] },
    { id: "docx", slot: "profile", version: "builtin", files: [] },
    { id: "epub", slot: "profile", version: "builtin", files: [] },
];

export const BUILTIN_PACKAGES: BuiltinPackage[] = BUILTIN_SEEDS.map((item) => ({
    ...item,
    artifactHash: treeDigest(item.files),
}));

export function lookupBuiltin(id: string, slot: PackageSlot): BuiltinPackage | undefined {
    return BUILTIN_PACKAGES.find((item) => item.id === id && item.slot === slot);
}

export function isBuiltinProfile(id: string): boolean {
    return lookupBuiltin(id, "profile") !== undefined;
}
