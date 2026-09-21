"""Offline checks for corpus integrity, failure handling and published statistics."""

import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.parse import quote
from xml.etree import ElementTree

from compare import (ROOT, aggregate, check_negative, compact_json, distribution,
                     prepare_artifact, sha256)


class BenchmarkTests(unittest.TestCase):
    def test_checksum_mismatch_is_not_silently_refetched(self):
        artifact = {"filename": "sample.step", "bytes": 4, "sha256": sha256(b"good")}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / artifact["filename"]
            path.write_bytes(b"oops")
            with patch("compare.urllib.request.urlopen") as fetch:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    prepare_artifact(artifact, Path(directory))
                fetch.assert_not_called()
            self.assertEqual(path.read_bytes(), b"oops")

    def test_bad_refresh_preserves_existing_cache(self):
        artifact = {"filename": "sample.step", "bytes": 4, "sha256": sha256(b"good"), "url": "https://example.invalid/file"}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / artifact["filename"]
            path.write_bytes(b"good")
            with patch("compare.urllib.request.urlopen") as fetch:
                fetch.return_value.__enter__.return_value.read.return_value = b"truncated"
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    prepare_artifact(artifact, Path(directory), refresh=True)
            self.assertEqual(path.read_bytes(), b"good")

    def test_negative_control_rejects_unrelated_failure_or_success(self):
        artifact = {"filename": "malformed.step", "sha256": "digest", "expected_error": "not a STEP Part 21 exchange file"}
        for result in (subprocess.CompletedProcess([], 0, b"{}", b""),
                       subprocess.CompletedProcess([], 1, b"", b"file not found"),
                       subprocess.CompletedProcess([], 1, b"{}", artifact["expected_error"].encode())):
            with self.assertRaisesRegex(ValueError, "did not fail as expected"):
                check_negative(result, artifact)
        rejected = subprocess.CompletedProcess([], 1, b"", artifact["expected_error"].encode())
        self.assertEqual(check_negative(rejected, artifact)["status"], "rejected_as_expected")

    def test_group_median_does_not_weight_by_repeat_count(self):
        files = [dict(input_bytes=100, ir_bytes=10, compact_summary_json_bytes=20,
                      cli_ms=distribution(samples)) for samples in ([1] * 11, [100])]
        self.assertEqual(aggregate(files)["cli_ms"], {"median": 50.5, "min": 1, "max": 100})

    def test_compact_size_is_utf8_not_escaped_unicode(self):
        self.assertEqual(compact_json({"name": "铜"}), '{"name":"铜"}'.encode("utf-8"))

    def test_public_manifest_is_pinned_and_download_names_are_safe(self):
        manifest = json.loads((ROOT / "benchmarks/public-artifacts.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["schema_version"], 2)
        names = [c["name"] for c in manifest["cases"]]
        self.assertEqual(len(names), len(set(names)))
        artifacts = [a for c in manifest["cases"] for a in c["artifacts"]] + manifest["negative_controls"]
        for artifact in artifacts:
            self.assertRegex(artifact["revision"], r"^[0-9a-f]{40}$")
            self.assertRegex(artifact["sha256"], r"^[0-9a-f]{64}$")
            self.assertGreater(artifact["bytes"], 0)
            self.assertNotRegex(artifact["filename"], r"[/\\:]")
            self.assertNotIn(artifact["filename"], ("", ".", ".."))
            self.assertIn(artifact["license"], ("MIT", "Apache-2.0"))
            suffix = f"{artifact['source_repo']}/{artifact['revision']}/{quote(artifact['source_path'], safe='/')}"
            self.assertEqual(artifact["url"], f"https://raw.githubusercontent.com/{suffix}")

    def test_published_measurements_match_manifest_and_raw_samples(self):
        manifest_path = ROOT / "benchmarks/public-artifacts.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        report = json.loads((ROOT / "artifacts/public-benchmark.json").read_text(encoding="utf-8"))
        self.assertEqual(report["manifest_sha256"], sha256(manifest_path.read_bytes()))
        self.assertEqual(report["runner_sha256"], sha256((ROOT / "benchmarks/compare.py").read_bytes()))
        self.assertEqual([r["case"] for r in report["results"]], [c["name"] for c in manifest["cases"]])
        all_files = []
        for case, row in zip(manifest["cases"], report["results"]):
            self.assertEqual(row["label"], case["label"])
            self.assertEqual(row["format"], case["format"])
            self.assertEqual([f["filename"] for f in row["files"]], [a["filename"] for a in case["artifacts"]])
            for source, file in zip(case["artifacts"], row["files"]):
                self.assertEqual(file["sha256"], source["sha256"])
                self.assertEqual(file["input_bytes"], source["bytes"])
                self.assertEqual(len(file["samples_ms"]), report["protocol"]["iterations_per_file"])
                self.assertTrue(all(v > 0 for v in file["samples_ms"]))
                self.assertEqual(file["cli_ms"], distribution(file["samples_ms"]))
                self.assertLessEqual(file["ir_bytes"], 16384)
                self.assertLess(file["ir_bytes"], file["compact_summary_json_bytes"])
            self.assertEqual(row["aggregate"], aggregate(row["files"]))
            all_files.extend(row["files"])
        self.assertEqual(report["aggregate"], aggregate(all_files))
        self.assertEqual([n["filename"] for n in report["negative_controls"]],
                         [n["filename"] for n in manifest["negative_controls"]])
        self.assertTrue(all(n["status"] == "rejected_as_expected" for n in report["negative_controls"]))

    def test_graph_has_accessible_labels_and_published_medians(self):
        report = json.loads((ROOT / "artifacts/public-benchmark.json").read_text(encoding="utf-8"))
        svg = ElementTree.parse(ROOT / "artifacts/public-benchmark.svg")
        self.assertEqual(svg.getroot().get("role"), "img")
        title = svg.find("{http://www.w3.org/2000/svg}title")
        self.assertIsNotNone(title)
        content = " ".join(svg.getroot().itertext())
        for row in report["results"]:
            self.assertIn(row["label"], content)
            self.assertIn(f"{row['aggregate']['cli_ms']['median']:.1f} ms", content)
            self.assertIn(f"{row['aggregate']['ir_bytes']['median'] / 1024:.2f} KiB", content)
        self.assertNotRegex(content, re.compile(r"C:\\Users|/Users/|/home/"))


if __name__ == "__main__":
    unittest.main()
