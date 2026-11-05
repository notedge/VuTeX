import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import TOML from "@iarna/toml";

import type {
    PackageCheck,
    PackageDoctorReport,
    PackageInstallOptions,
    PackageRecord,
} from "../types.js";
import { readLockfile, writeLockfile, type LockedPackage } from "./lockfile.js";
import { readManifest, type VuTeXManifest } from "./manifest.js";

function canonicalRecord(entry: Omit<LockedPackage, "contentHash">): string {
    return [entry.name, entry.version, entry.source, [...entry.dependencies].sort().join(",")].join(
        "\u0000",
    );
}

/** SHA-256 of the canonical package record, never of a downloaded artifact. */
export function contentDigest(entry: Omit<LockedPackage, "contentHash">): string {
    return `sha256:${createHash("sha256").update(canonicalRecord(entry)).digest("hex")}`;
}

function failedCheck(id: string, code: string, message: string): PackageCheck {
    return { id, ok: false, code, message };
}

function passedCheck(id: string, message: string): PackageCheck {
    return { id, ok: true, message };
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
            return Object.entries(manifest.packages)
                .sort(([left], [right]) => left.localeCompare(right, "en"))
                .map(([alias, version]) => ({
                    name: alias,
                    version,
                    source: "manifest",
                }));
        }
        return [...lock.packages]
            .sort((left, right) => left.name.localeCompare(right.name, "en"))
            .map((item) => ({
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
        await this.writeManifest({ ...manifest, packages });
    }

    async remove(name: string): Promise<void> {
        const manifest = await readManifest(this.projectRoot);
        if (!manifest?.packages) {
            return;
        }
        const packages = { ...manifest.packages };
        delete packages[name];
        await this.writeManifest({ ...manifest, packages });
    }

    async install(options: PackageInstallOptions = {}): Promise<void> {
        const manifest = await readManifest(this.projectRoot);
        const cacheDir = join(this.projectRoot, ".vutex", "packages");
        await mkdir(cacheDir, { recursive: true });

        const declared: Record<string, string> = { ...(manifest?.packages ?? {}) };
        if (options.name) {
            declared[options.name] = options.version ?? "latest";
        }

        const entries: LockedPackage[] = Object.keys(declared)
            .sort((left, right) => left.localeCompare(right, "en"))
            .map((name) => {
                const record = {
                    name,
                    version: declared[name],
                    source: `@vutex/${name}`,
                    dependencies: [] as string[],
                };
                return { ...record, contentHash: contentDigest(record) };
            });

        await writeLockfile(this.projectRoot, {
            version: 1,
            packages: entries,
        });
    }

    async update(): Promise<void> {
        await this.install();
    }

    async doctor(): Promise<PackageDoctorReport> {
        const checks: PackageCheck[] = [];
        const manifest = await readManifest(this.projectRoot);
        const lock = await readLockfile(this.projectRoot);

        if (manifest) {
            checks.push(passedCheck("manifest", "vutex.toml present"));
        } else {
            checks.push(failedCheck("manifest", "VUTEX_MANIFEST_MISSING", "vutex.toml is missing"));
        }

        const declared = manifest?.packages ?? {};
        const declaredNames = Object.keys(declared).sort((left, right) => left.localeCompare(right, "en"));

        if (declaredNames.length === 0) {
            checks.push(passedCheck("lockfile", "no resource packages declared"));
        } else if (!lock) {
            checks.push(failedCheck("lockfile", "VUTEX_LOCK_MISSING", "vutex.lock is missing"));
        } else {
            checks.push(passedCheck("lockfile", `vutex.lock tracks ${lock.packages.length} package(s)`));
        }

        if (declaredNames.length > 0 && lock) {
            checks.push(...this.checkActivation(declared, lock.packages));
            checks.push(...this.checkIntegrity(lock.packages));
        }
        checks.push(...this.checkProfile(manifest?.profile?.name, declared));

        const report: PackageDoctorReport = {
            ok: checks.every((check) => check.ok),
            checks,
        };
        return report;
    }

    private checkActivation(
        declared: Record<string, string>,
        locked: LockedPackage[],
    ): PackageCheck[] {
        const lockedByName = new Map(locked.map((item) => [item.name, item]));
        const missing = Object.keys(declared).filter((name) => !lockedByName.has(name));
        const stale = locked.filter((item) => !(item.name in declared)).map((item) => item.name);
        const drifted = Object.keys(declared)
            .filter((name) => lockedByName.has(name))
            .filter((name) => lockedByName.get(name)?.version !== declared[name]);

        const checks: PackageCheck[] = [];
        if (missing.length === 0 && stale.length === 0) {
            checks.push(passedCheck("activation", "every declared package is locked and active"));
        } else {
            const parts: string[] = [];
            if (missing.length > 0) {
                parts.push(`unactivated: ${missing.join(", ")}`);
            }
            if (stale.length > 0) {
                parts.push(`stale: ${stale.join(", ")}`);
            }
            checks.push(failedCheck("activation", "VUTEX_PACKAGE_UNACTIVATED", parts.join(" but ")));
        }

        if (drifted.length === 0) {
            checks.push(passedCheck("version", "locked versions match the manifest"));
        } else {
            checks.push(
                failedCheck(
                    "version",
                    "VUTEX_PACKAGE_VERSION_DRIFT",
                    `manifest and lock disagree for: ${drifted.join(", ")}`,
                ),
            );
        }
        return checks;
    }

    private checkIntegrity(locked: LockedPackage[]): PackageCheck[] {
        const broken = locked.filter((item) => contentDigest(item) !== item.contentHash);
        if (broken.length === 0) {
            return [passedCheck("integrity", "lockfile content hashes match their records")];
        }
        return [
            failedCheck(
                "integrity",
                "VUTEX_PACKAGE_HASH_MISMATCH",
                `content hash mismatch for: ${broken.map((item) => item.name).join(", ")}`,
            ),
        ];
    }

    private checkProfile(
        profileName: string | undefined,
        declared: Record<string, string>,
    ): PackageCheck[] {
        const packageProfile = declared.profile;
        if (packageProfile === undefined || profileName === undefined) {
            return [passedCheck("profile", "no profile conflict")];
        }
        if (packageProfile === profileName) {
            return [passedCheck("profile", `profile matches the active profile ${profileName}`)];
        }
        return [
            failedCheck(
                "profile",
                "VUTEX_PACKAGE_PROFILE_CONFLICT",
                `profile package ${packageProfile} conflicts with profile ${profileName}`,
            ),
        ];
    }

    private async writeManifest(manifest: VuTeXManifest): Promise<void> {
        await writeFile(join(this.projectRoot, "vutex.toml"), `${TOML.stringify(manifest)}\n`, "utf8");
    }
}
