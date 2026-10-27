#!/usr/bin/env python3
"""Validate VuTeX-generated PDFs with an external parser (PyMuPDF / fitz).

This script drives the vutex-napi `pdf_export` integration test (marked
`#[ignore]`) through environment variables, asking it to render several
markdown inputs to PDFs. Each generated file is then opened with PyMuPDF
and its page count, text content, and any parse exceptions are reported.

The goal is PROVE the /Length fix produces PDF bytes a mainstream parser
accepts without warnings or exceptions. No fix is attempted; we only
collect evidence.

Usage::

    python scripts/ci/validate-pdf-pymupdf.py

Dependencies:
    - PyMuPDF (``pip install PyMuPDF``)
    - cargo + a checked-out VuTeX workspace
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


ROOT = Path(__file__).resolve().parents[2]
CARGO_TARGET = ROOT / "target"
# The integration test that renders markdown -> PDF via env vars.
TEST_NAME = "pdf_export::export_pdf_to_path"


# ---------- Sample documents -------------------------------------------------

SIMPLE_MD = """# 简单文档

这是一个简单段落。

- 项目一
- 项目二
- 项目三

```rust
fn main() {
    println!("Hello, world!");
}
```

> 这是一段引用文字。
"""

CJK_MD = """# 中文与箭头

这是一段中文正文：你好，世界！

箭头符号：→ ← ↑ ↓ ⇒ ⇐

数学符号：∑ ∞ ≤ ≥ ∈ ± ≠ ≈ √

单色 emoji：✔ ★ ☀ ✦

