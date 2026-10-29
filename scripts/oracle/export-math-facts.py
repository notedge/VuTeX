#!/usr/bin/env python3
"""Export OpenType MATH facts with fontTools, as an external oracle snapshot.

The MATH table is the only source of truth for math constants, glyph variants,
piecewise assemblies and corner kerns. VuTeX reads that table through its own
Rust parser, so a snapshot produced by a completely different implementation
(fontTools) is the only way to tell "VuTeX read the font correctly" apart from
"VuTeX agrees with itself".

This script writes raw font units exactly as fontTools reports them. It never
normalizes to em, rounds, or fills in a value the table does not contain:
missing coverage is written as JSON null. Conversion to em happens in the
Rust test, which also owns the assertion tolerances.

Usage (from the repository root):

    python scripts/oracle/export-math-facts.py

Optional flags:

    --font <path>   font to measure (default: the font embedded by vutex-text)
    --out <path>    fixture to write (default: tests/fixtures/oracle/...)

Requires fontTools (`pip install fonttools`). Regeneration is deterministic:
no timestamps are written, so a rerun against an unchanged font produces a
byte-identical fixture.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

import fontTools
from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_FONT = REPO_ROOT / "projects/crates/vutex-text/assets/LatinModernMath-Regular.otf"
DEFAULT_OUT = REPO_ROOT / "projects/crates/vutex-layout/tests/fixtures/oracle/latinmodern-math-facts.json"

# MATH constants that the spec stores as percentages instead of design units.
PERCENT_CONSTANTS = {
    "ScriptPercentScaleDown",
    "ScriptScriptPercentScaleDown",
    "RadicalDegreeBottomRaisePercent",
}

# Characters whose glyph metrics VuTeX derives from the font. They cover the
# corpus atoms, the delimiters and the accent marks, so a drift in any of the
# three extraction paths (advance, bbox, italic correction) shows up here.
GLYPH_CHARS = [
    "x", "a", "b", "f", "A", "B", "1", "0",
    "(", ")", "[", "]", "{", "}",
    "|", "\u2016", "\u27e8", "\u27e9",
    "\u2211", "\u220f", "\u222b", "\u221a",
    "\u02c6", "\u00af", "\u2192",
]

# Characters whose vertical MATH Variants construction VuTeX imports. All of
# them stretch vertically in Latin Modern Math, which is the construction the
# layout engine consumes for delimiters and display-size operators.
VARIANT_CHARS = [
    "(", ")", "[", "]", "{", "}", "|", "\u2016", "\u27e8", "\u27e9",
    "\u2211", "\u220f", "\u222b",
]

EXTENDER_BIT = 0x0001
KERN_CORNERS = (
    ("top_right", "TopRightMathKern"),
    ("top_left", "TopLeftMathKern"),
    ("bottom_right", "BottomRightMathKern"),
    ("bottom_left", "BottomLeftMathKern"),
)


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


class MathFacts:
    """Everything the dump needs, read once from one TTFont."""

    def __init__(self, font: TTFont) -> None:
        if "MATH" not in font:
            raise SystemExit("font has no MATH table")
        self.font = font
        self.cmap = font.getBestCmap()
        self.glyph_set = font.getGlyphSet()
        self.glyph_ids = {name: font.getGlyphID(name) for name in font.getGlyphOrder()}
        self.hmtx = {name: int(font["hmtx"][name][0]) for name in font.getGlyphOrder()}

        math_glyph_info = font["MATH"].table.MathGlyphInfo
        self.italic_correction = self.covered_values(math_glyph_info.MathItalicsCorrectionInfo, "Coverage", "ItalicsCorrection")
        self.top_accent = self.covered_values(math_glyph_info.MathTopAccentAttachment, "TopAccentCoverage", "TopAccentAttachment")

    @staticmethod
    def covered_values(subtable, coverage_attr: str, values_attr: str) -> dict[str, int]:
        """Map glyph name -> MathValue for one MathGlyphInfo coverage pair."""
        if subtable is None:
            return {}
        coverage = getattr(subtable, coverage_attr, None)
        values = getattr(subtable, values_attr, None)
        if coverage is None or values is None:
            return {}
        return {name: int(value.Value) for name, value in zip(coverage.glyphs, values)}

    def glyph_name(self, ch: str) -> str:
        codepoint = ord(ch)
        if codepoint not in self.cmap:
            raise SystemExit(f"font has no cmap entry for U+{codepoint:04X} ({ch!r})")
        return self.cmap[codepoint]

    def glyph_facts(self, ch: str) -> dict:
        glyph_name = self.glyph_name(ch)
        pen = BoundsPen(self.glyph_set)
        self.glyph_set[glyph_name].draw(pen)
        bounds = pen.bounds  # (xMin, yMin, xMax, yMax) or None
        return {
            "char": ch,
            "codepoint": ord(ch),
            "glyph_name": glyph_name,
            "glyph_id": self.glyph_ids[glyph_name],
            "advance": self.hmtx[glyph_name],
            "bounds": None
            if bounds is None
            else {"x_min": int(bounds[0]), "y_min": int(bounds[1]), "x_max": int(bounds[2]), "y_max": int(bounds[3])},
            "italic_correction": self.italic_correction.get(glyph_name),
            "top_accent_attachment": self.top_accent.get(glyph_name),
        }

    def construction_facts(self, construction) -> dict | None:
        if construction is None:
            return None
        return {
            "variants": [
                {
                    "glyph_name": record.VariantGlyph,
                    "glyph_id": self.glyph_ids[record.VariantGlyph],
                    # MATH advance along the stretch direction, not the hmtx advance.
                    "advance_measurement": int(record.AdvanceMeasurement),
                    "hmtx_advance": self.hmtx[record.VariantGlyph],
                }
                for record in construction.MathGlyphVariantRecord
            ],
            "assembly": None
            if construction.GlyphAssembly is None
            else {
                "parts": [
                    {
                        "glyph_name": part.glyph,
                        "glyph_id": self.glyph_ids[part.glyph],
                        "hmtx_advance": self.hmtx[part.glyph],
                        "start_connector_length": int(part.StartConnectorLength),
                        "end_connector_length": int(part.EndConnectorLength),
                        "full_advance": int(part.FullAdvance),
                        "is_extender": bool(part.PartFlags & EXTENDER_BIT),
                    }
                    for part in construction.GlyphAssembly.PartRecords
                ],
            },
        }

    def variant_facts(self, ch: str) -> dict:
        glyph_name = self.glyph_name(ch)
        variations = self.font["MATH"].table.MathVariants
        coverage = variations.VertGlyphCoverage.glyphs
        record = {
            "char": ch,
            "codepoint": ord(ch),
            "glyph_name": glyph_name,
            "glyph_id": self.glyph_ids[glyph_name],
            "vertical": None,
        }
        if glyph_name in coverage:
            record["vertical"] = self.construction_facts(variations.VertGlyphConstruction[coverage.index(glyph_name)])
        return record

    def kern_facts(self) -> dict:
        kern = getattr(self.font["MATH"].table.MathGlyphInfo, "MathKernInfo", None)
        if kern is None:
            return {
                "present": False,
                "reason": "the font publishes no MathKernInfo subtable, so no corner kern exists at all",
                "glyphs": None,
            }

        glyphs = []
        for glyph_name, record in zip(kern.MathKernCoverage.glyphs, kern.MathKernInfoRecords):
            corners = {}
            for corner, attr in KERN_CORNERS:
                table = getattr(record, attr, None)
                corners[corner] = None if table is None else {
                    "correction_heights": [int(h) for h in table.CorrectionHeight],
                    "kern_values": [int(v) for v in table.KernValue],
                }
            if all(corner is None for corner in corners.values()):
                continue
            glyphs.append({"glyph_name": glyph_name, "glyph_id": self.glyph_ids[glyph_name], "corners": corners})
        return {"present": True, "reason": None, "glyphs": glyphs}

    def constants(self) -> dict:
        constants = {}
        for name, value in vars(self.font["MATH"].table.MathConstants).items():
            if name.startswith("_"):
                continue
            is_percent = name in PERCENT_CONSTANTS
            # MathValue-based constants arrive as records with a device table.
            raw = int(value.Value) if hasattr(value, "Value") else int(value)
            constants[name] = {"value": raw, "unit": "percent" if is_percent else "font-units"}
        return constants


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--font", type=Path, default=DEFAULT_FONT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args()

    if not args.font.is_file():
        raise SystemExit(f"font not found: {args.font}")

    font = TTFont(str(args.font))
    facts = MathFacts(font)
    name_table = font["name"]

    document = {
        "schema_version": "v0",
        "source": {
            "engine": "fonttools",
            "version": fontTools.version,
            "command": "python scripts/oracle/export-math-facts.py",
            "status": "available",
            "status_reason": None,
        },
        "font": {
            "path": args.font.relative_to(REPO_ROOT).as_posix() if args.font.is_absolute() else str(args.font),
            "sha256": sha256_of(args.font),
            "size_bytes": args.font.stat().st_size,
            "family": name_table.getDebugName(1),
            "subfamily": name_table.getDebugName(2),
            "full_name": name_table.getDebugName(4),
            "version": name_table.getDebugName(5),
            "units_per_em": int(font["head"].unitsPerEm),
            "outline_format": "CFF" if "CFF " in font else "glyf",
        },
        "math": {
            "constants": facts.constants(),
            "glyphs": [facts.glyph_facts(ch) for ch in GLYPH_CHARS],
            "variants": [facts.variant_facts(ch) for ch in VARIANT_CHARS],
            "math_kern_info": facts.kern_facts(),
        },
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {args.out.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
