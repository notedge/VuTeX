/**
 * Publish gate: require a green `ci.yml` run on the exact tag-peeled commit.
 *
 * Contract (release gate, not a convenience check):
 * - A failed workflow can never pass. An earlier success on the same SHA must
 *   not mask a later completed failure (re-run, new run, different event).
 * - Missing, skipped, cancelled and timed-out jobs are not success. Delivery
 *   jobs are listed in REQUIRED_JOBS and verified one by one on the accepted run.
 * - Only runs whose headSha equals the bound tag SHA are considered.
 *
 * Why not `gh run list … -q '.[0].conclusion'`?
 * - Tag push can race dev CI (run still queued → "missing").
 * - A newer failed re-run hides an older success on the same SHA.
 */

import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO = process.env.GITHUB_REPOSITORY || process.env.VMZ_GITHUB_REPO || 'doki-land/vmz-framework';
const TAG_SHA = String(process.env.TAG_SHA || process.argv.find((a) => a.startsWith('--sha='))?.slice(6) || '')
    .trim()
    .toLowerCase();
const WORKFLOW = process.env.CI_WORKFLOW_FILE || 'ci.yml';
const TIMEOUT_MS = Number(process.env.CI_WAIT_TIMEOUT_MS || 45 * 60 * 1000);
const INTERVAL_MS = Number(process.env.CI_WAIT_INTERVAL_MS || 20 * 1000);

// Every job that proves the release artifacts are usable. `Build Runtimes` alone
// is not enough: format, unit/example/browser and runtime gates all ship evidence.
export const REQUIRED_JOBS = ['Build Runtimes', 'Format Check', 'Build and Test', 'UI and Browser Production', 'Slim and Runtime Quality'];

const ACTIVE_STATUSES = new Set(['queued', 'in_progress', 'pending', 'requested', 'waiting']);

/**
 * Decide the gate from run metadata only (no I/O), so fixtures can drive it.
 *
 * @param {object} input
 * @param {string} input.sha bound tag SHA (40-char hex, lower-case)
 * @param {Array<object>} input.runs `gh run list` entries, any order
 * @param {(runId: number) => Array<{name: string, conclusion: string|null, status?: string}>} [input.jobsByRunId]
 * @returns {{ok: boolean, state: string, runId?: number, job?: string, conclusion?: string|null, active?: number, event?: string, displayTitle?: string}}
 */
export function evaluateGate({ sha, runs, jobsByRunId }) {
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

    const jobs = jobsByRunId ? jobsByRunId(newest.databaseId) ?? [] : [];
    const byName = new Map(jobs.map((job) => [job.name, job]));
    for (const name of REQUIRED_JOBS) {
        const job = byName.get(name);
        if (!job) {
            return { ok: false, state: 'missing-job', runId: newest.databaseId, job: name };
        }
        if (job.conclusion !== 'success') {
            return {
                ok: false,
                state: 'job-not-success',
                runId: newest.databaseId,
                job: name,
                conclusion: job.conclusion ?? job.status ?? null,
            };
        }
    }

    return {
        ok: true,
        state: 'success',
        runId: newest.databaseId,
        event: newest.event,
        displayTitle: newest.displayTitle,
    };
}

export function describeFailure(result) {
    switch (result.state) {
        case 'run-not-success':
            return `newest completed ${WORKFLOW} run on ${TAG_SHA} is ${result.conclusion} (run ${result.runId}); required jobs on a failed workflow never satisfy the release gate`;
        case 'missing-job':
            return `required job "${result.job}" is missing from run ${result.runId} on ${TAG_SHA}`;
        case 'job-not-success':
            return `required job "${result.job}" in run ${result.runId} concluded ${result.conclusion} (missing/skipped/cancelled/timeout is not success)`;
        case 'no-runs':
            return `no ${WORKFLOW} run found for ${TAG_SHA}`;
        case 'no-completed':
            return `no completed ${WORKFLOW} run for ${TAG_SHA}`;
        default:
            return `gate not satisfied (state=${result.state})`;
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

function listRuns() {
    const raw = gh([
        'run',
        'list',
        '--repo',
        REPO,
        '--commit',
        TAG_SHA,
        '--workflow',
        WORKFLOW,
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

function jobsByRunId(runId) {
    const raw = gh(['api', `repos/${REPO}/actions/runs/${runId}/jobs?per_page=100`]);
    try {
        return (JSON.parse(raw).jobs ?? []).map((job) => ({ name: job.name, conclusion: job.conclusion, status: job.status }));
    } catch {
        fail(`invalid jobs json for run ${runId}: ${raw.slice(0, 200)}`);
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
        const runs = listRuns();
        const result = evaluateGate({ sha: TAG_SHA, runs, jobsByRunId });

        if (result.ok) {
            console.log(`CI success confirmed for ${TAG_SHA} (run ${result.runId} "${result.displayTitle}" event=${result.event})`);
            return;
        }

        // Only "still settling" states wait; a decided failure fails immediately.
        if (result.state === 'active' || result.state === 'no-runs' || result.state === 'no-completed') {
            if (Date.now() >= deadline) {
                fail(`timed out after ${TIMEOUT_MS}ms waiting for green ${WORKFLOW} on ${TAG_SHA} (state=${result.state})`);
            }
            console.log(`[${attempt}] ${describeFailure(result)} — waiting…`);
            await sleep(INTERVAL_MS);
            continue;
        }

        fail(describeFailure(result));
    }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
    main().catch((e) => fail(e instanceof Error ? e.message : String(e)));
}
