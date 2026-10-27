#!/usr/bin/env python3
"""Validate VuTeX math-corpus PDFs with two independent parsers.

The gate is optional: it shells out to the `#[ignore]` Rust writer in
`projects/crates/vutex-render-pdf/tests/math_corpus_pdf.rs`, which renders one
PDF page per layout record of the shared math corpus. Every page is then opened
with pypdf *and* PyMuPDF, two parsers that share no code, and both must agree
that the page carries vector drawings and no text at all.

Usage::

    python scripts/ci/validate-math-corpus-pdf.py

Dependencies:
    - pypdf (``pip install pypdf``)
    - PyMuPDF (``pip install PyMuPDF``)
    - cargo and a checked-out VuTeX workspace
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TEST_NAME = "write_corpus_pdfs_for_external_check"


def import_parsers():
    """Import both parsers or fail with an install hint."""
    try:
        import pypdf  # type: ignore[import-not-found]
    except ImportError as exc:
        raise SystemExit(f"ERROR: pypdf missing (pip install pypdf): {exc}")
    try:
        import pymupdf  # type: ignore[import-not-found]
    except ImportError:
        try:
            import fitz as pymupdf  # PyMuPDF < 1.25 still ships as ``fitz``
        except ImportError as exc:
            raise SystemExit(f"ERROR: PyMuPDF missing (pip install PyMuPDF): {exc}")
    return pypdf, pymupdf


def run_cargo_writer(output_dir: Path) -> int:
    """Run the ignored Rust test that writes the corpus PDFs."""
    command = [
        "cargo",
        "test",
        "-p",
        "vutex-render-pdf",
        "--test",
        "math_corpus_pdf",
        "--",
        "--ignored",
        "--nocapture",
        TEST_NAME,
    ]
    env = os.environ.copy()
    env["VUTEX_CORPUS_PDF_DIR"] = str(output_dir)
    proc = subprocess.run(command, cwd=str(ROOT), env=env, capture_output=True, text=True, timeout=900)
    sys.stderr.write(f"--- cargo stdout ---\n{proc.stdout[-1500:]}\n")
    if proc.stderr:
        sys.stderr.write(f"--- cargo stderr ---\n{proc.stderr[-1500:]}\n")
    return proc.returncode


def check_with_pypdf(pypdf, pdf_path: Path, raw: bytes) -> list[str]:
    """A pypdf page must have ink and no text."""
    errors: list[str] = []
    try:
        reader = pypdf.PdfReader(str(pdf_path), strict=False)
    except Exception as exc:  # noqa: BLE001 - any parser failure is a finding
        return [f"pypdf raised {type(exc).__name__}: {exc}"]

    if len(reader.pages) != 1:
        errors.append(f"pypdf sees {len(reader.pages)} page(s), expected 1")
        return errors

    page = reader.pages[0]
    text = page.extract_text() or ""
    if text.strip():
        errors.append(f"pypdf extracted host text: {text[:120]!r}")

    try:
        contents = page.get_contents()
        data = contents.get_data() if contents is not None else b""
    except Exception as exc:  # noqa: BLE001
        errors.append(f"pypdf could not read the content stream: {exc}")
        data = b""

    for operator in (b"BT", b"Tj", b"TJ"):
        if operator in data:
            errors.append(f"pypdf found the text operator {operator!r} in the content stream")

    if b"/BaseFont" in raw:
        errors.append("pypdf target references a host font (/BaseFont)")

    return errors


def check_with_pymupdf(pymupdf, pdf_path: Path) -> tuple[list[str], int, int]:
    """A PyMuPDF page must expose drawings and no text."""
    errors: list[str] = []
    try:
        document = pymupdf.open(str(pdf_path))
    except Exception as exc:  # noqa: BLE001
        return [f"PyMuPDF raised {type(exc).__name__}: {exc}"], 0, 0

    try:
        if document.page_count != 1:
            errors.append(f"PyMuPDF sees {document.page_count} page(s), expected 1")
            return errors, document.page_count, 0
        page = document.load_page(0)
        text = page.get_text("text") or ""
        if text.strip():
            errors.append(f"PyMuPDF extracted host text: {text[:120]!r}")
        drawings = page.get_drawings()
        if not drawings:
            errors.append("PyMuPDF found no vector drawings on a math page")
        return errors, document.page_count, len(drawings)
    finally:
        document.close()


def main() -> int:
    if shutil.which("cargo") is None:
        print("ERROR: cargo not on PATH", file=sys.stderr)
        return 2

    pypdf, pymupdf = import_parsers()
    output_dir = Path(tempfile.mkdtemp(prefix="vutex-math-corpus-"))

    print(f"Rendering the math corpus with the Rust writer into {output_dir}")
    code = run_cargo_writer(output_dir)
    if code != 0:
        print(f"ERROR: the Rust writer exited {code}, artifacts kept in {output_dir}", file=sys.stderr)
        return 1

    manifest_path = output_dir / "manifest.json"
    if not manifest_path.exists():
        print(f"ERROR: no manifest written to {manifest_path}", file=sys.stderr)
        return 1
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    print(f"Validating {len(manifest)} page(s) with pypdf and PyMuPDF\n")

    failures = 0
    for entry in manifest:
        pdf_path = output_dir / entry["file"]
        raw = pdf_path.read_bytes()
        pypdf_errors = check_with_pypdf(pypdf, pdf_path, raw)
        pymupdf_errors, pages, drawings = check_with_pymupdf(pymupdf, pdf_path)
        errors = pypdf_errors + pymupdf_errors
        status = "PASS" if not errors else "FAIL"
        failures += 1 if errors else 0
        print(f"  [{status}] {entry['file']:<12} style={entry['style']:<8} drawings={drawings:<3} {entry['src']}")
        for error in errors:
            print(f"           ! {error}")

    print()
    if failures:
        print(f"RESULT: {failures} page(s) failed the dual-parser gate, artifacts kept in {output_dir}")
        return 1
    shutil.rmtree(output_dir, ignore_errors=True)
    print(f"RESULT: all {len(manifest)} math pages are outline-only under both pypdf and PyMuPDF.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
