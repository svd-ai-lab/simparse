"""Measure single-file CLI inspection and IR size on pinned public artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import statistics
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from validate_ir import validate_document


def compact_json(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False,
                      sort_keys=True, separators=(",", ":")).encode("utf-8")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def distribution(values):
    if not values:
        raise ValueError("cannot aggregate an empty sample")
    return {"median": statistics.median(values), "min": min(values), "max": max(values)}


def verified_bytes(data, artifact):
    if len(data) != artifact["bytes"] or sha256(data) != artifact["sha256"]:
        raise ValueError(f"checksum mismatch: {artifact['filename']}")
    return data


def prepare_artifact(artifact, directory, refresh=False):
    path = directory / artifact["filename"]
    if path.exists() and not refresh:
        verified_bytes(path.read_bytes(), artifact)
        return path
    with urllib.request.urlopen(artifact["url"], timeout=120) as response:
        data = verified_bytes(response.read(), artifact)
    directory.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_bytes(data)
    temporary.replace(path)
    return path


def inspect(cli, path):
    start = time.perf_counter_ns()
    result = subprocess.run([str(cli), "inspect", str(path), "--json", "--summary"],
                            capture_output=True, timeout=120)
    elapsed_ms = (time.perf_counter_ns() - start) / 1e6
    return result, elapsed_ms


def checked_output(result, case):
    if result.returncode:
        raise ValueError(f"inspection failed: {result.stderr.decode('utf-8', errors='replace')}")
    data = json.loads(result.stdout)
    if data.get("ok") is not True or data.get("format") != case["format"]:
        raise ValueError("inspection did not return the expected format and success status")
    if data.get("view") != "summary":
        raise ValueError("expected the summary view")
    if "expected_design_type" in case:
        designs = data["summary"]["data"]["designs"]["sample"]
        if not designs or any(d["design_type"] != case["expected_design_type"] for d in designs):
            raise ValueError("AEDT design type does not match the plotted software group")
    ir = data["ir"]
    report = validate_document(ir)
    if report["errors"] or len(compact_json(ir)) > 16384:
        raise ValueError(f"invalid or oversized IR: {report['errors']}")
    return data


def check_negative(result, artifact):
    error = result.stderr.decode("utf-8", errors="replace")
    if result.returncode == 0 or result.stdout.strip() or artifact["expected_error"] not in error:
        raise ValueError(f"negative control did not fail as expected: {artifact['filename']}")
    return {"filename": artifact["filename"], "sha256": artifact["sha256"],
            "status": "rejected_as_expected", "error": artifact["expected_error"]}


def output_metrics(data, wire_bytes):
    ir = compact_json(data["ir"])
    source = data["summary"]["data"]
    dialects = data["ir"]["dialects"]
    return {
        "ir_bytes": len(ir), "ir_sha256": sha256(ir),
        "compact_summary_json_bytes": len(compact_json(data)),
        "cli_stdout_bytes": wire_bytes,
        "warning_count": data["warnings"]["total"],
        "source_truncated": source.get("source_truncated", source.get("truncated", False)),
        "ir_dialects": list(dialects),
        "ir_feature_count": sum(len(d.get("features", [])) for d in dialects.values()),
    }


def aggregate(files):
    """Give every file equal weight, irrespective of its size or repeat count."""
    return {
        "file_count": len(files),
        "input_bytes_total": sum(f["input_bytes"] for f in files),
        "input_bytes": distribution([f["input_bytes"] for f in files]),
        "cli_ms": distribution([f["cli_ms"]["median"] for f in files]),
        "ir_bytes": distribution([f["ir_bytes"] for f in files]),
        "compact_summary_json_bytes": distribution([f["compact_summary_json_bytes"] for f in files]),
    }


def environment():
    cpu = platform.processor() or platform.machine()
    if platform.system() == "Windows":
        cpu = subprocess.check_output([
            "pwsh", "-NoProfile", "-Command",
            "(Get-CimInstance Win32_Processor | Select-Object -First 1).Name"
        ], text=True, timeout=30).strip()
    elif Path("/proc/cpuinfo").exists():
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    return {"os": platform.system(), "os_release": platform.release(),
            "architecture": platform.machine(), "cpu": cpu, "logical_cpus": os.cpu_count(),
            "python": platform.python_version(),
            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
            "build_profile": "release", "filesystem_cache": "warm; no cache flushing"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=ROOT / "benchmarks/public-artifacts.json")
    parser.add_argument("--cache-dir", type=Path, default=ROOT / "target/benchmark-artifacts")
    parser.add_argument("--out", type=Path, default=ROOT / "artifacts/public-benchmark.json")
    parser.add_argument("--iterations", type=int, default=11)
    parser.add_argument("--seed", type=int, default=1729)
    parser.add_argument("--case", action="append", dest="cases")
    parser.add_argument("--refresh", action="store_true")
    args = parser.parse_args()
    if args.iterations < 3:
        parser.error("--iterations must be at least 3")
    manifest_bytes = args.manifest.read_bytes()
    manifest = json.loads(manifest_bytes)
    if manifest["schema_version"] != 2:
        parser.error("expected public artifact manifest version 2")
    known = {case["name"] for case in manifest["cases"]}
    if args.cases and set(args.cases) - known:
        parser.error(f"unknown cases: {sorted(set(args.cases) - known)}")
    cases = [c for c in manifest["cases"] if not args.cases or c["name"] in args.cases]
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", "crates", "Cargo.toml", "Cargo.lock"],
                   cwd=ROOT, check=True, capture_output=True)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "simparse-cli"], cwd=ROOT, check=True)
    cli = ROOT / "target/release" / ("simparse.exe" if os.name == "nt" else "simparse")
    host = environment()
    records = []
    for case in cases:
        for source in case["artifacts"]:
            path = prepare_artifact(source, args.cache_dir / case["name"], args.refresh)
            records.append({"case": case, "source": source, "path": path, "samples_ms": []})

    negatives = []
    for source in manifest.get("negative_controls", []):
        path = prepare_artifact(source, args.cache_dir / "negative_controls", args.refresh)
        result, _ = inspect(cli, path)
        negatives.append(check_negative(result, source))

    # One excluded warmup per file checks output before collecting timings.
    for record in records:
        result, _ = inspect(cli, record["path"])
        data = checked_output(result, record["case"])
        record["output_hash"] = sha256(compact_json(data))
        record["metrics"] = output_metrics(data, len(result.stdout))

    rng = random.Random(args.seed)
    order = list(range(len(records)))
    for round_index in range(args.iterations):
        rng.shuffle(order)
        for index in order:
            record = records[index]
            result, elapsed_ms = inspect(cli, record["path"])
            if result.returncode or sha256(compact_json(json.loads(result.stdout))) != record["output_hash"]:
                raise ValueError(f"inspection failed or changed between repeats: {record['source']['filename']}")
            record["samples_ms"].append(elapsed_ms)
        print(f"Round {round_index + 1}/{args.iterations}: {len(records)} files inspected", flush=True)

    results = []
    all_files = []
    for case in cases:
        files = []
        for record in records:
            if record["case"]["name"] != case["name"]:
                continue
            source = record["source"]
            files.append({"filename": source["filename"], "source_url": source["source_url"],
                          "sha256": source["sha256"], "input_bytes": source["bytes"],
                          "samples_ms": record["samples_ms"],
                          "cli_ms": distribution(record["samples_ms"]), **record["metrics"]})
        all_files.extend(files)
        results.append({"case": case["name"], "label": case["label"], "format": case["format"],
                        "aggregate": aggregate(files), "files": files})
    artifact = {
        "schema_version": 2, "benchmark": "simparse public single-file inspection",
        "generated_at_utc": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "source_revision": revision, "runner_sha256": sha256(Path(__file__).read_bytes()),
        "manifest_sha256": sha256(manifest_bytes), "environment": host,
        "protocol": {
            "command": "simparse inspect <file> --json --summary",
            "iterations_per_file": args.iterations, "warmups_per_file": 1, "order_seed": args.seed,
            "order": "one invocation per file per round; deterministic shuffled order; serial execution",
            "timing": "wall time around fresh subprocess, including startup, extraction and stdout capture; excludes download, build, JSON decoding and validation",
            "row_latency": "median of per-file median times; whiskers span minimum to maximum per-file median",
            "ir_size": "compact UTF-8 JSON of the ir field only, ensure_ascii=False; includes reference handles, excludes external source artifacts",
            "size_range": "minimum to maximum across files; median used for the bar",
            "scope": "shallow metadata inspection; not full model parsing, solver execution, extraction accuracy or lossless compression",
        },
        "aggregate": aggregate(all_files), "negative_controls": negatives, "results": results,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(artifact, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    from plot import render
    render(artifact, args.out.with_suffix(".svg"), args.out.with_suffix(".png"))
    print(json.dumps(artifact["aggregate"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
