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
import re
from pathlib import Path

import pymupdf

MARKERS_PER_CASE = 2
MARKER_TEXT = re.compile(r"(?:case|atom) \d+ (?:head|tail)")

CASES = (
    {"id": "fraction-alpha-beta", "first": "case 1 head", "second": "case 1 tail"},
    {"id": "fraction-sum", "first": "case 2 head", "second": "case 2 tail"},
    {"id": "scripts", "first": "case 3 head", "second": "case 3 tail"},
    {"id": "integral-limits", "first": "case 4 head", "second": "case 4 tail"},
    {"id": "radical-index", "first": "case 5 head", "second": "case 5 tail"},
    {"id": "sum-limits", "first": "case 6 head", "second": "case 6 tail"},
    {"id": "scaled-delimiters", "first": "case 7 head", "second": "case 7 tail"},
    {"id": "aligned-rows", "first": "case 8 head", "second": "case 8 tail"},
    {"id": "overbrace", "first": "case 9 head", "second": "case 9 tail"},
    {"id": "underbrace", "first": "case 10 head", "second": "case 10 tail"},
    {"id": "cases-environment", "first": "case 11 head", "second": "case 11 tail"},
    {"id": "pmatrix", "first": "case 12 head", "second": "case 12 tail"},
    {"id": "scripts-implicit", "first": "case 13 head", "second": "case 13 tail"},
    {"id": "nested-middle", "first": "case 14 head", "second": "case 14 tail"},
    {"id": "sqrt-plain", "first": "case 15 head", "second": "case 15 tail"},
    {"id": "textstyle-fraction", "first": "case 16 head", "second": "case 16 tail"},
    {"id": "inline-sum", "first": "case 17 head", "second": "case 17 tail"},
)

# Single-atom formulas.  Their facts isolate the font difference: no fraction
# rule, script shift or limit placement takes part, so whatever remains is the
# reference symbol against ours.
ATOMS = (
    {"id": "alpha", "first": "atom 1 head", "second": "atom 1 tail"},
    {"id": "beta", "first": "atom 2 head", "second": "atom 2 tail"},
    {"id": "sum", "first": "atom 3 head", "second": "atom 3 tail"},
    {"id": "integral", "first": "atom 4 head", "second": "atom 4 tail"},
    {"id": "paren", "first": "atom 5 head", "second": "atom 5 tail"},
    {"id": "radical", "first": "atom 6 head", "second": "atom 6 tail"},
    {"id": "x", "first": "atom 7 head", "second": "atom 7 tail"},
    {"id": "one", "first": "atom 8 head", "second": "atom 8 tail"},
    {"id": "langle", "first": "atom 9 head", "second": "atom 9 tail"},
    {"id": "null-delimiter", "first": "atom 10 head", "second": "atom 10 tail"},
)

# Each set is one PDF pair.  Sets stay small on purpose: a case must sit on one
# page on both sides, and a page that has to absorb too many display formulas
# shrinks the display skips, which moves `gap_above` / `gap_below` without any
# layout change.  `(marker prefix, rows, first number, last number)`.
CASE_SETS = {
    "display": ("case", CASES, 1, 8),
    "display-extra-a": ("case", CASES, 9, 12),
    "display-extra-b": ("case", CASES, 13, 17),
    "atoms-a": ("atom", ATOMS, 1, 5),
    "atoms-b": ("atom", ATOMS, 6, 10),
}


def expand(spec):
    prefix, rows, low, high = spec
    expanded = []
    for number, row in enumerate(rows, start=1):
        if low <= number <= high:
            expanded.append({
                "id": row["id"],
                "first": f"{prefix} {number} head",
                "second": f"{prefix} {number} tail",
            })
    return expanded

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


def pages_of(doc):
    return [text_lines(page) for page in doc]


def marker_index(pages):
    index = {}
    for number, lines in enumerate(pages):
        for line in lines:
            index.setdefault(line["text"], number)
    return index


def marker_page(index, pages, text):
    number = index.get(normalise(text))
    if number is None:
        raise SystemExit(f"marker `{text}` missing from PDF text")
    return number, marker_line(pages[number], text)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ours", required=True)
    parser.add_argument("--reference", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--set", choices=sorted(CASE_SETS), default="display")
    args = parser.parse_args()
    cases = expand(CASE_SETS[args.set])

    ours_doc = pymupdf.open(args.ours)
    reference_doc = pymupdf.open(args.reference)
    ours_pages = pages_of(ours_doc)
    reference_pages = pages_of(reference_doc)
    ours_index = marker_index(ours_pages)
    reference_index = marker_index(reference_pages)
    rasters = {"ours": {}, "reference": {}}

    def rasters_for(kind, doc, pages, index, first, second):
        # A case must live on one page: a band that straddles a page break has
        # no meaning, and our pagination is not TeX's.
        page_number, start = marker_page(index, pages, first)
        other, _ = marker_page(index, pages, second)
        if other != page_number:
            raise SystemExit(f"`{first}` and `{second}` straddle a page break")
        if page_number not in rasters[kind]:
            cache = rasters[kind]
            cache[page_number] = Raster(
                doc[page_number],
                [line for line in pages[page_number] if MARKER_TEXT.fullmatch(line["text"])],
            )
        return doc[page_number], start, marker_line(pages[page_number], second), rasters[kind][page_number]

    records = []
    for case in cases:
        try:
            ours_page, ours_head, ours_tail, ours_raster = rasters_for(
                "ours", ours_doc, ours_pages, ours_index, case["first"], case["second"])
            reference_page, reference_head, reference_tail, reference_raster = rasters_for(
                "reference", reference_doc, reference_pages, reference_index, case["first"], case["second"])
            ours_facts = facts(ink_measure(ours_raster, ours_head, ours_tail))
            reference_facts = facts(ink_measure(reference_raster, reference_head, reference_tail))
            legacy_ours = legacy_measure("ours", ours_page, case["first"], case["second"])
            legacy_reference = legacy_measure("reference", reference_page, case["first"], case["second"])
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
        "corpus": args.set,
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