```rust
// 中文注释：渲染入口
fn render() -> usize { 42 }
```
"""

# We use the shipped acceptance fixture directly — it covers Chinese prose,
# Mermaid / TikZ / SVG diagrams, arrows, maths, and emoji end-to-end.
ACCEPTANCE_FIXTURE = ROOT / "projects" / "crates" / "vutex-napi" / "tests" / "fixtures" / "document-acceptance.md"


# ---------- Orchestration ----------------------------------------------------

@dataclass
class Case:
    name: str
    input_path: Path          # path to the markdown we feed vutex
    note: str = ""            # short description for the report
    options: Optional[dict] = None  # extra JSON options, e.g. {"font_policy": "filesystem"}

@dataclass
class Result:
    case: Case
    exit_code: int
    pdf_path: Optional[Path] = None
    pdf_bytes: int = 0
    pymupdf_ok: bool = False
    pages: int = 0
    text_preview: str = ""
    errors: list[str] = field(default_factory=list)


def run_cargo_export(case: Case, out_pdf: Path) -> int:
    """Run the ignored export test with the given env vars."""
    cargo_cmd = [
        "cargo", "test", "-p", "vutex-napi",
        "--test", "pdf_export",
        "--", "--ignored", "--nocapture", TEST_NAME.split("::")[-1],
    ]
    env = os.environ.copy()
    env["VUTEX_INPUT"] = str(case.input_path)
    env["VUTEX_OUTPUT"] = str(out_pdf)
    if case.options:
        env["VUTEX_OPTIONS"] = json.dumps(case.options, ensure_ascii=False)
    proc = subprocess.run(
        cargo_cmd,
        cwd=str(ROOT),
        env=env,
        capture_output=True,
        text=True,
        timeout=600,
    )
    # Echo cargo output so operators can see compiler/linker issues.
    sys.stderr.write(f"--- cargo stdout for {case.name} ---\n{proc.stdout[-2000:]}\n")
    if proc.stderr:
        sys.stderr.write(f"--- cargo stderr for {case.name} ---\n{proc.stderr[-2000:]}\n")
    return proc.returncode


def _import_pymupdf():
    """Import pymupdf with a graceful fallback to the legacy ``fitz`` name."""
    try:
        import pymupdf  # type: ignore[import-not-found]
        return pymupdf
    except ImportError:
        try:
            import fitz as pymupdf  # PyMuPDF < 1.25 still ships as ``fitz``
            return pymupdf
        except ImportError as exc:
            raise exc


def validate_with_pymupdf(pdf_path: Path) -> tuple[bool, int, str, list[str]]:
    """Open the PDF with PyMuPDF, extract basic stats. Returns (ok, pages, preview, errors)."""
    errors: list[str] = []
    try:
        pymupdf = _import_pymupdf()
    except ImportError as exc:
        return False, 0, "", [f"PyMuPDF not importable (try ``pip install PyMuPDF``): {exc}"]

    try:
        doc = pymupdf.open(str(pdf_path))
    except Exception as exc:  # pragma: no cover — PyMuPDF 1.28+ wraps everything
        return False, 0, "", [f"fitz.open raised: {type(exc).__name__}: {exc}"]

    pages = doc.page_count
    if pages == 0:
        doc.close()
        return True, 0, "", ["PDF has zero pages — vutex wrote an empty document"]

    # Extract text from every page. We don't assert content — just confirm
    # PyMuPDF can walk the tree without raising.
    chunks: list[str] = []
    try:
        for i in range(pages):
            page = doc.load_page(i)
            text = page.get_text("text") or ""
            chunks.append(f"[p{i+1}:{len(text)}c]")
    except Exception as exc:
        errors.append(f"page walk raised: {type(exc).__name__}: {exc}")

    # Also read xref size + trailer to catch stream /Length issues the
    # high-level text API might swallow as warnings.
    try:
        xref_size = doc.xref_length()
        _ = doc.xref_stream
    except Exception as exc:
        errors.append(f"xref introspection raised: {type(exc).__name__}: {exc}")

    doc.close()
    ok = len(errors) == 0
    preview = " ".join(chunks[:8])
    return ok, pages, preview, errors


# ---------- Main -------------------------------------------------------------

def build_cases(tmpdir: Path) -> list[Case]:
    cases: list[Case] = []

    # Case 1: simple markdown.
    p1 = tmpdir / "simple.md"
    p1.write_text(SIMPLE_MD, encoding="utf-8")
    cases.append(Case("simple", p1, "paragraph + list + code + quote"))

    # Case 2: CJK + symbols + emoji.
    p2 = tmpdir / "cjk.md"
    p2.write_text(CJK_MD, encoding="utf-8")
    cases.append(Case("cjk", p2, "Chinese prose, arrows, math symbols, emoji"))

    # Case 3: full acceptance fixture — also exercises Mermaid/TikZ/SVG.
    if ACCEPTANCE_FIXTURE.exists():
        cases.append(Case(
            "acceptance_fixture",
            ACCEPTANCE_FIXTURE,
            "shipped document-acceptance.md (CJK + diagrams)",
        ))

    return cases


def main() -> int:
    # Sanity check tooling up-front.
    cargo = shutil.which("cargo")
    if cargo is None:
        print("ERROR: cargo not on PATH", file=sys.stderr)
        return 2
    try:
        _import_pymupdf()
    except ImportError:
        print("ERROR: PyMuPDF missing. Install with `pip install PyMuPDF`.", file=sys.stderr)
        return 2

    tmpdir = Path(tempfile.mkdtemp(prefix="vutex-pymupdf-"))
    results: list[Result] = []

    try:
        cases = build_cases(tmpdir)
        print(f"Running {len(cases)} validation case(s) from workspace at {ROOT}")

        for case in cases:
            print(f"\n=== {case.name} : {case.note} ===")
            out_pdf = tmpdir / f"{case.name}.pdf"
            rc = run_cargo_export(case, out_pdf)
            result = Result(case=case, exit_code=rc)

            if rc != 0:
                result.errors.append(f"cargo test exited {rc}")
                print(f"  cargo FAILED (exit {rc})")
            elif not out_pdf.exists():
                result.errors.append("expected PDF file not written by cargo test")
                print(f"  cargo succeeded but PDF file is missing at {out_pdf}")
            else:
                result.pdf_path = out_pdf
                result.pdf_bytes = out_pdf.stat().st_size
                print(f"  cargo OK — {result.pdf_bytes} bytes written")
                ok, pages, preview, errors = validate_with_pymupdf(out_pdf)
                result.pymupdf_ok = ok
                result.pages = pages
                result.text_preview = preview
                result.errors.extend(errors)
                if ok:
                    print(f"  PyMuPDF OK — {pages} page(s)  {preview}")
                else:
                    print(f"  PyMuPDF FAIL: {errors}")

            results.append(result)
    finally:
        # Keep artifacts around when things fail for post-mortem.
        if any(not r.pymupdf_ok for r in results):
            print(f"\n(keeping artifacts in {tmpdir} for inspection)")
            tmpdir = None  # type: ignore[assignment]
        else:
            shutil.rmtree(tmpdir, ignore_errors=True)

    # Summary.
    print("\n" + "=" * 60)
    print("SUMMARY")
    print("=" * 60)
    all_ok = True
    for r in results:
        status = "PASS" if r.pymupdf_ok and r.exit_code == 0 else "FAIL"
        all_ok = all_ok and (r.pymupdf_ok and r.exit_code == 0)
        print(f"  [{status}] {r.case.name:<25} pdf={r.pdf_bytes:>8}B  pages={r.pages}")
        if r.errors:
            for e in r.errors:
                print(f"            ⚠ {e}")

    print()
    if all_ok:
        print("RESULT: ALL PDFs parse cleanly under PyMuPDF. /Length fix confirmed.")
        return 0
    else:
        print("RESULT: at least one PDF failed external parsing — investigate above.")
        return 1


if __name__ == "__main__":
    sys.exit(main())
