"""Offline validation for the draft AI Infra System IR, separate from simparse APIs."""

import argparse
import hashlib
import json
from pathlib import Path
from urllib.parse import unquote, urlsplit
from urllib.request import url2pathname

from jsonschema import Draft202012Validator, FormatChecker, ValidationError
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1]
MAX_MANIFEST_BYTES = 64 * 1024


def make_validator():
    schemas = [json.loads(path.read_text(encoding="utf-8"))
               for path in sorted((ROOT / "schemas").glob("*.schema.json"))]
    registry = Registry()
    for schema in schemas:
        Draft202012Validator.check_schema(schema)
        registry = registry.with_resource(schema["$id"], Resource.from_contents(schema))
    core = next(schema for schema in schemas if schema["$id"] == "urn:simparse:ai-infra-system:v0")
    return Draft202012Validator(core, registry=registry, format_checker=FormatChecker())


def read_manifest(path):
    if path.stat().st_size > MAX_MANIFEST_BYTES:
        raise ValueError("manifest exceeds byte limit; reference large inventories externally")

    def unique_keys(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    def invalid_constant(value):
        raise ValueError(f"non-JSON numeric constant: {value}")

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_keys,
                      parse_constant=invalid_constant)


def artifact_references(document):
    def extensions(obj, location):
        for name, extension in obj.get("extensions", {}).items():
            if "artifact" in extension:
                yield f"{location}/extensions/{name}/artifact", extension["artifact"]

    yield from extensions(document, "")
    for name, dialect in document["dialects"].items():
        location = f"/dialects/{name}"
        for key in ("source", "geometry", "inventory", "cad", "setup", "mesh", "result_inventory"):
            if key in dialect:
                yield f"{location}/{key}", dialect[key]
        yield from extensions(dialect, location)
        for group in ("entities", "features", "results"):
            for i, record in enumerate(dialect.get(group, [])):
                record_location = f"{location}/{group}/{i}"
                yield from extensions(record, record_location)
                if "data" in record:
                    yield f"{record_location}/data", record["data"]
                if "artifact" in record.get("selection", {}):
                    yield f"{record_location}/selection/artifact", record["selection"]["artifact"]


def local_path(uri, base_dir):
    parts = urlsplit(uri)
    if parts.scheme == "file" and parts.netloc in ("", "localhost") and not parts.query:
        path = Path(url2pathname(parts.path))
        return path.resolve() if path.is_absolute() else None
    if parts.scheme or parts.netloc or parts.query:
        return None
    return (base_dir / unquote(parts.path)).resolve() if parts.path else None


