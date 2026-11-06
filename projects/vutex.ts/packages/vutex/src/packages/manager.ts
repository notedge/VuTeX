import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, readdir, rename, stat, writeFile } from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";

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
import {
    downloadRegistryFile,
    fetchRegistryMetadata,
    nodeTransport,
    parseRegistrySpec,
    satisfiesRange,
    selectVersion,
    type RegistryFile,
    type RegistryMetadata,
    type RegistryTransport,
    type RegistryVersion,
} from "./registry.js";

export type ResolvedPackage = Omit<LockedPackage, "contentHash"> & {
    /** Files a registry version downloads, absent for builtin and path sources. */
    registryFiles?: RegistryFile[];
    /** Metadata URL that named the selected version. */
    metadataUrl?: string;
};

export type PackageManagerOptions = {
    /** Network client, injectable so tests never reach a real registry. */
    transport?: RegistryTransport;
    /** Registry base URL that wins over `VUTEX_REGISTRY`. */
    registry?: string;
    /** Environment to read `VUTEX_REGISTRY` from. */
    environment?: NodeJS.ProcessEnv;
};

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
    constructor(
        private readonly projectRoot: string,
        private readonly options: PackageManagerOptions = {},
    ) {}

    /** Registry base URL: option, then manifest `[registry] url`, then env. */
    private async registryBase(): Promise<string | undefined> {
        const manifest = await readManifest(this.projectRoot);
        const environment = this.options.environment ?? process.env;
        const candidate = this.options.registry ?? manifest?.registry?.url ?? environment.VUTEX_REGISTRY;
        return typeof candidate === "string" && candidate.trim().length > 0 ? candidate.trim() : undefined;
    }

    /** Content addressed cache path for one downloaded registry package. */
    private registryCachePath(slot: string, name: string, artifactHash: string): string {
        const digest = artifactHash.startsWith("sha256:") ? artifactHash.slice("sha256:".length) : artifactHash;
        return join(this.projectRoot, ".vutex", "packages", slot, encodeURIComponent(name), digest);
    }

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
        await this.resolveAndInstall(options, "install");
    }

    private async resolveAndInstall(
        options: PackageInstallOptions,
        mode: "install" | "update",
    ): Promise<void> {
        const declared = await this.declaredPackages(options);
        const plan = await this.resolvePlan(declared, mode);
        const errors = plan.diagnostics.filter((item) => item.severity === "error");
        if (errors.length > 0) {
            throw new PackageResolveError(errors);
        }

        await mkdir(join(this.projectRoot, ".vutex", "packages"), { recursive: true });
        const failures: VuTeXDiagnostic[] = [];
        const entries: LockedPackage[] = [];
        for (const item of plan.packages) {
            let artifactHash = item.artifactHash;
            if (item.source.startsWith("registry+")) {
                // A package already present in the content addressed cache is
                // not downloaded again, so an install stays offline once the
                // lock has been satisfied.
                if (await this.cachedRegistryArtifact(item)) {
                    artifactHash = item.artifactHash;
                } else {
                    let downloadable = item;
                    if (item.registryFiles === undefined) {
                        const hydrated = await this.hydratePinnedPackage(item);
                        if (hydrated.diagnostic) {
                            failures.push(hydrated.diagnostic);
                            continue;
                        }
                        downloadable = hydrated.package as ResolvedPackage;
                    }
                    const downloaded = await this.downloadRegistryPackage(downloadable);
                    if (downloaded.diagnostic) {
                        failures.push(downloaded.diagnostic);
                        continue;
                    }
                    artifactHash = downloaded.artifactHash;
                }
            }
            const entry: Omit<LockedPackage, "contentHash"> = {
                slot: item.slot,
                name: item.name,
                version: item.version,
                source: item.source,
                artifactHash,
                dependencies: item.dependencies,
            };
            entries.push({ ...entry, contentHash: contentDigest(entry) });
        }
        if (failures.length > 0) {
            throw new PackageResolveError(failures);
        }
        await writeLockfile(this.projectRoot, { version: 1, packages: entries });
    }

    /** Whether the content addressed cache already holds this exact artifact. */
    private async cachedRegistryArtifact(item: ResolvedPackage): Promise<boolean> {
        if (item.artifactHash === undefined) {
            return false;
        }
        const cache = this.registryCachePath(item.slot, item.name, item.artifactHash);
        try {
            const tree = await hashTree(cache);
            return tree.artifactHash === item.artifactHash;
        } catch {
            return false;
        }
    }

    /**
     * A locked registry package whose cache went missing still knows its exact
     * version. Fetch that version's metadata so the files can be downloaded
     * without letting a newer version replace the pin.
     */
    private async hydratePinnedPackage(
        item: ResolvedPackage,
    ): Promise<{ package?: ResolvedPackage; diagnostic?: VuTeXDiagnostic }> {
        const base = await this.registryBase();
        if (base === undefined) {
            return {
                diagnostic: packageDiagnostic(
                    "VUTEX_PACKAGE_REGISTRY_UNCONFIGURED",
                    `no registry is configured to restore ${item.slot} ${item.name}`,
                    { slot: item.slot, name: item.name },
                ),
            };
        }
        const fetched = await fetchRegistryMetadata(this.options.transport ?? nodeTransport, base, item.slot, item.name);
        if (fetched.diagnostic || fetched.metadata === undefined) {
            return { diagnostic: fetched.diagnostic };
        }
        const version = fetched.metadata.versions.find((entry) => entry.version === item.version);
        if (version === undefined) {
            return {
                diagnostic: packageDiagnostic(
                    "VUTEX_PACKAGE_VERSION_NO_MATCH",
                    `${item.slot} ${item.name}@${item.version} is locked but no longer published`,
                    { slot: item.slot, name: item.name, version: item.version },
                ),
            };
        }
        return {
            package: this.registryPackage(item.slot, item.name, version, base, fetched.metadataUrl),
        };
    }

    /**
     * Download one registry version into a staging tree and move it into the
     * content addressed cache.
     *
     * The cache is never mutated in place: a staged tree either matches the
     * address already present or lands under a fresh one, so a failed download
     * cannot poison a later activation.
     */
    private async downloadRegistryPackage(
        item: ResolvedPackage,
    ): Promise<{ artifactHash?: string; diagnostic?: VuTeXDiagnostic }> {
        const files = item.registryFiles ?? [];
        if (files.length === 0) {
            return { artifactHash: item.artifactHash };
        }
        const staging = join(this.projectRoot, ".vutex", "staging", randomUUID());
        await mkdir(staging, { recursive: true });
        const transport = this.options.transport ?? nodeTransport;

        for (const file of files) {
            const target = resolve(staging, file.path);
            const relativePath = relative(staging, target);
            if (relativePath.length === 0 || relativePath.startsWith("..") || isAbsolute(relativePath)) {
                return {
                    diagnostic: packageDiagnostic(
                        "VUTEX_PACKAGE_METADATA_INVALID",
                        `package file escapes its root: ${file.path}`,
                        { path: file.path },
                    ),
                };
            }
            let url: string;
            try {
                url = new URL(file.url, item.metadataUrl).toString();
            } catch {
                return {
                    diagnostic: packageDiagnostic(
                        "VUTEX_PACKAGE_METADATA_INVALID",
                        `package file url is invalid: ${file.url}`,
                        { path: file.path, url: file.url },
                    ),
                };
            }
            const downloaded = await downloadRegistryFile(transport, url, file.path);
            if (downloaded.diagnostic || downloaded.bytes === undefined) {
                return { diagnostic: downloaded.diagnostic };
            }
            const bytes = downloaded.bytes;
            if (file.sha256 !== undefined) {
                const expected = file.sha256.startsWith("sha256:") ? file.sha256.slice("sha256:".length) : file.sha256;
                const actual = createHash("sha256").update(bytes).digest("hex");
                if (actual !== expected.toLowerCase()) {
                    return {
                        diagnostic: packageDiagnostic(
                            "VUTEX_PACKAGE_FILE_HASH_MISMATCH",
                            `downloaded ${file.path} does not match its sha256`,
                            { path: file.path, url, expected, actual },
                        ),
                    };
                }
            }
            await mkdir(dirname(target), { recursive: true });
            await writeFile(target, bytes);
        }

        const tree = await hashTree(staging);
        const computed = tree.files > 0 ? tree.artifactHash : undefined;
        if (item.artifactHash !== undefined && computed !== item.artifactHash) {
            return {
                diagnostic: packageDiagnostic(
                    "VUTEX_PACKAGE_ARTIFACT_MISMATCH",
                    `${item.slot} ${item.name} tree does not match the registry artifact hash`,
                    { slot: item.slot, expected: item.artifactHash, actual: computed },
                ),
            };
        }
        const artifactHash = computed ?? item.artifactHash;
        if (artifactHash === undefined || computed === undefined) {
            return { artifactHash };
        }
        const cache = this.registryCachePath(item.slot, item.name, artifactHash);
        try {
            const existing = await hashTree(cache);
            if (existing.artifactHash === artifactHash) {
                return { artifactHash };
            }
        } catch {
            // The cache is absent, so the staged tree can take the address.
        }
        await mkdir(dirname(cache), { recursive: true });
        try {
            await rename(staging, cache);
        } catch (error) {
            return {
                diagnostic: packageDiagnostic(
                    "VUTEX_PACKAGE_CACHE_CORRUPT",
                    `${item.slot} cache is occupied by a different tree: ${error instanceof Error ? error.message : String(error)}`,
                    { slot: item.slot, cache },
                ),
            };
        }
        return { artifactHash };
    }

    async update(): Promise<void> {
        await this.resolveAndInstall({}, "update");
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
        // Doctor resolves as if updating, so the drift check still sees what
        // the declared ranges would pick today.
        const plan = await this.resolvePlan(declared, "update");

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
            checks.push(...(await this.checkCache(lock.packages)));
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

        const plan = await this.resolvePlan(declared, "activate");
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
            // Registry packages are activated from the content addressed cache.
            // A render never reaches the network and never trusts the lock alone.
            if (locked.source.startsWith("registry+") && locked.artifactHash !== undefined) {
                const cache = this.registryCachePath(slot, locked.name, locked.artifactHash);
                let tree;
                try {
                    tree = await hashTree(cache);
                } catch {
                    diagnostics.push(
                        packageDiagnostic(
                            "VUTEX_PACKAGE_ARTIFACT_MISSING",
                            `${slot} is locked but not downloaded, run \`vutex package install\``,
                            { slot, name: locked.name, version: locked.version },
                        ),
                    );
                    continue;
                }
                if (tree.artifactHash !== locked.artifactHash) {
                    diagnostics.push(
                        packageDiagnostic(
                            "VUTEX_PACKAGE_ARTIFACT_MISMATCH",
                            `${slot} cache does not match ${locked.name}`,
                            { slot, expected: locked.artifactHash, actual: tree.artifactHash },
                        ),
                    );
                    continue;
                }
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
        mode: "install" | "update" | "activate" = "install",
    ): Promise<{ packages: ResolvedPackage[]; diagnostics: VuTeXDiagnostic[] }> {
        const diagnostics: VuTeXDiagnostic[] = [];
        const packages = new Map<string, ResolvedPackage>();
        const rangesBySlot = new Map<string, string[]>();
        const metadataByKey = new Map<string, RegistryMetadata>();
        const metadataUrls = new Map<string, string | undefined>();
        const expanded = new Set<string>();
        const pinnedSlots = new Set<string>();
        const registryBase = await this.registryBase();
        const transport = this.options.transport ?? nodeTransport;

        // `install` reproduces the lockfile, so an unchanged declaration never
        // silently upgrades to a newer registry version. `update` ignores the
        // pins and resolves the declared ranges again.
        const pins = new Map<string, LockedPackage>();
        if (mode === "install") {
            const lock = await readLockfile(this.projectRoot);
            for (const item of lock?.packages ?? []) {
                pins.set(item.slot, item);
            }
        }

        type Requirement = { slot: string; name: string; range: string; from?: string; registry?: boolean };
        const queue: Requirement[] = [];
        for (const slot of Object.keys(declared).sort((left, right) => left.localeCompare(right, "en"))) {
            const value = declared[slot];
            if (isPathSource(value)) {
                queue.push({ slot, name: value, range: "*" });
            } else {
                const spec = parseRegistrySpec(value);
                // An explicit range asks the registry even when the id also
                // names a built-in preset, so versioned packages stay usable.
                queue.push({ slot, name: spec.name, range: spec.range, registry: spec.range !== "*" });
            }
        }

        const keyOf = (slot: string, name: string): string => `${slot}\u0000${name}`;
        const expand = (slot: string, name: string, version: RegistryVersion): void => {
            const key = `${keyOf(slot, name)}\u0000${version.version}`;
            if (expanded.has(key)) {
                return;
            }
            expanded.add(key);
            for (const dependency of version.dependencies) {
                queue.push({
                    slot: dependency.slot,
                    name: dependency.name,
                    range: dependency.range ?? "*",
                    from: `${slot}:${name}@${version.version}`,
                });
            }
        };

        /**
         * Reuse the version the lockfile already pinned, as long as it still
         * satisfies the requirement. Reused versions keep their recorded
         * dependencies, so a plain install touches neither metadata nor files.
         */
        const tryPin = (requirement: Requirement): boolean => {
            if (mode !== "install") {
                return false;
            }
            const pin = pins.get(requirement.slot);
            if (pin === undefined || pin.name !== requirement.name) {
                return false;
            }
            if (!pin.source.startsWith("registry+")) {
                return false;
            }
            // A bare id prefers the built-in preset when one exists.
            if (
                requirement.registry !== true &&
                isPackageSlot(pin.slot) &&
                lookupBuiltin(pin.name, pin.slot) !== undefined
            ) {
                return false;
            }
            if (!satisfiesRange(pin.version, requirement.range)) {
                return false;
            }
            const existing = packages.get(requirement.slot);
            if (existing !== undefined) {
                return existing.name === pin.name && existing.version === pin.version;
            }
            packages.set(requirement.slot, {
                slot: pin.slot,
                name: pin.name,
                version: pin.version,
                source: pin.source,
                artifactHash: pin.artifactHash,
                dependencies: [...pin.dependencies],
            });
            pinnedSlots.add(requirement.slot);
            return true;
        };

        while (queue.length > 0) {
            const requirement = queue.shift() as Requirement;
            const { slot, name } = requirement;
            const range = requirement.range.length > 0 ? requirement.range : "*";
            if (!isPackageSlot(slot)) {
                diagnostics.push(
                    packageDiagnostic("VUTEX_PACKAGE_SLOT_UNKNOWN", `unknown package slot: ${slot}`, { slot }),
                );
                continue;
            }

            const existing = packages.get(slot);
            if (existing) {
                if (existing.name !== name) {
                    diagnostics.push(
                        packageDiagnostic(
                            "VUTEX_PACKAGE_DEPENDENCY_CONFLICT",
                            `${slot} is required as both ${existing.name} and ${name}${requirement.from ? ` by ${requirement.from}` : ""}`,
                            { slot, names: [existing.name, name], from: requirement.from },
                        ),
                    );
                    continue;
                }
                if (existing.source === "builtin" || existing.source === "path") {
                    if (range !== "*") {
                        diagnostics.push(
                            packageDiagnostic(
                                "VUTEX_PACKAGE_VERSION_NO_MATCH",
                                `${existing.name} is a ${existing.source} package and takes no version range`,
                                { slot, name, range },
                            ),
                        );
                    }
                    continue;
                }
                // A second requirement can rule out the locked version. Drop
                // the pin and resolve the registry with both ranges instead.
                if (pinnedSlots.has(slot) && !satisfiesRange(existing.version, range)) {
                    packages.delete(slot);
                    pinnedSlots.delete(slot);
                    queue.unshift(requirement);
                    continue;
                }
                const ranges = rangesBySlot.get(slot) ?? [];
                if (!ranges.includes(range)) {
                    ranges.push(range);
                }
                rangesBySlot.set(slot, ranges);
                const found = metadataByKey.get(keyOf(slot, name));
                if (!found) {
                    continue;
                }
                const selected = selectVersion(found, ranges);
                if (selected.diagnostic) {
                    diagnostics.push(selected.diagnostic);
                    continue;
                }
                const version = selected.version as RegistryVersion;
                if (version.version !== existing.version) {
                    packages.set(
                        slot,
                        this.registryPackage(slot, name, version, registryBase, metadataUrls.get(keyOf(slot, name))),
                    );
                    expand(slot, name, version);
                }
                continue;
            }

            if (isPathSource(name)) {
                const resolved = await this.resolvePath(slot, name);
                if (resolved.diagnostic) {
                    diagnostics.push(resolved.diagnostic);
                } else if (resolved.package) {
                    packages.set(slot, resolved.package);
                }
                continue;
            }

            if (tryPin(requirement)) {
                continue;
            }

            const builtin = requirement.registry === true ? undefined : lookupBuiltin(name, slot);
            if (builtin) {
                if (range !== "*") {
                    diagnostics.push(
                        packageDiagnostic(
                            "VUTEX_PACKAGE_VERSION_NO_MATCH",
                            `${name} is a built-in ${slot} package and takes no version range`,
                            { slot, name, range },
                        ),
                    );
                    continue;
                }
                packages.set(slot, {
                    slot,
                    name: builtin.id,
                    version: builtin.version,
                    source: "builtin",
                    artifactHash: builtin.artifactHash,
                    dependencies: [],
                });
                continue;
            }

            // Registry packages activate from the lockfile and the content
            // addressed cache, so a render never depends on the network.
            if (mode === "activate") {
                continue;
            }

            if (registryBase === undefined) {
                const scoped = name.startsWith("@");
                diagnostics.push(
                    scoped
                        ? packageDiagnostic(
                            "VUTEX_PACKAGE_REGISTRY_UNCONFIGURED",
                            `no registry is configured for ${name}, set [registry] url or VUTEX_REGISTRY`,
                            { slot, name },
                        )
                        : packageDiagnostic(
                            slot === "font" || slot === "math-font" ? "VUTEX_PACKAGE_FONT_MISSING" : "VUTEX_PACKAGE_NOT_FOUND",
                            `no built-in ${slot} package named ${name}`,
                            { slot, name },
                        ),
                );
                continue;
            }

            const key = keyOf(slot, name);
            let found = metadataByKey.get(key);
            if (!found) {
                const fetched = await fetchRegistryMetadata(transport, registryBase, slot, name);
                if (fetched.diagnostic) {
                    diagnostics.push(fetched.diagnostic);
                    continue;
                }
                found = fetched.metadata as RegistryMetadata;
                metadataByKey.set(key, found);
                metadataUrls.set(key, fetched.metadataUrl);
            }
            const ranges = rangesBySlot.get(slot) ?? [];
            if (!ranges.includes(range)) {
                ranges.push(range);
            }
            rangesBySlot.set(slot, ranges);
            const selected = selectVersion(found, ranges);
            if (selected.diagnostic) {
                diagnostics.push(selected.diagnostic);
                continue;
            }
            const version = selected.version as RegistryVersion;
            packages.set(slot, this.registryPackage(slot, name, version, registryBase, metadataUrls.get(key)));
            expand(slot, name, version);
        }

        // Dependency references name the versions the plan actually chose. Reused
        // lock entries keep their recorded references untouched.
        for (const item of packages.values()) {
            if (!item.source.startsWith("registry+") || pinnedSlots.has(item.slot)) {
                continue;
            }
            const found = metadataByKey.get(keyOf(item.slot, item.name));
            const version = found?.versions.find((entry) => entry.version === item.version);
            item.dependencies = (version?.dependencies ?? [])
                .map((dependency) => packages.get(dependency.slot))
                .filter((dependency): dependency is ResolvedPackage => dependency !== undefined)
                .map((dependency) => `${dependency.slot}:${dependency.name}@${dependency.version}`)
                .sort((left, right) => left.localeCompare(right, "en"));
        }

        // A reused lock entry brings its dependency references along, so their
        // targets must enter the plan even when nothing declares them.
        const pending = [...pinnedSlots];
        while (pending.length > 0) {
            const slot = pending.pop() as string;
            const item = packages.get(slot);
            if (item === undefined) {
                continue;
            }
            for (const reference of item.dependencies) {
                const separator = reference.indexOf(":");
                const at = reference.lastIndexOf("@");
                if (separator <= 0 || at <= separator) {
                    diagnostics.push(
                        packageDiagnostic(
                            "VUTEX_PACKAGE_LOCK_INCOMPLETE",
                            `${slot} locks an unreadable dependency reference: ${reference}`,
                            { slot, reference },
                        ),
                    );
                    continue;
                }
                const refSlot = reference.slice(0, separator);
                const refName = reference.slice(separator + 1, at);
                const refVersion = reference.slice(at + 1);
                const target = packages.get(refSlot);
                if (target !== undefined) {
                    if (target.name !== refName || target.version !== refVersion) {
                        diagnostics.push(
                            packageDiagnostic(
                                "VUTEX_PACKAGE_DEPENDENCY_CONFLICT",
                                `${refSlot} is locked as ${refName}@${refVersion} but the plan resolved ${target.name}@${target.version}`,
                                { slot: refSlot, expected: `${refName}@${refVersion}`, actual: `${target.name}@${target.version}` },
                            ),
                        );
                    }
                    continue;
                }
                const pin = pins.get(refSlot);
                if (pin === undefined || pin.name !== refName || pin.version !== refVersion) {
                    diagnostics.push(
                        packageDiagnostic(
                            "VUTEX_PACKAGE_LOCK_INCOMPLETE",
                            `${slot} depends on ${reference} which the lockfile does not contain`,
                            { slot, reference },
                        ),
                    );
                    continue;
                }
                packages.set(refSlot, {
                    slot: pin.slot,
                    name: pin.name,
                    version: pin.version,
                    source: pin.source,
                    artifactHash: pin.artifactHash,
                    dependencies: [...pin.dependencies],
                });
                if (pin.source.startsWith("registry+")) {
                    pinnedSlots.add(refSlot);
                    pending.push(refSlot);
                }
            }
        }

        return { packages: [...packages.values()], diagnostics };
    }

    private registryPackage(
        slot: string,
        name: string,
        version: RegistryVersion,
        registryBase: string | undefined,
        metadataUrl: string | undefined,
    ): ResolvedPackage {
        return {
            slot,
            name,
            version: version.version,
            source: registryBase === undefined ? "registry" : `registry+${registryBase}`,
            artifactHash: version.artifactHash,
            dependencies: [],
            registryFiles: version.files,
            metadataUrl,
        };
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
        // A locked slot that no declaration names is only stale when nothing
        // depends on it. Dependency-only slots stay locked without activating:
        // activation is driven by what the document declares.
        const dependencySlots = new Set<string>();
        for (const item of locked) {
            for (const dependency of item.dependencies) {
                const separator = dependency.indexOf(":");
                if (separator > 0) {
                    dependencySlots.add(dependency.slice(0, separator));
                }
            }
        }
        const missing = Object.keys(declared).filter((slot) => !lockedBySlot.has(slot));
        const stale = locked
            .filter((item) => !(item.slot in declared) && !dependencySlots.has(item.slot))
            .map((item) => item.slot);
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
            .filter((item) => {
                const source = resolvedBySlot.get(item.slot) as ResolvedPackage;
                // A registry metadata document may omit the tree hash. The lock
                // keeps the hash `install` computed, so there is nothing to compare.
                if (source.source.startsWith("registry+") && source.artifactHash === undefined) {
                    return false;
                }
                return source.artifactHash !== item.artifactHash;
            });
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

    private async checkCache(locked: LockedPackage[]): Promise<PackageCheck[]> {
        const registry = locked.filter((item) => item.source.startsWith("registry+") && item.artifactHash !== undefined);
        if (registry.length === 0) {
            return [];
        }
        const broken: string[] = [];
        for (const item of registry) {
            const cache = this.registryCachePath(item.slot, item.name, item.artifactHash as string);
            try {
                const tree = await hashTree(cache);
                if (tree.artifactHash !== item.artifactHash) {
                    broken.push(item.slot);
                }
            } catch {
                broken.push(item.slot);
            }
        }
        return broken.length === 0
            ? [passedCheck("cache", `${registry.length} registry package(s) cached and intact`)]
            : [
                failedCheck(
                    "cache",
                    "VUTEX_PACKAGE_ARTIFACT_MISSING",
                    `not cached or corrupt: ${broken.join(", ")}`,
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
