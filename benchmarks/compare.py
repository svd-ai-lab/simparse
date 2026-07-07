"""Benchmark simparse against small public artifact readers.

The repository stores only source URLs in ``public-artifacts.json``. This script
downloads those files into ``target/`` and writes a local comparison artifact.
"""

from __future__ import annotations

import argparse
import html
import json
import statistics
import subprocess
import sys
import time
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "benchmarks" / "public-artifacts.json"
DEFAULT_OUT = ROOT / "artifacts" / "public-benchmark.json"
DEFAULT_CACHE = ROOT / "target" / "benchmark-artifacts"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--cache-dir", type=Path, default=DEFAULT_CACHE)
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument("--case", action="append", dest="cases")
    parser.add_argument("--refresh", action="store_true")
    parser.add_argument("--svg-out", type=Path)
    parser.add_argument("--python-worker", choices=sorted(BASELINES), help=argparse.SUPPRESS)
    parser.add_argument("--python-worker-dir", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()

    if args.python_worker:
        if args.python_worker_dir is None:
            raise SystemExit("--python-worker-dir is required with --python-worker")
        count = BASELINES[args.python_worker](args.python_worker_dir)
        print(json.dumps({"count": count}))
        return 0

    manifest = load_manifest(args.manifest)
    cases = select_cases(manifest["cases"], args.cases)
    build_cli()

    results = []
    for case in cases:
        prepared = prepare_case(case, args.cache_dir, args.refresh)
        simparse_ms = measure(
            lambda c=case, p=prepared: run_simparse_scan(p["dir"], c["include"]),
            args.iterations,
        )

        baseline_kind = case.get("python_baseline")
        baseline_status, tool_call_ms, in_process_ms, tool_speedup, in_process_speedup = run_baseline(
            baseline_kind,
            prepared["dir"],
            simparse_ms,
            args.iterations,
        )

        results.append(
            {
                "case": case["name"],
                "format": case["format"],
                "artifacts": prepared["artifacts"],
                "simparse_cli_ms": simparse_ms,
                "python_tool_call_ms": tool_call_ms,
                "python_in_process_ms": in_process_ms,
                "python_baseline_status": baseline_status,
                "simparse_vs_python_tool_call_speedup": tool_speedup,
                "simparse_vs_python_in_process_speedup": in_process_speedup,
                "vendor_native_ms": None,
                "vendor_native_status": "not_run",
                "vendor_native_note": "requires locally licensed vendor software or APIs",
            }
        )

    artifact = {
        "benchmark": "simparse public artifact scan comparison",
        "generated_at_utc": datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "artifact_manifest": "benchmarks/public-artifacts.json",
        "source_policy": "public URLs only; downloaded files are cached under target/ and are not committed",
        "workload": {
            "iterations": args.iterations,
            "primary_scenario": "agent tool call: one external command scans one public artifact group",
            "simparse_tool_call": "target/release/simparse scan <case-cache-dir> --jsonl --include <patterns>",
            "python_tool_call": "python benchmarks/compare.py --python-worker <baseline> --python-worker-dir <case-cache-dir>",
            "python_in_process": "same Python baseline function called inside the benchmark process",
        },
        "results": results,
    }
    assert_no_private_paths(artifact)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    svg_out = args.svg_out or args.out.with_suffix(".svg")
    write_svg(artifact, svg_out)
    print(args.out.relative_to(ROOT))
    print(svg_out.relative_to(ROOT))
    return 0


def load_manifest(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def select_cases(cases: list[dict], selected: list[str] | None) -> list[dict]:
    if not selected:
        return cases
    names = set(selected)
    missing = names - {case["name"] for case in cases}
    if missing:
        raise SystemExit(f"unknown benchmark case(s): {', '.join(sorted(missing))}")
    return [case for case in cases if case["name"] in names]


def build_cli() -> None:
    subprocess.run(
        ["cargo", "build", "--release", "-p", "simparse-cli"],
        cwd=ROOT,
        check=True,
    )


def prepare_case(case: dict, cache_dir: Path, refresh: bool) -> dict:
    case_dir = cache_dir / case["name"]
    case_dir.mkdir(parents=True, exist_ok=True)

    artifacts = []
    for item in case["artifacts"]:
        path = case_dir / item["filename"]
        if refresh or not path.exists():
            download(item["url"], path)
        artifacts.append(
            {
                "filename": item["filename"],
                "bytes": path.stat().st_size,
                "source_url": item["source_url"],
                "source_repo": item["source_repo"],
                "license": item["license"],
            }
        )

    return {"dir": case_dir, "artifacts": artifacts}


def download(url: str, out: Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    tmp = out.with_suffix(out.suffix + ".tmp")
    request = urllib.request.Request(url, headers={"User-Agent": "simparse-benchmark"})
    with urllib.request.urlopen(request, timeout=120) as response:
        tmp.write_bytes(response.read())
    tmp.replace(out)


def run_simparse_scan(path: Path, include: str) -> int:
    exe = ROOT / "target" / "release" / exe_name("simparse")
    output = subprocess.run(
        [str(exe), "scan", str(path), "--jsonl", "--include", include],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    ).stdout
    records = [json.loads(line) for line in output.splitlines() if line.strip()]
    for record in records:
        if record.get("path") is not None:
            raise RuntimeError("simparse benchmark output unexpectedly included a path")
    return len(records)


def run_baseline(
    kind: str | None,
    path: Path,
    simparse_ms: dict[str, float],
    iterations: int,
) -> tuple[
    str,
    dict[str, float] | None,
    dict[str, float] | None,
    float | None,
    float | None,
]:
    if kind is None:
        return "not_configured", None, None, None, None
    if kind == "fluent_hdf5_h5py" and not has_h5py():
        return "not_available: h5py is not installed", None, None, None, None

    baseline = BASELINES[kind]
    tool_call_ms = measure(lambda: run_python_worker(kind, path), iterations)
    in_process_ms = measure(lambda: baseline(path), iterations)
    tool_speedup = tool_call_ms["median_ms"] / simparse_ms["median_ms"]
    in_process_speedup = in_process_ms["median_ms"] / simparse_ms["median_ms"]
    return "ok", tool_call_ms, in_process_ms, round(tool_speedup, 3), round(in_process_speedup, 3)


def run_python_worker(kind: str, path: Path) -> int:
    output = subprocess.run(
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "--python-worker",
            kind,
            "--python-worker-dir",
            str(path),
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    ).stdout
    return int(json.loads(output)["count"])


def measure(fn, iterations: int) -> dict[str, float]:
    values = []
    for _ in range(iterations):
        start = time.perf_counter()
        fn()
        values.append((time.perf_counter() - start) * 1000)
    return {
        "median_ms": round(statistics.median(values), 3),
        "min_ms": round(min(values), 3),
        "max_ms": round(max(values), 3),
    }


def baseline_abaqus(path: Path) -> int:
    total = 0
    for file in path.glob("*.inp"):
        current = ""
        for raw in file.read_text(encoding="utf-8", errors="replace").splitlines():
            line = raw.strip()
            if line.startswith("*"):
                current = line.split(",", 1)[0].upper()
            elif current in {"*NODE", "*ELEMENT"} and line and not line.startswith("**"):
                total += 1
    return total


def baseline_comsol(path: Path) -> int:
    total = 0
    for file in path.glob("*.mph"):
        with zipfile.ZipFile(file) as archive:
            names = archive.namelist()
            for member in ("fileversion", "modelinfo.xml", "dmodel.xml"):
                if member in names:
                    payload = archive.read(member)
                    if member.endswith(".xml"):
                        ElementTree.fromstring(payload)
            total += len(names)
    return total


def baseline_hfss(path: Path) -> int:
    total = 0
    markers = (
        "ProjectName",
        "DesignName",
        "$begin 'AnalysisSetup'",
        "$begin 'Optimetrics'",
        "$begin 'Boundaries'",
        "$begin 'Excitations'",
    )
    for file in list(path.glob("*.aedt")) + list(path.glob("*.aedtz")):
        if file.suffix.lower() == ".aedtz":
            total += baseline_hfss_aedtz(file, markers)
        else:
            text = file.read_text(encoding="utf-8", errors="replace")
            total += sum(text.count(marker) for marker in markers)
    return total


def baseline_hfss_aedtz(path: Path, markers: tuple[str, ...]) -> int:
    total = 0
    with zipfile.ZipFile(path) as archive:
        for name in archive.namelist():
            if name.lower().endswith(".aedt"):
                text = archive.read(name).decode("utf-8", errors="replace")
                total += sum(text.count(marker) for marker in markers)
    return total


def baseline_hdf5(path: Path) -> int:
    import h5py  # type: ignore

    total = 0
    for file in list(path.glob("*.cas.h5")) + list(path.glob("*.msh.h5")):
        with h5py.File(file, "r") as h5:
            total += len(h5.attrs)

            def visit(_name, obj):
                nonlocal total
                total += 1 + len(obj.attrs)

            h5.visititems(visit)
    return total


def has_h5py() -> bool:
    try:
        import h5py  # noqa: F401
    except Exception:
        return False
    return True


def exe_name(name: str) -> str:
    return f"{name}.exe" if sys.platform == "win32" else name


def assert_no_private_paths(value: dict) -> None:
    text = json.dumps(value)
    private_markers = ("C:\\\\Users\\\\", "C:/Users/", "\\\\Users\\\\", "/Users/", "/home/")
    found = [marker for marker in private_markers if marker in text]
    if found:
        raise RuntimeError(f"private path marker found in benchmark artifact: {found[0]}")


def write_svg(artifact: dict, path: Path) -> None:
    rows = [
        row
        for row in artifact["results"]
        if row.get("python_tool_call_ms") is not None
    ]
    if not rows:
        return

    max_ms = max(
        max(row["simparse_cli_ms"]["median_ms"], row["python_tool_call_ms"]["median_ms"])
        for row in rows
    )
    width = 760
    left = 155
    bar_width = 435
    top = 72
    row_height = 70
    height = top + row_height * len(rows) + 54

    def scale(ms: float) -> float:
        return max(1.0, (ms / max_ms) * bar_width)

    def fmt(ms: float) -> str:
        return f"{ms:.1f} ms"

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">',
        "<title id=\"title\">simparse agent tool-call benchmark</title>",
        "<desc id=\"desc\">Median time for one external command scanning each public artifact group. Lower is faster.</desc>",
        '<rect width="100%" height="100%" fill="#ffffff"/>',
        '<text x="24" y="28" font-family="Arial, sans-serif" font-size="18" font-weight="700" fill="#111827">Agent tool-call scan benchmark</text>',
        '<text x="24" y="50" font-family="Arial, sans-serif" font-size="12" fill="#4b5563">Median time for one external command scanning each public artifact group. Lower is faster.</text>',
        '<rect x="24" y="61" width="12" height="12" fill="#2563eb" rx="2"/>',
        '<text x="42" y="71" font-family="Arial, sans-serif" font-size="12" fill="#374151">simparse CLI</text>',
        '<rect x="145" y="61" width="12" height="12" fill="#f97316" rx="2"/>',
        '<text x="163" y="71" font-family="Arial, sans-serif" font-size="12" fill="#374151">Python tool script</text>',
    ]

    for index, row in enumerate(rows):
        y = top + index * row_height
        sim_ms = row["simparse_cli_ms"]["median_ms"]
        py_ms = row["python_tool_call_ms"]["median_ms"]
        speedup = row["simparse_vs_python_tool_call_speedup"]
        label = html.escape(row["case"].replace("_", " "))
        parts.extend(
            [
                f'<text x="24" y="{y + 16}" font-family="Arial, sans-serif" font-size="13" font-weight="700" fill="#111827">{label}</text>',
                f'<rect x="{left}" y="{y}" width="{scale(sim_ms):.1f}" height="16" fill="#2563eb" rx="3"/>',
                f'<text x="{left + scale(sim_ms) + 8:.1f}" y="{y + 12}" font-family="Arial, sans-serif" font-size="12" fill="#111827">{fmt(sim_ms)}</text>',
                f'<rect x="{left}" y="{y + 24}" width="{scale(py_ms):.1f}" height="16" fill="#f97316" rx="3"/>',
                f'<text x="{left + scale(py_ms) + 8:.1f}" y="{y + 36}" font-family="Arial, sans-serif" font-size="12" fill="#111827">{fmt(py_ms)}</text>',
                f'<text x="650" y="{y + 26}" font-family="Arial, sans-serif" font-size="13" fill="#374151">{speedup:.2f}x</text>',
            ]
        )

    parts.append("</svg>\n")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(parts), encoding="utf-8")


BASELINES = {
    "abaqus_keywords": baseline_abaqus,
    "comsol_zip_xml": baseline_comsol,
    "fluent_hdf5_h5py": baseline_hdf5,
    "hfss_text_scan": baseline_hfss,
}


if __name__ == "__main__":
    raise SystemExit(main())
