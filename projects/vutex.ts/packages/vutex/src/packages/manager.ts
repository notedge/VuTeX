import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import TOML from "@iarna/toml";

import type { PackageDoctorReport, PackageInstallOptions, PackageRecord } from "../types.js";
import { readLockfile, writeLockfile, type LockedPackage } from "./lockfile.js";
import { readManifest } from "./manifest.js";

function hashPlaceholder(name: string, version: string): string {
    return `sha256:scaffold:${name}@${version}`;
}

export class PackageManager {
    constructor(private readonly projectRoot: string) {}

    async list(): Promise<PackageRecord[]> {
        const lock = await readLockfile(this.projectRoot);
        if (!lock) {
            const manifest = await readManifest(this.projectRoot);
            if (!manifest?.packages) {
                return [];
            }
            return Object.entries(manifest.packages).map(([alias, version]) => ({
                name: alias,
                version,
                source: "manifest",
            }));
        }
        return lock.packages.map((item) => ({
            name: item.name,
            version: item.version,
            source: item.source,
        }));
    }

    async add(name: string, options: PackageInstallOptions = {}): Promise<void> {
        const manifest = (await readManifest(this.projectRoot)) ?? {};
        const packages = { ...(manifest.packages ?? {}) };
        const alias = name.includes("/") ? name.split("/").pop() ?? name : name;
        packages[alias] = options.version ?? "latest";
        const next = {
            ...manifest,
            packages,
        };
        await writeFile(join(this.projectRoot, "vutex.toml"), `${TOML.stringify(next)}\n`, "utf8");
    }

    async remove(name: string): Promise<void> {
        const manifest = await readManifest(this.projectRoot);
        if (!manifest?.packages) {
            return;
        }
        const packages = { ...manifest.packages };
        delete packages[name];
        const next = { ...manifest, packages };
        await writeFile(join(this.projectRoot, "vutex.toml"), `${TOML.stringify(next)}\n`, "utf8");
    }

    async install(options: PackageInstallOptions = {}): Promise<void> {
        const manifest = await readManifest(this.projectRoot);
        const cacheDir = join(this.projectRoot, ".vutex", "packages");
        await mkdir(cacheDir, { recursive: true });

        const entries: LockedPackage[] = [];
        const sourcePackages = manifest?.packages ?? {};
        if (options.name) {
            sourcePackages[options.name] = options.version ?? "latest";
        }

        for (const [alias, version] of Object.entries(sourcePackages)) {
            entries.push({
                name: alias,
                version,
                source: `@vutex/${alias}`,
                contentHash: hashPlaceholder(alias, version),
                dependencies: [],
            });
        }

        await writeLockfile(this.projectRoot, {
            version: 1,
            packages: entries,
        });
    }

    async update(): Promise<void> {
        await this.install();
    }

    async doctor(): Promise<PackageDoctorReport> {
        const checks: PackageDoctorReport["checks"] = [];
        const manifest = await readManifest(this.projectRoot);
        const lock = await readLockfile(this.projectRoot);

        checks.push({
            id: "manifest",
            ok: manifest !== null,
            message: manifest ? "vutex.toml present" : "vutex.toml missing (optional for scaffold)",
        });

        if (manifest?.packages && Object.keys(manifest.packages).length > 0) {
            checks.push({
                id: "lockfile",
                ok: lock !== null && lock.packages.length > 0,
                message: lock ? `vutex.lock tracks ${lock.packages.length} package(s)` : "vutex.lock missing",
            });
        } else {
            checks.push({
                id: "lockfile",
                ok: true,
                message: "no resource packages declared",
            });
        }

        const ok = checks.every((check) => check.ok);
        return { ok, checks };
    }
}
