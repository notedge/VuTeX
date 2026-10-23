/**
 * Bind npm publish version to an existing git tag peel (0.1.13 incident).
 *
 * Provenance contract:
 * - Semver X.Y.Z may publish only when refs/tags/vX.Y.Z exists.
 * - workflow_dispatch must NOT publish from an arbitrary branch HEAD with a free
 *   version input — that produced npm @vmz/vmz@0.1.13 from aaced24a while tag
 *   v0.1.13 pointed at 1c36aa2 and the tag push publish failed.
 * - Outputs the peeled tag SHA so CI jobs checkout that commit only.
 *
 * Usage (CI):
 *   EVENT_NAME=workflow_dispatch INPUT_VERSION=0.1.14 node scripts/ci/bind-publish-tag.mjs
 *   EVENT_NAME=push GITHUB_REF=refs/tags/v0.1.14 GITHUB_SHA=… node scripts/ci/bind-publish-tag.mjs
 *
 * Prints:
 *   version=X.Y.Z
 *   sha=<40-hex>
 * and appends the same keys to $GITHUB_OUTPUT when set.
 */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

function fail(msg) {
    console.error(`bind-publish-tag: ${msg}`);
    process.exit(1);
}

function run(args) {
    const r = spawnSync('git', args, {
        cwd: ROOT,
        encoding: 'utf8',
        shell: false,
    });
    return {
        status: r.status ?? 1,
        stdout: String(r.stdout ?? '').trim(),
        stderr: String(r.stderr ?? '').trim(),
    };
}

function stripV(v) {
    return String(v || '')
        .trim()
        .replace(/^v/i, '');
}

function isSemver(v) {
    return /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(v);
}

const eventName = process.env.EVENT_NAME || process.env.GITHUB_EVENT_NAME || '';
let version = '';

if (eventName === 'workflow_dispatch') {
    version = stripV(process.env.INPUT_VERSION || '');
    if (!version) {
        fail('workflow_dispatch requires INPUT_VERSION (existing tag vX.Y.Z; not a free float over branch HEAD)');
    }
} else if (eventName === 'push') {
    const ref = process.env.GITHUB_REF || '';
    const m = ref.match(/^refs\/tags\/v?(.+)$/);
    if (!m) fail(`push publish expects GITHUB_REF=refs/tags/vX.Y.Z (got ${ref || '(empty)'})`);
    version = stripV(m[1]);
} else {
    // Local / explicit: --version=X.Y.Z
    const fromArg = process.argv.find((a) => a.startsWith('--version='))?.slice('--version='.length);
    version = stripV(fromArg || process.env.INPUT_VERSION || '');
    if (!version) {
        fail('need EVENT_NAME=push|workflow_dispatch or --version=X.Y.Z');
    }
}

if (!isSemver(version)) fail(`version must be semver X.Y.Z (got '${version}')`);

const tag = `v${version}`;

// Ensure tags are visible (shallow checkout / dispatch from branch).
const fetch = run(['fetch', '--tags', '--force', 'origin']);
if (fetch.status !== 0) {
    // Offline / already complete clone: continue if local tag exists.
    console.warn(`bind-publish-tag: git fetch --tags warn: ${fetch.stderr || fetch.stdout || fetch.status}`);
}

const tagRef = run(['rev-parse', '--verify', `refs/tags/${tag}`]);
if (tagRef.status !== 0) {
    fail(
        `tag ${tag} does not exist. Create and push the tag on the intended commit before publish; do not invent a version over branch HEAD (0.1.13 incident).`,
    );
}

const peel = run(['rev-parse', `${tag}^{}`]);
if (peel.status !== 0 || ! /^[0-9a-f]{40}$/i.test(peel.stdout)) {
    fail(`could not peel ${tag}: ${peel.stderr || peel.stdout}`);
}
const tagSha = peel.stdout.toLowerCase();

if (eventName === 'push') {
    const head = String(process.env.GITHUB_SHA || '')
        .trim()
        .toLowerCase();
    if (head && head !== tagSha) {
        fail(`push GITHUB_SHA=${head} does not match peeled ${tag}=${tagSha}`);
    }
}

// For workflow_dispatch: ignore the UI-selected branch HEAD entirely. Downstream
// jobs must checkout refs/tags/v$version @ tagSha.

const lines = [`version=${version}`, `sha=${tagSha}`, `tag=${tag}`];
for (const line of lines) console.log(line);

const outFile = process.env.GITHUB_OUTPUT;
if (outFile) {
    fs.appendFileSync(outFile, `${lines.join('\n')}\n`, 'utf8');
}
