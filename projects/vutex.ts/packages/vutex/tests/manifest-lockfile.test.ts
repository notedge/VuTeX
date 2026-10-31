import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { readManifest } from "../src/packages/manifest.js";
import { readLockfile, writeLockfile, type VuTeXLockfile } from "../src/packages/lockfile.js";

test("readManifest returns null when vutex.toml is missing", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-manifest-"));
    try {
        assert.equal(await readManifest(dir), null);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("readLockfile returns null when vutex.lock is missing", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-lock-"));
    try {
        assert.equal(await readLockfile(dir), null);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("readManifest rethrows malformed TOML as a non-ENOENT error", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-manifest-"));
    try {
        await writeFile(join(dir, "vutex.toml"), "[packages\nbroken", "utf8");
        await assert.rejects(readManifest(dir), (error: unknown) => {
            assert.ok(error instanceof Error);
            assert.notEqual((error as NodeJS.ErrnoException).code, "ENOENT");
            return true;
        });
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("readLockfile rethrows malformed TOML as a non-ENOENT error", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-lock-"));
    try {
        await writeFile(join(dir, "vutex.lock"), "version = = 1", "utf8");
        await assert.rejects(readLockfile(dir), (error: unknown) => {
            assert.ok(error instanceof Error);
            assert.notEqual((error as NodeJS.ErrnoException).code, "ENOENT");
            return true;
        });
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("writeLockfile and readLockfile round-trip every field", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-lock-"));
    try {
        const lockfile: VuTeXLockfile = {
            version: 1,
            packages: [
                {
                    name: "alpha",
                    version: "1.2.3",
                    source: "@vutex/alpha",
                    contentHash: "sha256:abc",
                    dependencies: ["beta"],
                },
                {
                    name: "beta",
                    version: "0.9.0",
                    source: "@vutex/beta",
                    contentHash: "sha256:def",
                    dependencies: [],
                },
            ],
        };
        await writeLockfile(dir, lockfile);
        const restored = await readLockfile(dir);
        assert.deepEqual(restored, lockfile);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("writeLockfile terminates the file with a trailing newline", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-lock-"));
    try {
        await writeLockfile(dir, { version: 1, packages: [] });
        const raw = await readFile(join(dir, "vutex.lock"), "utf8");
        assert.ok(raw.endsWith("\n"));
        // NOTE: TOML.stringify already ends with a newline, so the explicit "\n"
        // append in writeLockfile produces a double trailing blank line.
        assert.ok(raw.endsWith("\n\n"));
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
