import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";
import test from "node:test";

import { PackageManager, PackageResolveError } from "../src/packages/manager.js";
import { readLockfile } from "../src/packages/lockfile.js";

const REGISTRY = "https://registry.test";

type Route = string | { status: number };

function digestOf(bytes: string): string {
    return createHash("sha256").update(bytes).digest("hex");
}

/** Deterministic transport stub that never touches the network. */
function transportFor(routes: Record<string, Route>): (url: string) => Promise<{ status: number; body: Uint8Array }> {
    const encoder = new TextEncoder();
    return async (url: string) => {
        const route = routes[url];
        if (route === undefined) {
            return { status: 404, body: encoder.encode("missing") };
        }
        if (typeof route === "object" && "status" in route) {
            return { status: route.status, body: encoder.encode("failure") };
        }
        return { status: 200, body: encoder.encode(route) };
    };
}

async function withProject(run: (dir: string) => Promise<void>): Promise<void> {
    const dir = await mkdtemp(join(tmpdir(), "vutex-registry-"));
    try {
        await run(dir);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

function metadataUrl(base: string, slot: string, name: string): string {
    return `${base}/${slot}/${encodeURIComponent(name)}.json`;
}

async function writeManifest(dir: string, packages: Record<string, string>, registry = REGISTRY): Promise<void> {
    const lines = [`[registry]`, `url = "${registry}"`, "", `[packages]`];
    for (const [slot, value] of Object.entries(packages)) {
        lines.push(`${slot} = "${value}"`);
    }
    await writeFile(join(dir, "vutex.toml"), `${lines.join("\n")}\n`, "utf8");
}

test("install selects a satisfying version, downloads files, and locks the tree hash", async () => {
    await withProject(async (dir) => {
        const payload = "libertinus math face bytes";
        const fileUrl = `${REGISTRY}/math-font/files/libertinus-math.otf`;
        const metadata = {
            slot: "math-font",
            name: "libertinus",
            versions: [
                { version: "1.0.0", files: [{ path: "libertinus-math.otf", url: "../../math-font/files/libertinus-math.otf" }] },
                {
                    version: "1.2.0",
                    files: [
                        {
                            path: "libertinus-math.otf",
                            url: "../../math-font/files/libertinus-math.otf",
                            sha256: digestOf(payload),
                        },
                    ],
                },
                { version: "2.0.0", files: [{ path: "libertinus-math.otf", url: "../../math-font/files/libertinus-math.otf" }] },
            ],
        };
        const transport = transportFor({ [metadataUrl(REGISTRY, "math-font", "libertinus")]: JSON.stringify(metadata), [fileUrl]: payload });
        await writeManifest(dir, { "math-font": "libertinus@^1.0.0" });

        const manager = new PackageManager(dir, { transport });
        await manager.install();
        const lock = await readLockfile(dir);
        assert.equal(lock?.packages[0]?.version, "1.2.0");
        assert.equal(lock?.packages[0]?.source, `registry+${REGISTRY}`);
        assert.match(lock?.packages[0]?.artifactHash ?? "", /^sha256:/u);

        const cached = await readFile(
            join(dir, ".vutex", "packages", "math-font", encodeURIComponent("libertinus"), (lock?.packages[0]?.artifactHash ?? "").slice(7), "libertinus-math.otf"),
            "utf8",
        );
        assert.equal(cached, payload);

        const activation = await manager.activation();
        assert.equal(activation.diagnostics.filter((item) => item.severity === "error").length, 0);
        assert.equal(activation.packages[0]?.name, "libertinus");
        assert.equal(activation.packages[0]?.version, "1.2.0");
    });
});

test("install resolves dependencies by slot and records them in the lock", async () => {
    await withProject(async (dir) => {
        const face = "face bytes";
        const body = "body bytes";
        const transport = transportFor({
            [metadataUrl(REGISTRY, "math-font", "libertinus")]: JSON.stringify({
                slot: "math-font",
                name: "libertinus",
                versions: [
                    {
                        version: "1.0.0",
                        files: [{ path: "math.otf", url: "../../files/math.otf" }],
                        dependencies: [{ slot: "font", name: "dejavu", range: "^1.0.0" }],
                    },
                ],
            }),
            [metadataUrl(REGISTRY, "font", "dejavu")]: JSON.stringify({
                slot: "font",
                name: "dejavu",
                versions: [{ version: "1.0.0", files: [{ path: "body.ttf", url: "../../files/body.ttf" }] }],
            }),
            [`${REGISTRY}/files/math.otf`]: face,
            [`${REGISTRY}/files/body.ttf`]: body,
        });
        await writeManifest(dir, { "math-font": "libertinus" });

        const manager = new PackageManager(dir, { transport });
        await manager.install();
        const lock = await readLockfile(dir);
        const bySlot = new Map(lock?.packages.map((item) => [item.slot, item]));
        assert.equal(bySlot.get("math-font")?.version, "1.0.0");
        assert.equal(bySlot.get("font")?.version, "1.0.0");
        assert.deepEqual(bySlot.get("math-font")?.dependencies, ["font:dejavu@1.0.0"]);
        for (const item of lock?.packages ?? []) {
            assert.ok(await stat(join(dir, ".vutex", "packages", item.slot, encodeURIComponent(item.name), (item.artifactHash ?? "").slice(7))));
        }

        const doctor = await manager.doctor();
        assert.equal(doctor.ok, true, JSON.stringify(doctor.checks));
    });
});

test("install fails closed when a dependency names another package in a declared slot", async () => {
    await withProject(async (dir) => {
        const transport = transportFor({
            [metadataUrl(REGISTRY, "font", "alpha")]: JSON.stringify({
                slot: "font",
                name: "alpha",
                versions: [{ version: "1.0.0", files: [{ path: "alpha.ttf", url: "../../files/alpha.ttf" }] }],
            }),
            [metadataUrl(REGISTRY, "font", "beta")]: JSON.stringify({
                slot: "font",
                name: "beta",
                versions: [{ version: "1.0.0", files: [{ path: "beta.ttf", url: "../../files/beta.ttf" }] }],
            }),
            [metadataUrl(REGISTRY, "template", "article")]: JSON.stringify({
                slot: "template",
                name: "article",
                versions: [
                    {
                        version: "1.0.0",
                        files: [{ path: "article.toml", url: "../../files/article.toml" }],
                        dependencies: [{ slot: "font", name: "alpha", range: "*" }],
                    },
                ],
            }),
            [`${REGISTRY}/files/alpha.ttf`]: "alpha",
            [`${REGISTRY}/files/beta.ttf`]: "beta",
            [`${REGISTRY}/files/article.toml`]: "article",
        });
        await writeManifest(dir, { font: "beta@^1.0.0", template: "article" });

        const manager = new PackageManager(dir, { transport });
        await assert.rejects(
            () => manager.install(),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics.some((item) => item.code === "VUTEX_PACKAGE_DEPENDENCY_CONFLICT"),
        );
    });
});

test("download failures keep the url and status in the diagnostic", async () => {
    await withProject(async (dir) => {
        const transport = transportFor({
            [metadataUrl(REGISTRY, "math-font", "libertinus")]: JSON.stringify({
                slot: "math-font",
                name: "libertinus",
                versions: [{ version: "1.0.0", files: [{ path: "math.otf", url: "../../files/math.otf" }] }],
            }),
            [`${REGISTRY}/files/math.otf`]: { status: 503 },
        });
        await writeManifest(dir, { "math-font": "libertinus@^1.0.0" });

        const manager = new PackageManager(dir, { transport });
        await assert.rejects(
            () => manager.install(),
            (error: unknown) => {
                if (!(error instanceof PackageResolveError)) {
                    return false;
                }
                const item = error.diagnostics[0];
                return item?.code === "VUTEX_PACKAGE_DOWNLOAD_FAILED" && (item.args as Record<string, unknown>)?.status === 503;
            },
        );
    });
});

test("an unreachable registry names the package and keeps the lock untouched", async () => {
    await withProject(async (dir) => {
        const transport = transportFor({ [metadataUrl(REGISTRY, "math-font", "libertinus")]: { status: 502 } });
        await writeManifest(dir, { "math-font": "libertinus" });

        const manager = new PackageManager(dir, { transport });
        await assert.rejects(
            () => manager.install(),
            (error: unknown) =>
                error instanceof PackageResolveError &&
                error.diagnostics[0]?.code === "VUTEX_PACKAGE_REGISTRY_UNREACHABLE",
        );
        assert.equal(await readLockfile(dir), null);
    });
});

test("activation fails when the cached tree is missing, without reaching the network", async () => {
    await withProject(async (dir) => {
        const payload = "cached face";
        const transport = transportFor({
            [metadataUrl(REGISTRY, "math-font", "libertinus")]: JSON.stringify({
                slot: "math-font",
                name: "libertinus",
                versions: [{ version: "1.0.0", files: [{ path: "math.otf", url: "../../files/math.otf" }] }],
            }),
            [`${REGISTRY}/files/math.otf`]: payload,
        });
        await writeManifest(dir, { "math-font": "libertinus" });
        const manager = new PackageManager(dir, { transport });
        await manager.install();

        const lock = await readLockfile(dir);
        const artifact = lock?.packages[0]?.artifactHash ?? "";
        await rm(join(dir, ".vutex", "packages", "math-font", encodeURIComponent("libertinus"), artifact.slice(7)), {
            recursive: true,
            force: true,
        });

        const offline = new PackageManager(dir, {
            transport: async () => {
                throw new Error("activation must stay offline");
            },
        });
        const activation = await offline.activation();
        assert.equal(activation.packages.length, 0);
        assert.equal(activation.diagnostics[0]?.code, "VUTEX_PACKAGE_ARTIFACT_MISSING");
    });
});
