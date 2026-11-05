"""Compare math typesetting facts between a real TeX PDF and the VuTeX PDF.

Reference formulas are text glyphs, ours are vector outlines.  The original
harness mixed two coordinate systems: it unioned PyMuPDF *span boxes* for the
reference (font ascender/descender padding, clipped by the marker band) and
`get_drawings()` ink rectangles for VuTeX.  That made `height` and
`baseline_offset` incomparable.

This harness now measures one fact the same way on both sides:

* render the page to a grayscale raster and take the ink bounding box of the
  formula band (marker glyphs are blanked out first);
* take the body size and the baseline from `rawdict` character origins of the
  marker line, never from a bbox edge;
* normalise advance / height / offsets / gaps by the body size.

The pre-fix numbers are still emitted under `legacy` so the false deviations
can be audited against the corrected ones.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import pymupdf

MARKERS_PER_CASE = 2

CASES = (
    {"id": "fraction-alpha-beta", "first": "case 1 head", "second": "case 1 tail"},
    {"id": "fraction-sum", "first": "case 2 head", "second": "case 2 tail"},
    {"id": "scripts", "first": "case 3 head", "second": "case 3 tail"},
    {"id": "integral-limits", "first": "case 4 head", "second": "case 4 tail"},
    {"id": "radical-index", "first": "case 5 head", "second": "case 5 tail"},
    {"id": "sum-limits", "first": "case 6 head", "second": "case 6 tail"},
    {"id": "scaled-delimiters", "first": "case 7 head", "second": "case 7 tail"},
    {"id": "aligned-rows", "first": "case 8 head", "second": "case 8 tail"},
)

DPI = 600
INK_THRESHOLD = 160
FACT_KEYS = ("advance", "height", "baseline_offset", "gap_above", "gap_below")


def normalise(text: str) -> str:
    return " ".join(text.split())


def text_lines(page):
    lines = []
    for block in page.get_text("rawdict")["blocks"]:
        for line in block.get("lines", []):
            text = normalise("".join(ch["c"] for span in line["spans"] for ch in span["chars"]))
            if not text:
                continue
            origin = None
            for span in line["spans"]:
                for char in span["chars"]:
                    if char["c"].strip():
                        origin = char["origin"]
                        break
                if origin is not None:
                    break
            sizes = [span["size"] for span in line["spans"]] or [12.0]
            lines.append({
                "text": text,
                "bbox": pymupdf.Rect(line["bbox"]),
                "baseline": origin[1] if origin is not None else line["bbox"][3],
                "origin_x": origin[0] if origin is not None else line["bbox"][0],
                "size": max(sizes),
            })
    return lines


def marker_line(lines, text):
    wanted = normalise(text)
    for line in lines:
        if line["text"] == wanted:
            return line
    raise SystemExit(f"marker `{text}` missing from PDF text")


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


def legacy_formula_rect(kind, page, band, marker_rects):
    if kind == "reference":
        rects = [span["bbox"] for span in text_spans(page)
                 if band[0] <= (span["bbox"][1] + span["bbox"][3]) / 2 <= band[1]
                 and not any(pymupdf.Rect(span["bbox"]).intersects(marker) for marker in marker_rects)]
        return union(rects)
    rects = [drawing["rect"] for drawing in page.get_drawings()
             if band[0] <= (drawing["rect"].y0 + drawing["rect"].y1) / 2 <= band[1]]
    return union(rects)


def legacy_measure(kind, page, first, second):
    head = marker_rect(page, first)
    tail = marker_rect(page, second)
    body = marker_size(page, head)
    rect = legacy_formula_rect(kind, page, (head.y1, tail.y0), (head, tail))
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


class Raster:
    """Grayscale page raster with marker glyphs blanked out."""

    def __init__(self, page, marker_lines):
        zoom = DPI / 72
        pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), alpha=False, colorspace=pymupdf.csGRAY)
        self.samples = bytearray(pix.samples)
        self.width = pix.width
        self.height = pix.height
        self.zoom = zoom
        for line in marker_lines:
            box = pymupdf.Rect(line["bbox"])
            self._blank(box)

    def _blank(self, box):
        x0 = max(0, int((box.x0 - 1.0) * self.zoom))
        y0 = max(0, int((box.y0 - 1.0) * self.zoom))
        x1 = min(self.width, int((box.x1 + 1.0) * self.zoom) + 1)
        y1 = min(self.height, int((box.y1 + 1.0) * self.zoom) + 1)
        for y in range(y0, y1):
            row = y * self.width
            for x in range(x0, x1):
                self.samples[row + x] = 255

    def ink(self, y0, y1):
        start = max(0, int(y0 * self.zoom))
        stop = min(self.height, int(y1 * self.zoom) + 1)
        min_x, min_y, max_x, max_y = self.width, self.height, -1, -1
        for y in range(start, stop):
            row = y * self.width
            for x in range(self.width):
                if self.samples[row + x] < INK_THRESHOLD:
                    if x < min_x:
                        min_x = x
                    if x > max_x:
                        max_x = x
                    if y < min_y:
                        min_y = y
                    if y > max_y:
                        max_y = y
        if max_x < 0:
            return None
        return pymupdf.Rect(
            min_x / self.zoom,
            min_y / self.zoom,
            (max_x + 1) / self.zoom,
            (max_y + 1) / self.zoom,
        )


def ink_measure(raster, head, tail):
    body = head["size"]
    top = head["bbox"].y0 - body
    bottom = tail["bbox"].y1 + body
    rect = raster.ink(top, bottom)
    if rect is None:
        raise SystemExit(f"no ink between `{head['text']}` and `{tail['text']}`")
    return {
        "advance": rect.width / body,
        "height": rect.height / body,
        "baseline_offset": ((rect.y0 + rect.y1) / 2 - head["baseline"]) / body,
        "gap_above": (rect.y0 - head["baseline"]) / body,
        "gap_below": (tail["baseline"] - rect.y1) / body,
        "body_size": body,
        "ink_bbox": [round(value, 3) for value in rect],
        "head_baseline": round(head["baseline"], 3),
        "tail_baseline": round(tail["baseline"], 3),
    }


def facts(measurement):
    return {key: measurement[key] for key in FACT_KEYS}


def deltas(ours, reference):
    return {key: round(ours[key] - reference[key], 3) for key in FACT_KEYS}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ours", required=True)
    parser.add_argument("--reference", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    ours = pymupdf.open(args.ours)
    reference = pymupdf.open(args.reference)
    ours_lines = text_lines(ours[0])
    reference_lines = text_lines(reference[0])
    marker_texts = [case[key] for case in CASES for key in ("first", "second")]
    ours_raster = Raster(ours[0], [marker_line(ours_lines, text) for text in marker_texts])
    reference_raster = Raster(reference[0], [marker_line(reference_lines, text) for text in marker_texts])

    records = []
    for case in CASES:
        try:
            ours_facts = facts(ink_measure(
                ours_raster,
                marker_line(ours_lines, case["first"]),
                marker_line(ours_lines, case["second"]),
            ))
            reference_facts = facts(ink_measure(
                reference_raster,
                marker_line(reference_lines, case["first"]),
                marker_line(reference_lines, case["second"]),
            ))
            legacy_ours = legacy_measure("ours", ours[0], case["first"], case["second"])
            legacy_reference = legacy_measure("reference", reference[0], case["first"], case["second"])
        except SystemExit as error:
            records.append({"case": case["id"], "error": str(error)})
            continue
        records.append({
            "case": case["id"],
            "ours": ours_facts,
            "reference": reference_facts,
            "deltas": deltas(ours_facts, reference_facts),
            "legacy": {
                "ours": facts(legacy_ours),
                "reference": facts(legacy_reference),
                "deltas": deltas(legacy_ours, legacy_reference),
            },
        })
    payload = {
        "engine": "tectonic",
        "criterion": {
            "version": 2,
            "fact": "ink-bbox",
            "raster_dpi": DPI,
            "ink_threshold": INK_THRESHOLD,
            "baseline": "marker rawdict character origin",
            "normalisation": "per-side marker body size",
        },
        "records": records,
    }
    Path(args.out).write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps(payload, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
