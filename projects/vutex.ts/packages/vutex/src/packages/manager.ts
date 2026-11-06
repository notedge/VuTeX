import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";

import TOML from "@iarna/toml";

import type {
    ActivatedPackage,
    PackageCheck,
    PackageDoctorReport,
    PackageInstallOptions,
    PackageRecord,
    VuTeXDiagnostic,
} from "../types.js";
import { lookupBuiltin } from "./builtin.js";
import { readLockfile, writeLockfile, type LockedPackage } from "./lockfile.js";
import { inferSlot, isPackageSlot, readManifest, type VuTeXManifest } from "./manifest.js";

export type ResolvedPackage = Omit<LockedPackage, "contentHash">;

export class PackageResolveError extends Error {
    constructor(readonly diagnostics: VuTeXDiagnostic[]) {
        super(diagnostics.map((item) => `${item.code}: ${item.message}`).join(" | "));
        this.name = "PackageResolveError";
    }
}

function packageDiagnostic(code: string, message: string, args?: Record<string, unknown>): VuTeXDiagnostic {
    return { code, severity: "error", message, stage: "package", args };
}

function canonicalRecord(entry: Omit<LockedPackage, "contentHash">): string {
    return [
        entry.slot,
        entry.name,
        entry.version,
        entry.source,
        [...entry.dependencies].sort().join(","),
    ].join("\u0000");
}

/** SHA-256 of the canonical package record, not of the resource bytes. */
export function contentDigest(entry: Omit<LockedPackage, "contentHash">): string {
    return `sha256:${createHash("sha256").update(canonicalRecord(entry)).digest("hex")}`;
}

function isPathSource(value: string): boolean {
    return (
        value.startsWith("file:") ||
        value.startsWith("./") ||
        value.startsWith("../") ||
        value.startsWith("/") ||
        value.startsWith("\\") ||
        /^[A-Za-z]:[\\/]/u.test(value)
    );
}

function bareName(value: string): string {
    return value.startsWith("@vutex/") ? value.slice("@vutex/".length) : value;
}

