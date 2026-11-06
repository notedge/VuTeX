import http from "node:http";
import https from "node:https";

import type { VuTeXDiagnostic } from "../types.js";

/** Numeric part of a version triple. Prerelease tags order by presence only. */
export type ParsedVersion = { major: number; minor: number; patch: number };

export type VersionRange = {
    operator: "^" | "~" | ">=" | ">" | "<=" | "<" | "=" | "*";
    version?: ParsedVersion;
    raw: string;
};

/** One downloadable file inside a registry package version. */
export type RegistryFile = {
    /** Path relative to the package root in the local cache. */
    path: string;
    /** Absolute or metadata-relative URL. */
    url: string;
    /** Optional SHA-256 of the bytes, verified after download. */
    sha256?: string;
};

/** A package another package version depends on, addressed by slot. */
export type RegistryDependency = {
    slot: string;
    name: string;
    /** Version range, `*` when omitted. */
    range?: string;
};

export type RegistryVersion = {
    version: string;
    files: RegistryFile[];
    dependencies: RegistryDependency[];
    /** Optional SHA-256 of the whole extracted tree. */
    artifactHash?: string;
};

export type RegistryMetadata = {
    slot: string;
    name: string;
    versions: RegistryVersion[];
};

export type RegistrySpec = { name: string; range: string };

/** One registry response: status plus the raw body. */
export type RegistryResponse = { status: number; body: Uint8Array };

/** Registry network client, injectable so tests never leave the process. */
export type RegistryTransport = (url: string) => Promise<RegistryResponse>;

/**
 * `node:http` driven transport with a fresh connection per request.
 *
 * Global `fetch` keeps a dispatcher alive on Windows, which outlives the
 * command and races the process teardown. One short-lived agent per request
 * leaves nothing behind and keeps registry commands deterministic.
 */
export function nodeTransport(url: string): Promise<RegistryResponse> {
    return new Promise((resolve, reject) => {
        const target = new URL(url);
        const client = target.protocol === "https:" ? https : http;
        const request = client.get(target, { agent: false }, (response) => {
            const chunks: Uint8Array[] = [];
            response.on("data", (chunk: Buffer) => chunks.push(chunk));
            response.on("end", () => {
                resolve({ status: response.statusCode ?? 0, body: new Uint8Array(Buffer.concat(chunks)) });
            });
            response.on("error", reject);
        });
        request.on("error", reject);
    });
}

/** Splitting `name@^1.2.0` from the last `@`, so scoped names stay intact. */
export function parseRegistrySpec(value: string): RegistrySpec {
    const at = value.lastIndexOf("@");
    if (at > 0) {
        const range = value.slice(at + 1).trim();
        return { name: value.slice(0, at), range: range.length > 0 ? range : "*" };
    }
    return { name: value, range: "*" };
}

export function parseVersion(raw: string): ParsedVersion | undefined {
    const match = /^v?(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.-]+)?$/u.exec(raw.trim());
    if (!match) {
        return undefined;
    }
    return { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3]) };
}

export function compareVersions(left: ParsedVersion, right: ParsedVersion): number {
    return left.major - right.major || left.minor - right.minor || left.patch - right.patch;
}

