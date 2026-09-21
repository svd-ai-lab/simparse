import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("validate_ir", ROOT / "scripts/validate_ir.py")
ir = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ir)


def package():
    return {
        "schema": "ai-infra-system/v0",
        "dialects": {
            "cad": {
                "version": 0, "source": {"uri": "package.step"}, "length_unit": "mm",
                "entities": [{"id": "die", "kind": "body"},
                             {"id": "lid", "kind": "body"},
                             {"id": "top", "kind": "face", "parent_id": "die"}],
            },
            "cae": {
                "version": 0, "source": {"uri": "package.mph"},
                "cad": {"uri": "#/dialects/cad"},
                "features": [{"id": "heat", "kind": "physics", "enabled": True},
                             {"id": "contact", "kind": "interface", "parent_id": "heat",
                              "selection": {"entity_ids": ["top"]}}],
            },
        },
    }


class ContractTests(unittest.TestCase):
    def assert_valid(self, doc, **kwargs):
        report = ir.validate_document(doc, **kwargs)
        self.assertEqual(report, {"errors": [], "unresolved": []})

    def test_examples(self):
        for path in sorted((ROOT / "examples/ir").glob("*.json")):
            with self.subTest(example=path.name):
                self.assert_valid(ir.read_manifest(path), base_dir=path.parent)

    def test_minimal_and_single_dialects(self):
        for dialect in ("cad", "cae"):
            self.assert_valid({"schema": "ai-infra-system/v0", "dialects": {
                dialect: {"version": 0, "source": {"uri": "component.bin"}}}})

    def test_scoped_parameters_and_expressions_are_preserved(self):
        doc = package()
        params = [{"name": "thickness", "scope": scope, "expression": "2*t_ref", "unit": "um"}
                  for scope in ("die1", "die2")]
        doc["dialects"]["cae"]["parameters"] = params
        original = copy.deepcopy(doc)
        self.assert_valid(doc)
        self.assertEqual(doc, original)
        params[1]["scope"] = "die1"
        self.assertTrue(ir.validate_document(doc)["errors"])

    def test_false_zero_and_unknown_are_distinct(self):
        doc = package()
        doc["dialects"]["cad"]["counts"] = {"mesh": 0}
        features = doc["dialects"]["cae"]["features"]
        features[0]["enabled"] = False
        self.assertNotIn("enabled", features[1])
        self.assert_valid(doc)

    def test_opaque_software_extensions_round_trip(self):
        for namespace in ("comsol", "ansys.mechanical", "freecad", "new_vendor"):
            doc = package()
            doc["dialects"]["cae"]["features"][0]["extensions"] = {
                namespace: {"version": "future-2", "attributes": {"order": 2, "active": False},
                            "artifact": {"uri": "details.json"}}}
            original = copy.deepcopy(doc)
            self.assert_valid(doc)
            self.assertEqual(json.loads(json.dumps(doc)), original)

    def test_native_and_external_selections_need_no_cad_ids(self):
        for selection in ({"native_ref": "named-boundary"},
                          {"artifact": {"uri": "large-selection.jsonl"}}):
            doc = package()
            del doc["dialects"]["cae"]["cad"]
            doc["dialects"]["cae"]["features"][1]["selection"] = selection
            self.assert_valid(doc)

    def test_negative_schema_cases(self):
        cases = [
            (("schema",), "physical-system/v0"),
            (("dialects",), {}),
            (("dialects", "cad", "version"), 1),
            (("dialects", "cad", "counts"), {"face": -1}),
            (("dialects", "cad", "length_unit"), None),
            (("dialects", "cad", "source", "uri"), "DaTa:application/json,{}"),
            (("dialects", "cad", "source", "uri"), "file with spaces.step"),
            (("dialects", "cae", "source", "sha256"), "bad"),
            (("dialects", "cae", "features", 0, "enabled"), None),
            (("dialects", "cae", "features", 0, "enabled"), "false"),
            (("dialects", "cad", "vertices"), [[0, 0, 0]]),
            (("dialects", "cae", "mesh"), {"uri": "mesh.h5", "nodes": [[0, 0, 0]]}),
            (("dialects", "cae", "features", 1, "selection", "entity_ids"), []),
            (("dialects", "cae", "features", 1, "selection", "entity_ids"), ["top", "top"]),
            (("extensions",), {"COMSOL": {"version": "0"}}),
            (("extensions",), {"comsol": {"attributes": {"active": True}}}),
            (("extensions",), {"comsol": {"version": "0", "attributes": {"mesh": [1, 2, 3]}}}),
            (("extensions",), {"comsol": {"version": "0", "attributes": {"tree": {"nested": 1}}}}),
            (("extensions",), {"comsol": {"version": "0", "attributes": {"dump": "x" * 1025}}}),
        ]
        for path, value in cases:
            with self.subTest(path=path, value=str(value)[:80]):
                doc = package()
                target = doc
                for key in path[:-1]:
                    target = target[key]
                target[path[-1]] = value
                self.assertTrue(ir.validate_document(doc)["errors"])

    def test_inline_limits_and_manifest_byte_limit(self):
        doc = package()
        doc["dialects"]["cad"]["entities"] = [{"id": str(i), "kind": "body"} for i in range(65)]
        self.assertTrue(ir.validate_document(doc)["errors"])
        doc = package()
        doc["dialects"]["cae"]["parameters"] = [{"name": "large", "expression": "x" * ir.MAX_MANIFEST_BYTES}]
        self.assertTrue(ir.validate_document(doc)["errors"])

    def test_duplicate_ids_and_containment_cycles(self):
        for dialect, group in (("cad", "entities"), ("cae", "features")):
            for mutation in ("duplicate", "cycle", "dangling"):
                with self.subTest(dialect=dialect, mutation=mutation):
                    doc = package()
                    records = doc["dialects"][dialect][group]
                    if mutation == "duplicate":
                        records.append(copy.deepcopy(records[0]))
                    elif mutation == "cycle":
                        records[0]["parent_id"] = records[1]["id"]
                        records[1]["parent_id"] = records[0]["id"]
                    else:
                        records[0]["parent_id"] = "missing"
                    self.assertTrue(ir.validate_document(doc)["errors"])

    def test_missing_cad_and_dangling_selection(self):
        for mutation in ("no-ref", "no-cad", "wrong-fragment", "missing-entity", "self-hash"):
            doc = package()
            cae = doc["dialects"]["cae"]
            if mutation == "no-ref":
                del cae["cad"]
            elif mutation == "no-cad":
                del doc["dialects"]["cad"]
            elif mutation == "wrong-fragment":
                cae["cad"]["uri"] = "#/dialects/cae"
            elif mutation == "self-hash":
                cae["cad"]["sha256"] = "a" * 64
            else:
                cae["features"][1]["selection"]["entity_ids"] = ["wrong-revision-face"]
            with self.subTest(mutation=mutation):
                self.assertTrue(ir.validate_document(doc)["errors"])

    def test_external_inventory_is_unresolved_not_proven_absent(self):
        doc = package()
        doc["dialects"]["cad"]["entities"] = []
        doc["dialects"]["cad"]["inventory"] = {"uri": "full-entities.jsonl"}
        report = ir.validate_document(doc)
        self.assertFalse(report["errors"])
        self.assertTrue(report["unresolved"])

    def test_remote_cad_is_not_fetched(self):
        doc = package()
        doc["dialects"]["cae"]["cad"] = {"uri": "https://example.invalid/cad.json#/dialects/cad"}
        self.assertTrue(ir.validate_document(doc)["unresolved"])

    def test_external_cad_hash_and_encoded_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "package.step").write_bytes(b"synthetic CAD payload")
            (base / "package.mph").write_bytes(b"synthetic CAE payload")
            external = package()
            del external["dialects"]["cae"]
            path = base / "geometry model.json"
            path.write_text(json.dumps(external), encoding="utf-8")
            doc = package()
            del doc["dialects"]["cad"]
            doc["dialects"]["cae"]["cad"] = {
                "uri": "geometry%20model.json#/dialects/cad",
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            self.assert_valid(doc, base_dir=base, check_artifacts=True)
            path.write_text(json.dumps(external, indent=2), encoding="utf-8")
            self.assertIn("hash mismatch", " ".join(ir.validate_document(doc, base, True)["errors"]))
            path.unlink()
            self.assertTrue(ir.validate_document(doc, base, True)["errors"])

    def test_extension_attribute_named_uri_is_not_an_artifact(self):
        doc = package()
        doc["extensions"] = {"vendor": {"version": "0", "attributes": {"uri": "opaque-native-label"}}}
        refs = list(ir.artifact_references(doc))
        self.assertFalse(any(ref["uri"] == "opaque-native-label" for _, ref in refs))

    def test_external_cad_source_hash_is_also_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source geometry.step"
            source.write_bytes(b"revision one")
            external = package()
            del external["dialects"]["cae"]
            external["dialects"]["cad"]["source"] = {
                "uri": source.as_uri(), "sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
            (base / "geometry.json").write_text(json.dumps(external), encoding="utf-8")
            (base / "package.mph").write_bytes(b"synthetic")
            doc = package()
            del doc["dialects"]["cad"]
            doc["dialects"]["cae"]["cad"] = {"uri": "geometry.json#/dialects/cad"}
            self.assert_valid(doc, base_dir=base, check_artifacts=True)
            source.write_bytes(b"revision two")
            report = ir.validate_document(doc, base, True)
            self.assertTrue(any("external CAD" in e and "hash mismatch" in e for e in report["errors"]))

    def test_result_headers_and_duplicate_column_indices(self):
        doc = package()
        result = {"id": "temperatures", "data": {"uri": "results.csv"}, "columns": [
            {"index": 0, "header": "x (mm)", "unit": "mm"},
            {"index": 1, "header": "T (K)", "unit": "K"}]}
        doc["dialects"]["cae"]["results"] = [result]
        self.assert_valid(doc)
        result["columns"][1]["index"] = 0
        self.assertTrue(ir.validate_document(doc)["errors"])

    def test_invalid_json_duplicate_keys_and_nonfinite_numbers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bad.json"
            for text in ('{"schema":"a","schema":"b"}', '{"number":NaN}', '{"number":Infinity}'):
                path.write_text(text, encoding="utf-8")
                with self.assertRaises(ValueError):
                    ir.read_manifest(path)
        doc = package()
        doc["extensions"] = {"vendor": {"version": "0", "attributes": {"value": float("nan")}}}
        self.assertTrue(ir.validate_document(doc)["errors"])


if __name__ == "__main__":
    unittest.main()
