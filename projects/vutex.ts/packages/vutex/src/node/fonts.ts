import { Buffer } from "node:buffer";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

/** Native entries that fill the font asset registry. */
export interface FontAssetBinding {
    /** Path P1: the host reads bytes and hands them over. */
    configureFontAssets: (manifestJson: string, files: Record<string, Uint8Array>) => void;
    /** Path P2: the native side reads a directory the host names. */
    configureFontAssetsDir: (dir: string, manifestJson?: string) => void;
}

type FontManifest = {
    files?: { name?: string }[];
};

const require = createRequire(import.meta.url);

function fontError(code: string, detail: unknown): Error {
    const message = detail instanceof Error ? detail.message : String(detail);
    const error = new Error(`${code}: ${message}`);
    error.name = code;
    return error;
}

/**
 * Read the manifest and every face it names from `@vutex/vutex-fonts`, then
 * configure the native registry from those bytes (path P1).
 */
export async function configurePackageFonts(binding: FontAssetBinding): Promise<void> {
    let manifestPath: string;
    try {
        manifestPath = require.resolve("@vutex/vutex-fonts/fonts.json");
    } catch (error) {
        throw fontError("VUTEX_FONT_ASSETS_MANIFEST_INVALID", error);
    }
    const dir = dirname(manifestPath);
    const manifestJson = await readFile(manifestPath, "utf8");
    let manifest: FontManifest;
    try {
        manifest = JSON.parse(manifestJson) as FontManifest;
    } catch (error) {
        throw fontError("VUTEX_FONT_ASSETS_MANIFEST_INVALID", error);
    }
    const names = (manifest.files ?? [])
        .map((file) => file.name)
        .filter((name): name is string => typeof name === "string" && name.length > 0);
    if (names.length === 0) {
        throw fontError("VUTEX_FONT_ASSETS_MANIFEST_INVALID", "fonts.json declares no files");
    }
    const files: Record<string, Uint8Array> = {};
    await Promise.all(
        names.map(async (name) => {
            try {
                files[name] = Buffer.from(await readFile(join(dir, name)));
            } catch (error) {
                throw fontError("VUTEX_FONT_ASSETS_FILE_MISSING", `${name}: ${error instanceof Error ? error.message : error}`);
            }
        }),
    );
    binding.configureFontAssets(manifestJson, files);
}

/**
 * Configure fonts in host priority order: an explicit directory, then
 * `VUTEX_FONT_DIR`, then the installed `@vutex/vutex-fonts` pack.
 *
 * An explicit source that fails does not fall through to the next one.
 */
export async function configureFonts(binding: FontAssetBinding, assetsDir?: string): Promise<void> {
    const explicit = assetsDir?.trim();
    if (explicit) {
        binding.configureFontAssetsDir(explicit);
        return;
    }
    const fromEnv = process.env.VUTEX_FONT_DIR?.trim();
    if (fromEnv) {
        binding.configureFontAssetsDir(fromEnv);
        return;
    }
    await configurePackageFonts(binding);
}