async function hashTree(root: string): Promise<{ artifactHash: string; files: number }> {
    const entries: string[] = [];
    async function walk(dir: string, prefix: string): Promise<void> {
        for (const item of await readdir(dir, { withFileTypes: true })) {
            const name = prefix.length > 0 ? `${prefix}/${item.name}` : item.name;
            if (item.isSymbolicLink()) {
                throw new Error(`symbolic links are not supported in resource packages: ${name}`);
            }
            if (item.isDirectory()) {
                await walk(join(dir, item.name), name);
            } else if (item.isFile()) {
                const digest = createHash("sha256")
                    .update(await readFile(join(dir, item.name)))
                    .digest("hex");
                entries.push(`${name}\u0000${digest}`);
            }
        }
    }
    await walk(root, "");
    const lines = entries.sort((left, right) => left.localeCompare(right, "en")).join("\n");
    return {
        artifactHash: `sha256:${createHash("sha256").update(lines).digest("hex")}`,
        files: entries.length,
    };
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
                .map(([slot, name]) => ({ slot, name, version: "unresolved", source: "manifest" }));
        }
        return [...lock.packages]
            .sort((left, right) =>
                left.slot === right.slot
                    ? left.name.localeCompare(right.name, "en")
                    : left.slot.localeCompare(right.slot, "en"),
            )
            .map((item) => ({
                slot: item.slot,
                name: item.name,
                version: item.version,
                source: item.source,
                artifactHash: item.artifactHash,
            }));
    }

    async add(name: string, options: PackageInstallOptions = {}): Promise<void> {
        const slot = options.slot ?? inferSlot(name);
        if (slot === null || !isPackageSlot(slot)) {
            throw new PackageResolveError([
                packageDiagnostic(
                    "VUTEX_PACKAGE_SLOT_REQUIRED",
                    `cannot infer a slot for ${name}, pass --slot <slot>`,
                ),
            ]);
        }
        const manifest = (await readManifest(this.projectRoot)) ?? {};
        const packages = { ...(manifest.packages ?? {}) };
        packages[slot] = name;
        await this.writeManifest({ ...manifest, packages });
    }

    async remove(name: string): Promise<void> {
        const manifest = await readManifest(this.projectRoot);
        if (!manifest?.packages) {
            return;
        }
        const packages = { ...manifest.packages };
        const bare = bareName(name);
        let removed = false;
        if (name in packages) {
            delete packages[name];
            removed = true;
        }
        for (const [slot, value] of Object.entries(packages)) {
            if (bareName(value) === bare) {
                delete packages[slot];
                removed = true;
            }
        }
        if (removed) {
            await this.writeManifest({ ...manifest, packages });
        }
    }

    async install(options: PackageInstallOptions = {}): Promise<void> {
        const declared = await this.declaredPackages(options);
        const plan = await this.resolvePlan(declared);
        const errors = plan.diagnostics.filter((item) => item.severity === "error");
        if (errors.length > 0) {
            throw new PackageResolveError(errors);
        }

        await mkdir(join(this.projectRoot, ".vutex", "packages"), { recursive: true });
        const entries: LockedPackage[] = plan.packages.map((item) => ({
            ...item,
            contentHash: contentDigest(item),
        }));
        await writeLockfile(this.projectRoot, { version: 1, packages: entries });
    }

    async update(): Promise<void> {
        await this.install();
    }

    async doctor(): Promise<PackageDoctorReport> {
        const checks: PackageCheck[] = [];
        const manifest = await readManifest(this.projectRoot);
        const lock = await readLockfile(this.projectRoot);

        checks.push(
            manifest
                ? passedCheck("manifest", "vutex.toml present")
                : failedCheck("manifest", "VUTEX_MANIFEST_MISSING", "vutex.toml is missing"),
        );

        const declared = manifest?.packages ?? {};
        const declaredSlots = Object.keys(declared).sort((left, right) => left.localeCompare(right, "en"));
        const plan = await this.resolvePlan(declared);

        const unknownSlots = plan.diagnostics.filter((item) => item.code === "VUTEX_PACKAGE_SLOT_UNKNOWN");
        checks.push(
            unknownSlots.length === 0
                ? passedCheck("slots", "every package slot is known")
                : failedCheck("slots", "VUTEX_PACKAGE_SLOT_UNKNOWN", unknownSlots.map((item) => item.message).join(" | ")),
        );

        const sourceIssues = plan.diagnostics.filter(
            (item) =>
                item.severity === "error" &&
                item.code !== "VUTEX_PACKAGE_SLOT_UNKNOWN" &&
                item.code !== "VUTEX_PACKAGE_FONT_MISSING",
        );
        checks.push(
            sourceIssues.length === 0
                ? passedCheck("sources", "every package source resolves")
                : failedCheck("sources", sourceIssues[0].code ?? "VUTEX_PACKAGE_NOT_FOUND", sourceIssues.map((item) => item.message).join(" | ")),
        );

        const fontIssues = plan.diagnostics.filter((item) => item.code === "VUTEX_PACKAGE_FONT_MISSING");
        checks.push(
            fontIssues.length === 0
                ? passedCheck("fonts", "every declared font resolves")
                : failedCheck("fonts", "VUTEX_PACKAGE_FONT_MISSING", fontIssues.map((item) => item.message).join(" | ")),
        );

        if (declaredSlots.length === 0) {
            checks.push(passedCheck("lockfile", "no resource packages declared"));
        } else if (!lock) {
            checks.push(failedCheck("lockfile", "VUTEX_LOCK_MISSING", "vutex.lock is missing"));
        } else {
            checks.push(passedCheck("lockfile", `vutex.lock tracks ${lock.packages.length} package(s)`));
        }

        if (declaredSlots.length > 0 && lock) {
            checks.push(...this.checkActivation(declared, plan.packages, lock.packages));
            checks.push(...this.checkIntegrity(lock.packages));
            checks.push(...this.checkArtifacts(plan.packages, lock.packages));
        }
        checks.push(...this.checkProfile(manifest?.profile?.name, declared));

        return { ok: checks.every((check) => check.ok), checks };
    }

    /**
     * Resolve the packages activated for a render.
     *
     * Error diagnostics mean the render must not silently proceed with a
     * missing, stale, or corrupted resource.
     */
    async activation(): Promise<{ packages: ActivatedPackage[]; diagnostics: VuTeXDiagnostic[] }> {
        const manifest = await readManifest(this.projectRoot);
        const declared = manifest?.packages ?? {};
        const declaredSlots = Object.keys(declared).sort((left, right) => left.localeCompare(right, "en"));
        if (declaredSlots.length === 0) {
            return { packages: [], diagnostics: [] };
        }

        const plan = await this.resolvePlan(declared);
        const diagnostics: VuTeXDiagnostic[] = [...plan.diagnostics];
        const lock = await readLockfile(this.projectRoot);
        if (!lock) {
            diagnostics.push(
                packageDiagnostic("VUTEX_PACKAGE_UNACTIVATED", "vutex.lock is missing, run `vutex package install`"),
            );
            return { packages: [], diagnostics };
        }

        const lockedBySlot = new Map(lock.packages.map((item) => [item.slot, item]));
        const packages: ActivatedPackage[] = [];
        for (const slot of declaredSlots) {
            const resolved = plan.packages.find((item) => item.slot === slot);
            const locked = lockedBySlot.get(slot);
            if (!locked) {
                diagnostics.push(
                    packageDiagnostic("VUTEX_PACKAGE_UNACTIVATED", `${slot} is declared but not locked`),
                );
                continue;
            }
            if (resolved && locked.name !== resolved.name) {
                diagnostics.push(
                    packageDiagnostic(
                        "VUTEX_PACKAGE_UNACTIVATED",
                        `${slot} is locked to ${locked.name} but the manifest declares ${resolved.name}`,
                    ),
                );
                continue;
            }
            if (resolved && locked.version !== resolved.version) {
                diagnostics.push(
                    packageDiagnostic(
                        "VUTEX_PACKAGE_VERSION_DRIFT",
                        `${slot} is locked at ${locked.version} but resolves to ${resolved.version}`,
                    ),
                );
                continue;
            }
            if (resolved && locked.artifactHash !== resolved.artifactHash) {
                diagnostics.push(
                    packageDiagnostic(
                        "VUTEX_PACKAGE_ARTIFACT_MISMATCH",
                        `${slot} artifact hash does not match ${locked.name}`,
                    ),
                );
                continue;
            }
            if (contentDigest(locked) !== locked.contentHash) {
                diagnostics.push(
                    packageDiagnostic(
                        "VUTEX_PACKAGE_HASH_MISMATCH",
                        `${slot} content hash does not match its record`,
                    ),
                );
                continue;
            }
            packages.push({
                slot,
                name: locked.name,
                version: locked.version,
                source: locked.source,
                artifactHash: locked.artifactHash,
            });
            diagnostics.push({
                code: "VUTEX_PACKAGE_ACTIVATED",
                severity: "info",
                message: `${slot} activates ${locked.name}`,
                stage: "package",
                args: { slot, name: locked.name, version: locked.version, source: locked.source },
            });
        }
        return { packages, diagnostics };
    }

    private async declaredPackages(options: PackageInstallOptions): Promise<Record<string, string>> {
        const manifest = await readManifest(this.projectRoot);
        const declared: Record<string, string> = { ...(manifest?.packages ?? {}) };
        if (options.name) {
            const slot = options.slot ?? inferSlot(options.name);
            if (slot === null || !isPackageSlot(slot)) {
                throw new PackageResolveError([
                    packageDiagnostic(
                        "VUTEX_PACKAGE_SLOT_REQUIRED",
                        `cannot infer a slot for ${options.name}, pass --slot <slot>`,
                    ),
                ]);
            }
            declared[slot] = options.name;
        }
        return declared;
    }

    private async resolvePlan(
        declared: Record<string, string>,
    ): Promise<{ packages: ResolvedPackage[]; diagnostics: VuTeXDiagnostic[] }> {
        const packages: ResolvedPackage[] = [];
        const diagnostics: VuTeXDiagnostic[] = [];
        const slots = Object.keys(declared).sort((left, right) => left.localeCompare(right, "en"));

        for (const slot of slots) {
            if (!isPackageSlot(slot)) {
                diagnostics.push(
                    packageDiagnostic("VUTEX_PACKAGE_SLOT_UNKNOWN", `unknown package slot: ${slot}`, { slot }),
                );
                continue;
            }
            const name = declared[slot];
            if (isPathSource(name)) {
                const resolved = await this.resolvePath(slot, name);
                if (resolved.diagnostic) {
                    diagnostics.push(resolved.diagnostic);
                } else if (resolved.package) {
                    packages.push(resolved.package);
                }
                continue;
            }
            if (name.startsWith("@")) {
                diagnostics.push(
                    packageDiagnostic(
                        "VUTEX_PACKAGE_SOURCE_UNAVAILABLE",
                        `registry downloads are not available yet: ${name}`,
                        { slot, name },
                    ),
                );
                continue;
            }
            const builtin = lookupBuiltin(name, slot);
            if (!builtin) {
                const code = slot === "font" || slot === "math-font"
                    ? "VUTEX_PACKAGE_FONT_MISSING"
                    : "VUTEX_PACKAGE_NOT_FOUND";
                diagnostics.push(
                    packageDiagnostic(code, `no built-in ${slot} package named ${name}`, { slot, name }),
                );
                continue;
            }
            packages.push({
                slot,
                name: builtin.id,
                version: builtin.version,
                source: "builtin",
                artifactHash: builtin.artifactHash,
                dependencies: [],
            });
        }
        return { packages, diagnostics };
    }

    private async resolvePath(
        slot: string,
        value: string,
    ): Promise<{ package?: ResolvedPackage; diagnostic?: VuTeXDiagnostic }> {
        const raw = value.startsWith("file:") ? value.slice("file:".length) : value;
        const absolute = resolve(this.projectRoot, raw);
        try {
            const info = await stat(absolute);
            if (!info.isDirectory()) {
                return {
                    diagnostic: packageDiagnostic(
                        "VUTEX_PACKAGE_PATH_MISSING",
                        `${slot} path is not a directory: ${value}`,
                        { slot, path: absolute },
                    ),
                };
            }
            const tree = await hashTree(absolute);
            if (tree.files === 0) {
                return {
                    diagnostic: packageDiagnostic(
                        "VUTEX_PACKAGE_PATH_EMPTY",
                        `${slot} path contains no files: ${value}`,
                        { slot, path: absolute },
                    ),
                };
            }
            const name = relative(this.projectRoot, absolute).replace(/\\/gu, "/");
            return {
                package: {
                    slot,
                    name: name.length > 0 ? name : absolute,
                    version: "local",
                    source: "path",
                    artifactHash: tree.artifactHash,
                    dependencies: [],
                },
            };
        } catch (error) {
            return {
                diagnostic: packageDiagnostic(
                    "VUTEX_PACKAGE_PATH_MISSING",
                    `${slot} path cannot be read: ${value}`,
                    { slot, path: absolute, cause: error instanceof Error ? error.message : String(error) },
                ),
            };
        }
    }

    private checkActivation(
        declared: Record<string, string>,
        resolved: ResolvedPackage[],
        locked: LockedPackage[],
    ): PackageCheck[] {
        const lockedBySlot = new Map(locked.map((item) => [item.slot, item]));
        const resolvedBySlot = new Map(resolved.map((item) => [item.slot, item]));
        const missing = Object.keys(declared).filter((slot) => !lockedBySlot.has(slot));
        const stale = locked.filter((item) => !(item.slot in declared)).map((item) => item.slot);
        const drifted = Object.keys(declared)
            .filter((slot) => lockedBySlot.has(slot))
            .filter((slot) => {
                const lockedItem = lockedBySlot.get(slot);
                const resolvedItem = resolvedBySlot.get(slot);
                return lockedItem && resolvedItem && lockedItem.version !== resolvedItem.version;
            });

        const checks: PackageCheck[] = [];
        if (missing.length === 0 && stale.length === 0) {
            checks.push(passedCheck("activation", "every declared slot is locked and active"));
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

        checks.push(
            drifted.length === 0
                ? passedCheck("version", "locked versions match resolved packages")
                : failedCheck(
                    "version",
                    "VUTEX_PACKAGE_VERSION_DRIFT",
                    `manifest and lock disagree for: ${drifted.join(", ")}`,
                ),
        );
        return checks;
    }

    private checkIntegrity(locked: LockedPackage[]): PackageCheck[] {
        const broken = locked.filter((item) => contentDigest(item) !== item.contentHash);
        return broken.length === 0
            ? [passedCheck("integrity", "lockfile record digests match their records")]
            : [
                failedCheck(
                    "integrity",
                    "VUTEX_PACKAGE_HASH_MISMATCH",
                    `record digest mismatch for: ${broken.map((item) => item.slot).join(", ")}`,
                ),
            ];
    }

    private checkArtifacts(resolved: ResolvedPackage[], locked: LockedPackage[]): PackageCheck[] {
        const resolvedBySlot = new Map(resolved.map((item) => [item.slot, item]));
        const broken = locked
            .filter((item) => resolvedBySlot.has(item.slot))
            .filter((item) => resolvedBySlot.get(item.slot)?.artifactHash !== item.artifactHash);
        return broken.length === 0
            ? [passedCheck("artifacts", "resource artifact hashes match their sources")]
            : [
                failedCheck(
                    "artifacts",
                    "VUTEX_PACKAGE_ARTIFACT_MISMATCH",
                    `artifact hash mismatch for: ${broken.map((item) => item.slot).join(", ")}`,
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
