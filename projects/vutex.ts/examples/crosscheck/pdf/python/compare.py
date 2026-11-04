"""PDF cross-check on the Python side.

Runs two independent extraction libraries over the same PDF artifacts and
writes one JSONL file per library under `results/crosscheck/<lib>/<stamp>.jsonl`.
Both libraries stay inside the Python ecosystem, so neither is a substitute for
the TypeScript `pdfjs-dist` pass.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

import pymupdf
from pdfminer.high_level import extract_text as pdfminer_text


def normalise(text: str) -> str:
    return re.sub(r"\s+", " ", text.replace("\u00a0", " ")).strip()


def read_cases(corpus: Path) -> list[dict]:
    return json.loads((corpus / "cases.json").read_text(encoding="utf8"))["cases"]


def pages_pymupdf(pdf: Path) -> dict:
    document = pymupdf.open(pdf)
    texts = [normalise(page.get_text()) for page in document]
    drawings = sum(len(page.get_drawings()) for page in document)
    fonts = [font for page in document for font in document.get_page_fonts(page.number)]
    return {
        "pages": document.page_count,
        "text": normalise(" ".join(texts)),
        "drawings": drawings,
        "fonts": fonts,
    }


def pages_pdfminer(pdf: Path) -> dict:
    text = pdfminer_text(str(pdf))
    pages = len([chunk for chunk in text.split("\f") if chunk.strip()])
    return {"pages": pages, "text": normalise(text), "drawings": None, "fonts": None}


def font_records(fonts: list) -> list[dict]:
    seen = {}
    for font in fonts:
        xref, ext, kind, basefont, name, encoding = font
        seen[name] = {
            "basefont": basefont,
            "kind": kind,
            "extension": ext,
            "embedded": ext not in ("n/a", ""),
        }
    return [seen[key] for key in sorted(seen)]


class Writer:
    def __init__(self, root: Path, library: str, stamp: str):
        directory = root / "crosscheck" / library
        directory.mkdir(parents=True, exist_ok=True)
        self.path = directory / f"{stamp}.jsonl"
        self.diffs = []
        self.equivalent = 0

    def record(self, payload: dict) -> None:
        with self.path.open("a", encoding="utf8") as handle:
            handle.write(json.dumps(payload, ensure_ascii=False) + "\n")
        self.diffs.append(payload)

    def equal(self) -> None:
        self.equivalent += 1

    def summary(self, label: str) -> None:
        counts: dict[str, int] = {}
        for payload in self.diffs:
            counts[payload["classification"]] = counts.get(payload["classification"], 0) + 1
        print(f"{label}: {self.equivalent} equivalent, {len(self.diffs)} diff ({json.dumps(counts)})")
        print(f"wrote {self.path}")


def run_library(library: str, extractor, cases: list[dict], pdf_dir: Path, writer: Writer) -> None:
    for case in cases:
        pdf = pdf_dir / f"{case['id']}.pdf"
        if not pdf.exists():
            writer.record({
                "case": case["id"],
                "feature": "artifact",
                "ours": None,
                "theirs": str(pdf),
                "tolerance": "exact",
                "classification": "our-limitation",
                "note": "PDF artifact missing, run pdf/compare-ts.mjs first",
            })
            continue
        found = extractor(pdf)

        if "page_count" in case["features"] and found["pages"] != case["pages"]:
            writer.record({
                "case": case["id"],
                "feature": "page_count",
                "ours": found["pages"],
                "theirs": case["pages"],
                "tolerance": 0,
                "classification": "real-defect",
                "note": f"extracted by {library}",
            })
        elif "page_count" in case["features"]:
            writer.equal()

        if "text_normalised" in case["features"]:
            expected = normalise(case["expected_text"])
            if found["text"] != expected:
                writer.record({
                    "case": case["id"],
                    "feature": "text_normalised",
                    "ours": found["text"],
                    "theirs": expected,
                    "tolerance": "exact",
                    "classification": "real-defect",
                    "note": f"extracted by {library}",
                })
            else:
                writer.equal()

        if "font_embedded" in case["features"]:
            if found["fonts"] is None:
                writer.record({
                    "case": case["id"],
                    "feature": "font_embedded",
                    "ours": None,
                    "theirs": None,
                    "tolerance": "record-only",
                    "classification": "spec-ambiguity",
                    "note": f"{library} exposes no font embedding flags",
                })
            else:
                fonts = font_records(found["fonts"])
                missing = [font for font in fonts if not font["embedded"]]
                if missing:
                    writer.record({
                        "case": case["id"],
                        "feature": "font_embedded",
                        "ours": fonts,
                        "theirs": {"embedded": True},
                        "tolerance": "record-only",
                        "classification": "our-limitation",
                        "note": "non-embedded base-14 font, portable readers may substitute",
                    })
                else:
                    writer.equal()

        if "math_searchable" in case["features"]:
            formula = case["formula"]
            if formula not in found["text"]:
                writer.record({
                    "case": case["id"],
                    "feature": "math_searchable",
                    "ours": found["text"],
                    "theirs": formula,
                    "tolerance": "exact",
                    "classification": "our-limitation",
                    "note": f"formula not searchable through {library}",
                })
            else:
                writer.equal()

        if "math_typeset" in case["features"]:
            drawings = found["drawings"]
            if drawings is None:
                writer.record({
                    "case": case["id"],
                    "feature": "math_typeset",
                    "ours": None,
                    "theirs": None,
                    "tolerance": "record-only",
                    "classification": "spec-ambiguity",
                    "note": f"{library} exposes no vector drawing count",
                })
            else:
                writer.record({
                    "case": case["id"],
                    "feature": "math_typeset",
                    "ours": {"drawings": drawings, "text": found["text"]},
                    "theirs": {"drawings": None},
                    "tolerance": "record-only",
                    "classification": "real-defect",
                    "note": "formula is emitted as literal source text, there are no vector glyphs",
                })


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", required=True, type=Path)
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--pdf", required=True, type=Path)
    parser.add_argument("--stamp", required=True)
    arguments = parser.parse_args()

    cases = read_cases(arguments.corpus)
    for library, extractor in (("pymupdf", pages_pymupdf), ("pdfminer", pages_pdfminer)):
        writer = Writer(arguments.results, library, arguments.stamp)
        run_library(library, extractor, cases, arguments.pdf, writer)
        writer.summary(library)


if __name__ == "__main__":
    main()
