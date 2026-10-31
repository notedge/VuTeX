import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { PackageManager } from "../src/packages/manager.js";
import { readLockfile } from "../src/packages/lockfile.js";
import { readManifest } from "../src/packages/manifest.js";

test("package manager writes lockfile on install", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);
        await manager.add("font-libertinus", { version: "^0.1.0" });
        await manager.install();
        const lock = await readFile(join(dir, "vutex.lock"), "utf8");
        assert.match(lock, /font-libertinus/);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("list falls back lock > manifest > empty", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);

        assert.deepEqual(await manager.list(), []);

        await manager.add("alpha", { version: "1.0.0" });
        assert.deepEqual(await manager.list(), [
            { name: "alpha", version: "1.0.0", source: "manifest" },
        ]);

        await manager.install();
        await writeFile(join(dir, "vutex.toml"), '[packages]\nbeta = "2.0.0"\n', "utf8");
        assert.deepEqual(await manager.list(), [
            { name: "alpha", version: "1.0.0", source: "@vutex/alpha" },
        ]);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("add stores a scoped name under its trailing alias and defaults to latest", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);
        await manager.add("fonts/libertinus");
        const manifest = await readManifest(dir);
        assert.deepEqual(manifest?.packages, { libertinus: "latest" });
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("add preserves unrelated document and profile sections", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        await writeFile(
            join(dir, "vutex.toml"),
            '[document]\ninput = "main.md"\noutput = "svg"\n\n[profile]\nname = "book"\n',
            "utf8",
        );
        const manager = new PackageManager(dir);
        await manager.add("alpha", { version: "0.2.0" });

        const manifest = await readManifest(dir);
        assert.equal(manifest?.document?.input, "main.md");
        assert.equal(manifest?.document?.output, "svg");
        assert.equal(manifest?.profile?.name, "book");
        assert.deepEqual(manifest?.packages, { alpha: "0.2.0" });
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("remove is a no-op without packages and deletes only the target entry", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);

        await manager.remove("ghost");
        await assert.rejects(readFile(join(dir, "vutex.toml"), "utf8"), { code: "ENOENT" });

        await manager.add("keep-me", { version: "1.0.0" });
        await manager.add("drop-me", { version: "2.0.0" });
        await manager.remove("drop-me");

        const manifest = await readManifest(dir);
        assert.deepEqual(manifest?.packages, { "keep-me": "1.0.0" });
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("install writes scaffold lock entries with source, hash, and empty dependencies", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);
        await manager.add("alpha", { version: "1.2.3" });
        await manager.install();

        const lock = await readLockfile(dir);
        assert.equal(lock?.version, 1);
        assert.deepEqual(lock?.packages, [
            {
                name: "alpha",
                version: "1.2.3",
                source: "@vutex/alpha",
                contentHash: "sha256:scaffold:alpha@1.2.3",
                dependencies: [],
            },
        ]);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("install merges options.name into the locked set", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);
        await manager.add("alpha", { version: "1.0.0" });
        await manager.install({ name: "extra", version: "9.9.9" });

        const lock = await readLockfile(dir);
        assert.deepEqual(
            lock?.packages.map((item) => `${item.name}@${item.version}`),
            ["alpha@1.0.0", "extra@9.9.9"],
        );
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("update rewrites the lock from the manifest like install", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);
        await manager.add("alpha", { version: "1.0.0" });
        await manager.update();

        const lock = await readLockfile(dir);
        assert.deepEqual(lock?.packages.map((item) => item.name), ["alpha"]);
        assert.equal(lock?.packages[0]?.source, "@vutex/alpha");
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("doctor reports the four manifest and lock states", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);

        const empty = await manager.doctor();
        assert.equal(empty.ok, false);
        assert.equal(empty.checks.find((c) => c.id === "manifest")?.ok, false);
        assert.equal(empty.checks.find((c) => c.id === "lockfile")?.ok, true);
        assert.match(empty.checks.find((c) => c.id === "lockfile")?.message ?? "", /no resource packages/);

        await manager.add("alpha", { version: "1.0.0" });
        const manifestOnly = await manager.doctor();
        assert.equal(manifestOnly.ok, false);
        assert.equal(manifestOnly.checks.find((c) => c.id === "manifest")?.ok, true);
        assert.equal(manifestOnly.checks.find((c) => c.id === "lockfile")?.ok, false);
        assert.match(manifestOnly.checks.find((c) => c.id === "lockfile")?.message ?? "", /missing/);

        await manager.install();
        const both = await manager.doctor();
        assert.equal(both.ok, true);
        assert.match(both.checks.find((c) => c.id === "lockfile")?.message ?? "", /tracks 1 package/);

        await writeFile(join(dir, "vutex.toml"), '[profile]\nname = "book"\n', "utf8");
        const noPackages = await manager.doctor();
        assert.equal(noPackages.ok, true);
        assert.match(noPackages.checks.find((c) => c.id === "lockfile")?.message ?? "", /no resource packages/);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
