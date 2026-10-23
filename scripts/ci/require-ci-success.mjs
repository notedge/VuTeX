/**
 * Publish gate: require green check workflows on the exact tag-peeled commit.
 *
 * Contract (release gate, not a convenience check):
 * - A failed workflow can never pass. An earlier success on the same SHA must
 *   not mask a later completed failure (re-run, new run, different event).
 * - Only runs whose headSha equals the bound tag SHA are considered.
 * - Every workflow in CI_WORKFLOW_FILES must have its newest completed run on
 *   that SHA conclude success. Missing, skipped, cancelled and timed-out runs
 *   are not success.
 *
 * Why not `gh run list … -q '.[0].conclusion'`?
 * - Tag push can race dev CI (run still queued → "missing").
 * - A newer failed re-run hides an older success on the same SHA.
 */

import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO = process.env.GITHUB_REPOSITORY || 'notedge/VuTeX';
const TAG_SHA = String(process.env.TAG_SHA || process.argv.find((a) => a.startsWith('--sha='))?.slice(6) || '')
    .trim()
    .toLowerCase();
const WORKFLOWS = String(process.env.CI_WORKFLOW_FILES || 'check-rust.yml,check-typescript.yml')
    .split(',')
    .map((name) => name.trim())
    .filter(Boolean);
const TIMEOUT_MS = Number(process.env.CI_WAIT_TIMEOUT_MS || 45 * 60 * 1000);
const INTERVAL_MS = Number(process.env.CI_WAIT_INTERVAL_MS || 20 * 1000);

const ACTIVE_STATUSES = new Set(['queued', 'in_progress', 'pending', 'requested', 'waiting']);

/**
 * Decide the gate for one workflow from run metadata only (no I/O).
 *
 * @param {{sha: string, workflow: string, runs: Array<object>}} input
 * @returns {{ok: boolean, state: string, runId?: number, conclusion?: string|null, active?: number, event?: string, displayTitle?: string}}
 */
export function evaluateGate({ sha, workflow, runs }) {
    const scoped = (runs ?? []).filter((run) => String(run?.headSha ?? '').toLowerCase() === sha);
    if (scoped.length === 0) {
        return { ok: false, state: 'no-runs' };
    }

    const active = scoped.filter((run) => ACTIVE_STATUSES.has(String(run.status)));
    if (active.length > 0) {
        return { ok: false, state: 'active', active: active.length };
    }

    const completed = scoped
        .filter((run) => String(run.status) === 'completed')
        .sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt) || (b.databaseId ?? 0) - (a.databaseId ?? 0));
    if (completed.length === 0) {
        return { ok: false, state: 'no-completed' };
    }

    // The newest completed run on the bound SHA is the only run that can pass.
    const newest = completed[0];
    if (newest.conclusion !== 'success') {
        return {
            ok: false,
            state: 'run-not-success',
            runId: newest.databaseId,
            conclusion: newest.conclusion ?? null,
        };
    }

    return {
        ok: true,
        state: 'success',
        runId: newest.databaseId,
        event: newest.event,
        displayTitle: newest.displayTitle,
        workflow,
    };
}

/**
 * Describe why the gate is not satisfied.
 *
 * @param {{state: string, runId?: number, conclusion?: string|null}} result
 * @param {string} workflow
 * @returns {string}
 */
export function describeFailure(result, workflow) {
    switch (result.state) {
        case 'run-not-success':
            return `newest completed ${workflow} run on ${TAG_SHA} is ${result.conclusion} (run ${result.runId}); a failed workflow never satisfies the release gate`;
        case 'active':
            return `${workflow} run on ${TAG_SHA} is still active (${result.active})`;
        case 'no-runs':
            return `no ${workflow} run found for ${TAG_SHA}`;
        case 'no-completed':
            return `no completed ${workflow} run for ${TAG_SHA}`;
        default:
            return `gate not satisfied (workflow=${workflow} state=${result.state})`;
    }
}

function fail(msg) {
    console.error(`require-ci-success: ${msg}`);
    process.exit(1);
}

function gh(args) {
    const r = spawnSync('gh', args, {
        encoding: 'utf8',
        shell: false,
        env: { ...process.env, GH_TOKEN: process.env.GH_TOKEN || process.env.GITHUB_TOKEN || '' },
    });
    if (r.status !== 0) {
        fail(r.stderr?.trim() || r.stdout?.trim() || `gh ${args.join(' ')} exit ${r.status}`);
    }
    return String(r.stdout ?? '').trim();
}

function listRuns(workflow) {
    const raw = gh([
        'run',
        'list',
        '--repo',
        REPO,
        '--commit',
        TAG_SHA,
        '--workflow',
        workflow,
        '--limit',
        '20',
        '--json',
        'databaseId,status,conclusion,createdAt,displayTitle,event,headSha',
    ]);
    try {
        return JSON.parse(raw);
    } catch {
        fail(`invalid gh json: ${raw.slice(0, 200)}`);
    }
}

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
    if (! /^[0-9a-f]{40}$/.test(TAG_SHA)) {
        fail(`TAG_SHA must be 40-char hex (got '${TAG_SHA || '(empty)'}')`);
    }

    const deadline = Date.now() + TIMEOUT_MS;
    let attempt = 0;

    for (;;) {
        attempt += 1;
        let waiting = false;

        for (const workflow of WORKFLOWS) {
            const result = evaluateGate({ sha: TAG_SHA, workflow, runs: listRuns(workflow) });

            if (result.ok) {
                console.log(`CI success confirmed for ${TAG_SHA}: ${workflow} (run ${result.runId} "${result.displayTitle}")`);
                continue;
            }

            // Only "still settling" states wait; a decided failure fails immediately.
            if (result.state === 'active' || result.state === 'no-runs' || result.state === 'no-completed') {
                if (Date.now() >= deadline) {
                    fail(`timed out after ${TIMEOUT_MS}ms waiting for green ${workflow} on ${TAG_SHA} (state=${result.state})`);
                }
                console.log(`[${attempt}] ${describeFailure(result, workflow)} — waiting…`);
                waiting = true;
            } else {
                fail(describeFailure(result, workflow));
            }
        }

        if (!waiting) return;
        await sleep(INTERVAL_MS);
    }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
    main().catch((e) => fail(e instanceof Error ? e.message : String(e)));
}