def validate_document(document, base_dir=None, check_artifacts=False):
    """Return errors and unresolved checks. Never fetch remote URIs or modify inputs."""
    errors, unresolved = [], []
    report = {"errors": errors, "unresolved": unresolved}
    base_dir = Path(base_dir or ".").resolve()
    try:
        size = len(json.dumps(document, ensure_ascii=False, allow_nan=False).encode("utf-8"))
    except (TypeError, ValueError) as exc:
        errors.append(str(exc))
        return report
    if size > MAX_MANIFEST_BYTES:
        errors.append("manifest exceeds byte limit; reference large inventories externally")
    validator = make_validator()
    for error in validator.iter_errors(document):
        location = "/" + "/".join(map(str, error.absolute_path))
        errors.append(f"{location}: {error.message}")
    if errors:
        return report

    def unique(records, key, label):
        values = [record[key] for record in records]
        if len(values) != len(set(values)):
            errors.append(f"{label}: duplicate {key}")

    def hierarchy(records, inventory, label):
        unique(records, "id", label)
        parents = {record["id"]: record.get("parent_id") for record in records}
        for record in records:
            current, visited = record["id"], set()
            while current is not None:
                if current in visited:
                    errors.append(f"{label}: cyclic containment at {current}")
                    break
                if current not in parents:
                    message = f"{label}: parent {current} not inline"
                    (unresolved if inventory else errors).append(message)
                    break
                visited.add(current)
                current = parents[current]

    cad = document["dialects"].get("cad")
    cae = document["dialects"].get("cae")
    artifact_documents = [(document, base_dir, "")]
    if cad:
        hierarchy(cad.get("entities", []), cad.get("inventory"), "cad/entities")
    selected_cad = None
    if cae:
        hierarchy(cae.get("features", []), cae.get("setup"), "cae/features")
        unique(cae.get("results", []), "id", "cae/results")
        parameters = [(p.get("scope"), p["name"]) for p in cae.get("parameters", [])]
        if len(parameters) != len(set(parameters)):
            errors.append("cae/parameters: duplicate name in the same scope")
        for result in cae.get("results", []):
            unique(result.get("columns", []), "index", f"cae/results/{result['id']}/columns")
        if "cad" in cae:
            ref = cae["cad"]
            parts = urlsplit(ref["uri"])
            if unquote(parts.fragment) != "/dialects/cad":
                errors.append("cae/cad: reference must select #/dialects/cad")
            elif not parts.path and not parts.scheme and not parts.netloc and not parts.query:
                selected_cad = cad
                if selected_cad is None:
                    errors.append("cae/cad: local CAD dialect missing")
                if "sha256" in ref:
                    errors.append("cae/cad: same-document reference cannot carry a file hash")
            else:
                path = local_path(ref["uri"], base_dir)
                if path is None:
                    unresolved.append("cae/cad: remote CAD document not fetched")
                elif not path.is_file():
                    (errors if check_artifacts else unresolved).append("cae/cad: external CAD document missing")
                else:
                    try:
                        external = read_manifest(path)
                        validator.validate(external)
                        selected_cad = external["dialects"].get("cad")
                        if selected_cad is None:
                            errors.append("cae/cad: external CAD dialect missing")
                        else:
                            hierarchy(selected_cad.get("entities", []), selected_cad.get("inventory"), "external cad/entities")
                            artifact_documents.append(({"dialects": {"cad": selected_cad}}, path.parent, "external CAD"))
                    except (OSError, ValueError) as exc:
                        errors.append(f"cae/cad: {exc}")
                    except ValidationError:
                        errors.append("cae/cad: external document does not match the IR schema")
        for feature in cae.get("features", []):
            selection = feature.get("selection", {})
            if "entity_ids" not in selection:
                continue
            if "cad" not in cae:
                errors.append(f"cae/features/{feature['id']}: CAD reference required")
            elif selected_cad is not None:
                known = {entity["id"] for entity in selected_cad.get("entities", [])}
                missing = set(selection["entity_ids"]) - known
                if missing:
                    message = f"cae/features/{feature['id']}: selection IDs not inline: {sorted(missing)}"
                    (unresolved if selected_cad.get("inventory") else errors).append(message)

    if check_artifacts:
        for owner, owner_dir, prefix in artifact_documents:
            for location, ref in artifact_references(owner):
                if location == "/dialects/cae/cad" and ref["uri"].startswith("#"):
                    continue
                location = prefix + location
                path = local_path(ref["uri"], owner_dir)
                if path is None:
                    unresolved.append(f"{location}: artifact not a local file reference")
                    continue
                if not path.is_file():
                    errors.append(f"{location}: artifact missing")
                elif "sha256" in ref:
                    digest = hashlib.sha256()
                    try:
                        with path.open("rb") as handle:
                            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                                digest.update(chunk)
                        if digest.hexdigest() != ref["sha256"]:
                            errors.append(f"{location}: artifact hash mismatch")
                    except OSError:
                        errors.append(f"{location}: artifact could not be read")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifests", nargs="+", type=Path)
    parser.add_argument("--check-artifacts", action="store_true", help="Check local referenced files and supplied hashes; no network fetching")
    parser.add_argument("--require-resolved", action="store_true", help="Fail if any semantic or artifact check remains unresolved")
    args = parser.parse_args()
    failed = False
    for path in args.manifests:
        try:
            report = validate_document(read_manifest(path), path.parent, args.check_artifacts)
        except (OSError, ValueError) as exc:
            report = {"errors": [str(exc)], "unresolved": []}
        print(json.dumps({"file": str(path), **report}, ensure_ascii=False))
        failed |= bool(report["errors"] or (args.require_resolved and report["unresolved"]))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