export function parseRange(raw: string): VersionRange | undefined {
    const value = raw.trim();
    if (value === "*" || value === "latest" || value.length === 0) {
        return { operator: "*", raw: "*" };
    }
    const match = /^(\^|~|>=|<=|>|<|=)?\s*(v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/u.exec(value);
    if (!match) {
        return undefined;
    }
    const version = parseVersion(match[2]);
    if (!version) {
        return undefined;
    }
    return { operator: (match[1] ?? "=") as VersionRange["operator"], version, raw: value };
}

/** npm-style satisfaction for `*`, `^`, `~`, comparison and exact ranges. */
export function satisfies(version: ParsedVersion, range: VersionRange): boolean {
    if (range.operator === "*") {
        return true;
    }
    const base = range.version;
    if (!base) {
        return false;
    }
    const order = compareVersions(version, base);
    switch (range.operator) {
        case "=":
            return order === 0;
        case ">=":
            return order >= 0;
        case ">":
            return order > 0;
        case "<=":
            return order <= 0;
        case "<":
            return order < 0;
        case "~":
            return order >= 0 && version.major === base.major && version.minor === base.minor;
        case "^":
            if (order < 0) {
                return false;
            }
            if (base.major > 0) {
                return version.major === base.major;
            }
            if (base.minor > 0) {
                return version.major === base.major && version.minor === base.minor;
            }
            return compareVersions(version, base) === 0;
        default:
            return false;
    }
}

/** Highest version satisfying every declared range. */
export function selectVersion(
    metadata: RegistryMetadata,
    ranges: string[],
): { version?: RegistryVersion; versionRange?: VersionRange; diagnostic?: VuTeXDiagnostic } {
    for (const raw of ranges) {
        if (!parseRange(raw)) {
            return {
                diagnostic: {
                    code: "VUTEX_PACKAGE_VERSION_RANGE_INVALID",
                    severity: "error",
                    message: `invalid version range for ${metadata.name}: ${raw}`,
                    stage: "package",
                },
            };
        }
    }
    const parsedRanges = ranges.map((raw) => parseRange(raw) as VersionRange);
    const candidates = [...metadata.versions]
        .map((item) => ({ item, version: parseVersion(item.version) }))
        .filter((entry): entry is { item: RegistryVersion; version: ParsedVersion } => entry.version !== undefined)
        .filter((entry) => parsedRanges.every((range) => satisfies(entry.version, range)))
        .sort((left, right) => compareVersions(right.version, left.version));
    if (candidates.length === 0) {
        return {
            diagnostic: {
                code: "VUTEX_PACKAGE_VERSION_NO_MATCH",
                severity: "error",
                message: `no version of ${metadata.name} satisfies ${ranges.join(", ")}`,
                stage: "package",
                args: { ranges },
            },
        };
    }
    return { version: candidates[0].item };
}

function registryDiagnostic(code: string, message: string, args?: Record<string, unknown>): VuTeXDiagnostic {
    return { code, severity: "error", message, stage: "package", args };
}

function encodeSegment(value: string): string {
    return encodeURIComponent(value);
}

/** Metadata URL for one package: `<base>/<slot>/<name>.json`. */
export function metadataUrl(base: string, slot: string, name: string): string {
    const trimmed = base.replace(/\/+$/u, "");
    const segments = name.split("/").map(encodeSegment).join("/");
    return `${trimmed}/${encodeSegment(slot)}/${segments}.json`;
}

function parseFiles(raw: unknown): RegistryFile[] | undefined {
    if (!Array.isArray(raw)) {
        return undefined;
    }
    const files: RegistryFile[] = [];
    for (const entry of raw) {
        if (typeof entry !== "object" || entry === null) {
            return undefined;
        }
        const record = entry as Record<string, unknown>;
        if (typeof record.path !== "string" || typeof record.url !== "string") {
            return undefined;
        }
        files.push({
            path: record.path,
            url: record.url,
            ...(typeof record.sha256 === "string" ? { sha256: record.sha256 } : {}),
        });
    }
    return files;
}

function parseDependencies(raw: unknown): RegistryDependency[] | undefined {
    if (raw === undefined) {
        return [];
    }
    if (!Array.isArray(raw)) {
        return undefined;
    }
    const dependencies: RegistryDependency[] = [];
    for (const entry of raw) {
        if (typeof entry !== "object" || entry === null) {
            return undefined;
        }
        const record = entry as Record<string, unknown>;
        if (typeof record.slot !== "string" || typeof record.name !== "string") {
            return undefined;
        }
        dependencies.push({
            slot: record.slot,
            name: record.name,
            ...(typeof record.range === "string" ? { range: record.range } : {}),
        });
    }
    return dependencies;
}

/** Validate one metadata document. Unknown fields are ignored on purpose. */
export function parseRegistryMetadata(
    slot: string,
    name: string,
    raw: unknown,
): { metadata?: RegistryMetadata; diagnostic?: VuTeXDiagnostic } {
    if (typeof raw !== "object" || raw === null) {
        return {
            diagnostic: registryDiagnostic("VUTEX_PACKAGE_METADATA_INVALID", `registry metadata for ${name} is not an object`),
        };
    }
    const record = raw as Record<string, unknown>;
    if (record.slot !== undefined && record.slot !== slot) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_METADATA_INVALID",
                `registry metadata for ${name} declares slot ${String(record.slot)} instead of ${slot}`,
            ),
        };
    }
    if (record.name !== undefined && record.name !== name) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_METADATA_INVALID",
                `registry metadata is named ${String(record.name)} instead of ${name}`,
            ),
        };
    }
    if (!Array.isArray(record.versions)) {
        return {
            diagnostic: registryDiagnostic("VUTEX_PACKAGE_METADATA_INVALID", `registry metadata for ${name} has no versions`),
        };
    }
    const versions: RegistryVersion[] = [];
    for (const entry of record.versions) {
        if (typeof entry !== "object" || entry === null) {
            return {
                diagnostic: registryDiagnostic("VUTEX_PACKAGE_METADATA_INVALID", `registry metadata for ${name} has a malformed version`),
            };
        }
        const versionRecord = entry as Record<string, unknown>;
        if (typeof versionRecord.version !== "string") {
            return {
                diagnostic: registryDiagnostic("VUTEX_PACKAGE_METADATA_INVALID", `registry metadata for ${name} has a version without a number`),
            };
        }
        const files = parseFiles(versionRecord.files);
        const dependencies = parseDependencies(versionRecord.dependencies);
        if (!files || !dependencies) {
            return {
                diagnostic: registryDiagnostic(
                    "VUTEX_PACKAGE_METADATA_INVALID",
                    `registry metadata for ${name}@${versionRecord.version} has malformed files or dependencies`,
                ),
            };
        }
        versions.push({
            version: versionRecord.version,
            files,
            dependencies,
            ...(typeof versionRecord.artifactHash === "string" ? { artifactHash: versionRecord.artifactHash } : {}),
        });
    }
    return { metadata: { slot, name, versions } };
}

