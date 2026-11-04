import { appendFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

import { resultsRoot } from "./ours.mjs";

/**
 * Create a JSONL writer for one comparison library.
 *
 * @param {string} library directory name under `results/crosscheck`, for example `katex`
 */
export function createReport(library) {
    const stamp = new Date().toISOString().replace(/[:.]/g, "-");
    const directory = resolve(resultsRoot, "crosscheck", library);
    mkdirSync(directory, { recursive: true });
    const file = resolve(directory, `${stamp}.jsonl`);
    const counts = { equivalent: 0, diff: 0, skipped: 0 };
    const written = [];

    function write(record) {
        appendFileSync(file, `${JSON.stringify(record)}\n`, "utf8");
        written.push(record);
        counts.diff += 1;
    }

    /**
     * Compare one feature of ours against one or more same-ecosystem references.
     *
     * `theirs` is a list of `{ engine, value }` entries, where `value` may be
     * `null` when that engine has no measurable counterpart for the feature.
     */
    function compare({ caseId, feature, ours, theirs, tolerance, judgment = "tolerance", note }) {
        if (judgment === "record-only") {
            return null;
        }
        if (typeof ours !== "number" || !Number.isFinite(ours)) {
            write({
                case: caseId,
                feature,
                ours,
                theirs,
                tolerance,
                classification: "our-limitation",
                note: note ?? "we do not produce this dimension",
            });
            return "our-limitation";
        }
        const measured = theirs.filter((entry) => typeof entry.value === "number" && Number.isFinite(entry.value));
        if (measured.length === 0) {
            counts.skipped += 1;
            write({
                case: caseId,
                feature,
                ours,
                theirs,
                tolerance,
                classification: "spec-ambiguity",
                note: note ?? "no reference in this ecosystem",
            });
            return "spec-ambiguity";
        }

        const within = (value) => Math.abs(ours - value) <= tolerance;
        if (measured.every((entry) => within(entry.value))) {
            counts.equivalent += 1;
            return null;
        }

        const disagree = measured.some((a, index) =>
            measured.slice(index + 1).some((b) => Math.abs(a.value - b.value) > tolerance * 2));
        const agreeing = measured.filter((entry) => within(entry.value));
        let classification;
        let judgedNote = note ?? "";
        if (disagree) {
            classification = "spec-ambiguity";
            judgedNote ||= "references disagree with each other";
        } else if (measured.length >= 2 && agreeing.length === measured.length - 1) {
            const outlier = measured.find((entry) => !within(entry.value));
            classification = "their-bug";
            judgedNote ||= `${outlier.engine} deviates while the other references agree with us`;
        } else {
            const closest = Math.min(...measured.map((entry) => Math.abs(ours - entry.value)));
            if (closest > tolerance * 2) {
                classification = "real-defect";
                judgedNote ||= "all references agree with each other and disagree with us";
            } else {
                classification = "spec-ambiguity";
                judgedNote ||= "deviation sits between the tolerance and twice the tolerance";
            }
        }

        write({
            case: caseId,
            feature,
            ours,
            theirs: measured,
            tolerance,
            classification,
            note: judgedNote,
        });
        return classification;
    }

    /**
     * Record an exact-value comparison, for features that are not numbers.
     *
     * Equal values count as `equivalent`. A difference is written with the
     * classification the caller justifies in `classification`.
     */
    function compareText({ caseId, feature, ours, theirs, classification, note }) {
        if (ours === theirs) {
            counts.equivalent += 1;
            return null;
        }
        write({
            case: caseId,
            feature,
            ours,
            theirs,
            tolerance: "exact",
            classification,
            note: note ?? "",
        });
        return classification;
    }

    /** Write a free-form evidence line, used for record-only dimensions. */
    function record(payload) {
        write(payload);
    }

    function summary(label) {
        const byClass = {};
        for (const record of written) {
            byClass[record.classification] = (byClass[record.classification] ?? 0) + 1;
        }
        console.log(
            `${label}: ${counts.equivalent} equivalent, ${counts.skipped} no-reference, ` +
            `${counts.diff} diff (${JSON.stringify(byClass)})`,
        );
        console.log(`wrote ${file}`);
        return { file, counts, byClass, records: written };
    }

    return { compare, compareText, record, summary, file };
}
