#!/usr/bin/env python3
"""Summarize JaCoCo coverage gaps for the codegen coverage run.

Usage:
  python3 report_gaps.py                 # overall + per-package summary
  python3 report_gaps.py --files [N]     # N worst-covered files (default 30)
  python3 report_gaps.py --file <name>   # missed line numbers for a source file
  python3 report_gaps.py --zero          # files with 0% coverage
"""

import sys
import xml.etree.ElementTree as ET
from pathlib import Path

REPORT = Path(__file__).parent / "build/reports/jacoco/coverage.xml"


def load():
    return ET.parse(REPORT).getroot()


def counters(elem, kind="LINE"):
    for c in elem.findall("counter"):
        if c.get("type") == kind:
            return int(c.get("missed")), int(c.get("covered"))
    return 0, 0


def pct(missed, covered):
    total = missed + covered
    return 100.0 * covered / total if total else 100.0


def overall(root):
    m, c = counters(root)
    print(f"OVERALL LINE COVERAGE: {pct(m, c):.2f}%  (covered={c}, missed={m}, total={m + c})")
    print()
    rows = []
    for pkg in root.findall("package"):
        m, c = counters(pkg)
        if m + c:
            rows.append((pct(m, c), m, c, pkg.get("name")))
    rows.sort()
    print(f"{'line%':>7}  {'missed':>7}  {'covered':>8}  package")
    for p, m, c, name in rows:
        print(f"{p:7.2f}  {m:7}  {c:8}  {name}")


def worst_files(root, n):
    rows = []
    for pkg in root.findall("package"):
        for sf in pkg.findall("sourcefile"):
            m, c = counters(sf)
            if m:
                rows.append((pct(m, c), m, c, f"{pkg.get('name')}/{sf.get('name')}"))
    rows.sort(key=lambda r: -r[1])
    print(f"{'line%':>7}  {'missed':>7}  {'covered':>8}  file")
    for p, m, c, name in rows[:n]:
        print(f"{p:7.2f}  {m:7}  {c:8}  {name}")
    print(f"\n{len(rows)} files have missed lines")


def zero_files(root):
    for pkg in root.findall("package"):
        for sf in pkg.findall("sourcefile"):
            m, c = counters(sf)
            if m and not c:
                print(f"{m:5}  {pkg.get('name')}/{sf.get('name')}")


def file_detail(root, name):
    for pkg in root.findall("package"):
        for sf in pkg.findall("sourcefile"):
            if sf.get("name") == name or f"{pkg.get('name')}/{sf.get('name')}" == name:
                missed = [ln.get("nr") for ln in sf.findall("line") if int(ln.get("ci")) == 0]
                m, c = counters(sf)
                print(f"{pkg.get('name')}/{sf.get('name')}: {pct(m, c):.2f}% covered")
                print("missed lines:", ", ".join(missed))
                return
    print(f"file not found: {name}", file=sys.stderr)
    sys.exit(1)


def main():
    root = load()
    args = sys.argv[1:]
    if not args:
        overall(root)
    elif args[0] == "--files":
        worst_files(root, int(args[1]) if len(args) > 1 else 30)
    elif args[0] == "--zero":
        zero_files(root)
    elif args[0] == "--file":
        file_detail(root, args[1])
    else:
        print(__doc__)
        sys.exit(1)


if __name__ == "__main__":
    main()