const decoder = new TextDecoder();

function statusOk(status: number): boolean {
    return status >= 200 && status < 300;
}

/** Fetch and parse one metadata document. Network faults keep the URL. */
export async function fetchRegistryMetadata(
    transport: RegistryTransport,
    base: string,
    slot: string,
    name: string,
): Promise<{ metadata?: RegistryMetadata; metadataUrl?: string; diagnostic?: VuTeXDiagnostic }> {
    const url = metadataUrl(base, slot, name);
    let response: RegistryResponse;
    try {
        response = await transport(url);
    } catch (error) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_REGISTRY_UNREACHABLE",
                `cannot reach registry for ${name}: ${error instanceof Error ? error.message : String(error)}`,
                { url },
            ),
        };
    }
    if (!statusOk(response.status)) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_REGISTRY_UNREACHABLE",
                `registry returned ${response.status} for ${name}`,
                { url, status: response.status },
            ),
        };
    }
    let parsed: unknown;
    try {
        parsed = JSON.parse(decoder.decode(response.body));
    } catch (error) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_METADATA_INVALID",
                `registry metadata for ${name} is not json: ${error instanceof Error ? error.message : String(error)}`,
                { url },
            ),
        };
    }
    const checked = parseRegistryMetadata(slot, name, parsed);
    return { ...checked, metadataUrl: url };
}

export async function downloadRegistryFile(
    transport: RegistryTransport,
    url: string,
    path: string,
): Promise<{ bytes?: Uint8Array; diagnostic?: VuTeXDiagnostic }> {
    let response: RegistryResponse;
    try {
        response = await transport(url);
    } catch (error) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_DOWNLOAD_FAILED",
                `cannot download ${path}: ${error instanceof Error ? error.message : String(error)}`,
                { url, path },
            ),
        };
    }
    if (!statusOk(response.status)) {
        return {
            diagnostic: registryDiagnostic(
                "VUTEX_PACKAGE_DOWNLOAD_FAILED",
                `cannot download ${path}: registry returned ${response.status}`,
                { url, path, status: response.status },
            ),
        };
    }
    return { bytes: response.body };
}
