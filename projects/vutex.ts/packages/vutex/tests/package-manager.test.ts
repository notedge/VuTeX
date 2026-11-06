import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { BUILTIN_PACKAGES } from "../src/packages/builtin.js";
import { contentDigest, PackageManager, PackageResolveError } from "../src/packages/manager.js";
import { readLockfile, writeLockfile } from "../src/packages/lockfile.js";

const REPO_FONTS = fileURLToPath(new URL("../../../../vutex.rs/fonts", import.meta.url));

async function withProject(run: (dir: string) => Promise<void>): Promise<void> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

async function writeManifest(dir: string, toml: string): Promise<void> {
    await writeFile(join(dir, "vutex.toml"), toml, "utf8");
}

test("builtin font digests match the bundled files", async () => {
    const fonts = BUILTIN_PACKAGES.filter((item) => item.files.length > 0);
    assert.ok(fonts.length > 0);
    for (const item of fonts) {
        for (const file of item.files) {
            const digest = createHash("sha256")
                .update(await readFile(join(REPO_FONTS, file.path)))
                .digest("hex");
            assert.equal(digest, file.digest, `${item.id} drifted from ${file.path}`);
        }
    }
});

test("install locks a builtin font by slot", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\nmath-font = "xits-math"\ntheme = "baseline"\n');
        const manager = new PackageManager(dir);
        await manager.install();

        const lock = await readLockfile(dir);
        const xits = BUILTIN_PACKAGES.find((item) => item.id === "xits-math");
        assert.ok(lock);
        assert.deepEqual(lock.packages.map((item) => item.slot), ["math-font", "theme"]);
        assert.deepEqual(lock.packages[0], {
            slot: "math-font",
            name: "xits-math",
            version: "builtin",
            source: "builtin",
            artifactHash: xits?.artifactHash,
            contentHash: contentDigest({
                slot: "math-font",
                name: "xits-math",
                version: "builtin",
                source: "builtin",
                artifactHash: xits?.artifactHash,
                dependencies: [],
            }),
            dependencies: [],
        });
    });
});

test("install is byte-identical across repeated runs", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\ntheme = "traditional"\nfont = "tinos"\n');
        const manager = new PackageManager(dir);
        await manager.install();
        const first = await readFile(join(dir, "vutex.lock"), "utf8");
        await manager.install();
        const second = await readFile(join(dir, "vutex.lock"), "utf8");

        assert.equal(first, second);
        assert.match(first, /font[\s\S]*theme/);
    });
});

test("add writes the package under its inferred slot", async () => {
    await withProject(async (dir) => {
        const manager = new PackageManager(dir);
        await manager.add("@vutex/font-libertinus");
        const manifest = await readFile(join(dir, "vutex.toml"), "utf8");
        assert.match(manifest, /font = "@vutex\/font-libertinus"/);
    });
});

test("add fails when the slot cannot be inferred", async () => {
    await withProject(async (dir) => {
        const manager = new PackageManager(dir);
        await assert.rejects(
            () => manager.add("xits-math"),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics[0]?.code === "VUTEX_PACKAGE_SLOT_REQUIRED",
        );
    });
});

test("remove deletes a package by name and by slot", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\nfont = "tinos"\ntheme = "baseline"\n');
        const manager = new PackageManager(dir);
        await manager.remove("tinos");
        let manifest = await readFile(join(dir, "vutex.toml"), "utf8");
        assert.doesNotMatch(manifest, /tinos/);
        await manager.remove("theme");
        manifest = await readFile(join(dir, "vutex.toml"), "utf8");
        assert.doesNotMatch(manifest, /baseline/);
    });
});

test("install rejects an unknown slot", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\ncolors = "xits-math"\n');
        const manager = new PackageManager(dir);
        await assert.rejects(
            () => manager.install(),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics[0]?.code === "VUTEX_PACKAGE_SLOT_UNKNOWN",
        );
    });
});

test("install reports a missing builtin font", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\nmath-font = "libertinus"\n');
        const manager = new PackageManager(dir);
        await assert.rejects(
            () => manager.install(),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics[0]?.code === "VUTEX_PACKAGE_FONT_MISSING",
        );
        const report = await manager.doctor();
        const fonts = report.checks.find((check) => check.id === "fonts");
        assert.equal(fonts?.code, "VUTEX_PACKAGE_FONT_MISSING");
    });
});

