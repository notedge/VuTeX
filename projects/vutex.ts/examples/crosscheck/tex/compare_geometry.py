"""Compare math typesetting facts between a real TeX PDF and the VuTeX PDF.

Both PDFs are read with PyMuPDF. Reference formulas are glyph spans, ours are
vector outlines, so the measurements are scale-normalised against the body
font size instead of absolute points: advance, height, baseline offset and the
gaps above and below the formula.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import pymupdf

MARKERS = ("before marker", "after marker", "second marker")

CASES = (
    {"id": "display-integral", "first": "before marker", "second": "after marker"},
    {"id": "display-fraction", "first": "after marker", "second": "second marker"},
)


def text_spans(page):
    spans = []
    for block in page.get_text("dict")["blocks"]:
        for line in block.get("lines", []):
            for span in line["spans"]:
                if span["text"].strip():
                    spans.append(span)
    return spans


def marker_rect(page, text):
    hits = page.search_for(text)
    if not hits:
        raise SystemExit(f"marker `{text}` missing from PDF text")
    return hits[0]


def marker_size(page, rect):
    sizes = [span["size"] for span in text_spans(page)
             if pymupdf.Rect(span["bbox"]).intersects(rect)]
    return max(sizes) if sizes else 12.0


def union(rects):
    boxes = [pymupdf.Rect(rect) for rect in rects]
    if not boxes:
        return None
    result = pymupdf.Rect(boxes[0])
    for box in boxes[1:]:
        result |= box
    return result


def formula_rect(kind, page, band, marker_rects):
    if kind == "reference":
        rects = [span["bbox"] for span in text_spans(page)
                 if band[0] <= (span["bbox"][1] + span["bbox"][3]) / 2 <= band[1]
                 and not any(pymupdf.Rect(span["bbox"]).intersects(marker) for marker in marker_rects)]
        return union(rects)
    rects = [drawing["rect"] for drawing in page.get_drawings()
             if band[0] <= (drawing["rect"].y0 + drawing["rect"].y1) / 2 <= band[1]]
    return union(rects)


def measure(kind, page, first, second):
    head = marker_rect(page, first)
    tail = marker_rect(page, second)
    body = marker_size(page, head)
    rect = formula_rect(kind, page, (head.y1, tail.y0), (head, tail))
    if rect is None:
        raise SystemExit(f"no formula geometry between `{first}` and `{second}`")
    return {
        "advance": rect.width / body,
        "height": rect.height / body,
        "baseline_offset": ((rect.y0 + rect.y1) / 2 - head.y1) / body,
        "gap_above": (rect.y0 - head.y1) / body,
        "gap_below": (tail.y0 - rect.y1) / body,
        "body_size": body,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ours", required=True)
    parser.add_argument("--reference", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    ours = pymupdf.open(args.ours)
    reference = pymupdf.open(args.reference)
    records = []
    for case in CASES:
        try:
            ours_facts = measure("ours", ours[0], case["first"], case["second"])
            reference_facts = measure("reference", reference[0], case["first"], case["second"])
        except SystemExit as error:
            records.append({"case": case["id"], "error": str(error)})
            continue
        records.append({
            "case": case["id"],
            "ours": ours_facts,
            "reference": reference_facts,
            "deltas": {
                key: round(ours_facts[key] - reference_facts[key], 3)
                for key in ("advance", "height", "baseline_offset", "gap_above", "gap_below")
            },
        })
    payload = {"engine": "tectonic", "records": records}
    Path(args.out).write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps(payload, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