test("install reports a scoped package without a configured registry", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\nmath-font = "@vutex/math-font-libertinus"\n');
        const manager = new PackageManager(dir);
        await assert.rejects(
            () => manager.install(),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics[0]?.code === "VUTEX_PACKAGE_REGISTRY_UNCONFIGURED",
        );
    });
});

test("path sources hash the resource tree and doctor detects drift", async () => {
    await withProject(async (dir) => {
        const resource = join(dir, "resources", "math");
        await managerAddFile(resource, "font.otf");
        await writeManifest(dir, '[packages]\nmath-font = "./resources/math"\n');

        const manager = new PackageManager(dir);
        await manager.install();
        const lock = await readLockfile(dir);
        assert.ok(lock);
        assert.equal(lock.packages[0]?.source, "path");
        assert.equal(lock.packages[0]?.version, "local");
        assert.match(lock.packages[0]?.artifactHash ?? "", /^sha256:[0-9a-f]{64}$/);

        let report = await manager.doctor();
        assert.equal(report.ok, true);

        await writeFile(join(resource, "font.otf"), "changed", "utf8");
        report = await manager.doctor();
        const artifacts = report.checks.find((check) => check.id === "artifacts");
        assert.equal(artifacts?.ok, false);
        assert.equal(artifacts?.code, "VUTEX_PACKAGE_ARTIFACT_MISMATCH");
    });
});

async function managerAddFile(dir: string, name: string): Promise<void> {
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, name), "font-bytes", "utf8");
}

test("doctor reports a missing path source", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\nfont = "./resources/missing"\n');
        const manager = new PackageManager(dir);
        await assert.rejects(
            () => manager.install(),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics[0]?.code === "VUTEX_PACKAGE_PATH_MISSING",
        );
    });
});

test("doctor reports a tampered record digest", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\ntheme = "baseline"\n');
        const manager = new PackageManager(dir);
        await manager.install();

        const lock = await readLockfile(dir);
        assert.ok(lock);
        lock.packages[0]!.contentHash = `sha256:${"0".repeat(64)}`;
        await writeLockfile(dir, lock);

        const report = await manager.doctor();
        const integrity = report.checks.find((check) => check.id === "integrity");
        assert.equal(integrity?.ok, false);
        assert.equal(integrity?.code, "VUTEX_PACKAGE_HASH_MISMATCH");
    });
});

test("doctor reports unactivated and stale slots", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[packages]\nfont = "tinos"\ntheme = "baseline"\n');
        const manager = new PackageManager(dir);
        await manager.install();

        await writeManifest(dir, '[packages]\nfont = "tinos"\n');
        const stale = await manager.doctor();
        const staleActivation = stale.checks.find((check) => check.id === "activation");
        assert.equal(staleActivation?.ok, false);
        assert.equal(staleActivation?.code, "VUTEX_PACKAGE_UNACTIVATED");
        assert.match(staleActivation?.message ?? "", /stale: theme/);

        await writeManifest(dir, '[packages]\nmath-font = "xits-math"\n');
        const missing = await manager.doctor();
        const missingActivation = missing.checks.find((check) => check.id === "activation");
        assert.equal(missingActivation?.ok, false);
        assert.match(missingActivation?.message ?? "", /unactivated: math-font/);
    });
});

test("doctor reports a profile conflict", async () => {
    await withProject(async (dir) => {
        await writeManifest(dir, '[profile]\nname = "book"\n\n[packages]\nprofile = "latex"\n');
        const manager = new PackageManager(dir);
        await manager.install();

        const report = await manager.doctor();
        const profile = report.checks.find((check) => check.id === "profile");
        assert.equal(profile?.code, "VUTEX_PACKAGE_PROFILE_CONFLICT");
    });
});

test("activation lists the packages and emits info diagnostics", async () => {
    await withProject(async (dir) => {
        await managerAddFile(join(dir, "resources", "template"), "template.toml");
        await writeManifest(dir, '[packages]\nmath-font = "xits-math"\ntemplate = "./resources/template"\n');

        const manager = new PackageManager(dir);
        await manager.install();

        const activation = await manager.activation();
        assert.deepEqual(
            activation.packages.map((item) => item.slot),
            ["math-font", "template"],
        );
        assert.deepEqual(
            activation.diagnostics.map((item) => item.code),
            ["VUTEX_PACKAGE_ACTIVATED", "VUTEX_PACKAGE_ACTIVATED"],
        );
    });
});
