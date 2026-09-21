# Draft AI Infra System IR

AI Infra System IR is a compact JSON inspection contract for AI infrastructure
hardware: cooling, advanced packaging, 3D IC, and their thermal, fluid and
mechanical models. Power and interconnect artifacts can be referenced; detailed
EDA semantics are outside this draft.

| Layer | Purpose | Schema |
|---|---|---|
| Core | Container, artifact references, ID/unit conventions, source expressions, software extensions | [ai-infra-system.schema.json](../schemas/ai-infra-system.schema.json) |
| `cad` dialect | Geometry units, entity totals and selected entities | [cad-ir.schema.json](../schemas/cad-ir.schema.json) |
| `cae` dialect | Analyses, parameters, materials, physics, conditions, interfaces, studies, mesh and results | [cae-ir.schema.json](../schemas/cae-ir.schema.json) |

A document uses `"schema": "ai-infra-system/v0"` and contains `dialects.cad`,
`dialects.cae`, or both. Each dialect declares its own `version` and `source`.
Software extensions have separate namespaces and contract versions. The initial
software focus is COMSOL, HFSS within Ansys AEDT, AEDT projects, and Ansys
Mechanical. Native software versions and details remain in their extensions.

The organization borrows coexisting, namespaced dialects from
[MLIR](https://mlir.llvm.org/docs/LangRef/#dialects). This is a JSON data contract
with no MLIR runtime dependency. Extraction probes use Rust and the existing
simparse readers. Python is used only for development-time schema validation.
The CLI and Python `inspect`/`scan` APIs do not yet emit this draft. The existing
`sim-cli` asset-scanning integration continues to consume those APIs.

## Small manifests, external payloads

Keep useful identifiers and summaries inline. Geometry, topology payloads, mesh
nodes/connectivity, time histories and field arrays stay in external files.
`inventory`, `setup`, `result_inventory` and `selection.artifact` can reference
larger inventories. Their format must be understood by the consumer; this draft
does not introduce a universal sidecar format.

An artifact reference contains `uri` and optionally `format` and `sha256`.
Relative URIs resolve from the containing IR file; encode spaces as `%20`.
A hash covers the referenced file's exact bytes. A native source can also serve
as geometry or setup without copying its payload. Data URIs and inline payload
fields are not allowed. Schemas bound inline lists and extension attributes;
the reference validator also enforces a manifest byte limit.

Omit unknown fields and empty optional lists. Missing information never means
zero, false, dimensionless, absent from the source, or an empty physical
selection. Inline lists are selected observations, not complete inventories.
CAD `counts` are known source totals, independent of the inline sample size.

## Identity and semantics

- IDs are unique within each CAD entity, CAE feature, or CAE result inventory.
  `parent_id` refers to the same inventory and expresses containment. It does
  not define topology adjacency, assembly transforms or physics coupling.
- `cae.cad` selects a CAD dialect using `#/dialects/cad` in the same document or
  `geometry.json#/dialects/cad` in another document. A same-document reference
  carries no hash; an external-reference hash covers the entire file.
- Selection `entity_ids` require `cae.cad` and contain the entire known selection.
  They resolve in that CAD dialect or its referenced inventory. Use an external
  artifact for a larger selection; never silently truncate it. A `native_ref`
  can retain a vendor selector. Multiple representations describe the same
  selection, not a union.
- IDs and native references have no stability guarantee after topology edits.
  Verify source revisions before reusing bindings. Matching names alone cannot
  establish correspondence between geometry, setup, mesh and results.
- Preserve expressions and unit text. Parameter `scope` retains a known component
  or namespace; omission does not establish global scope. Serialized vendor node
  properties must not be promoted to physical parameters without establishing
  their meaning. Large or unscoped native inventories stay external.
- `enabled` records an observed feature flag; omission means unknown. It does
  not establish effective activation in a study. `native_type` and `native_ref`
  retain source vocabulary without promising solver-independent semantics.
- Column `index` is zero-based; `header` preserves original text. Emit quantity
  and unit only when known. Result values stay external, missing values never
  become zero, and result presence does not prove solve success or freshness.

## Software extensions

Extensions are allowed on the document, dialects, CAD entities, CAE features and
results. Each contains a contract `version`, optional scalar `attributes`, and an
optional `artifact` for larger or nested native details. For example:

```json
{"extensions":{"comsol":{"version":"0","attributes":{"tag":"ht"}}}}
```

Namespaces such as `comsol`, `ansys.aedt` and `ansys.mechanical` retain software
semantics alongside the shared dialects. The core validates the envelope, not
each vendor's attributes. Consumers must preserve unknown extensions when
serializing. Extensions do not override core or dialect fields. A consumer
cannot claim a semantics-preserving conversion while silently discarding an
extension it does not understand. These drafts do not define executable models.

## Examples and validation

[cooling-component.json](../examples/ir/cooling-component.json) demonstrates
linked geometry and thermal setup. [package-stack.json](../examples/ir/package-stack.json)
demonstrates a compute die, memory stack, interposer and thermal/mechanical
inventories with a native interface definition. Both are hand-authored examples;
referenced `artifacts/` files are illustrative and not included. The values and
software tags do not constitute validated simulation models.

The schemas use JSON Schema Draft 2020-12. Register their `$id` URNs in a local
schema registry so validation requires no network retrieval. Development checks:

```sh
python -m pip install -r tests/ir/requirements.txt
python scripts/validate_ir.py examples/ir/cooling-component.json examples/ir/package-stack.json --require-resolved
python -m unittest discover -s tests/ir -p 'test_*.py' -v
```

The validator checks structure, byte/list bounds, duplicate IDs and JSON keys,
containment cycles, parameter scopes, column indices and CAD selection references.
`--check-artifacts` additionally checks local artifacts and supplied hashes,
including the sources of a referenced external CAD dialect. `--require-resolved`
fails if checks remain unresolved. Remote documents and inventories in arbitrary
sidecar formats are not fetched or interpreted. Physical consistency, vendor
extension semantics and effective solver state need their owning readers/tools.

The test-only [Rust asset probe](../crates/simparse-core/examples/ir_asset_probe.rs)
accepts a JSON array of local asset paths. It makes shallow trial projections,
retains complete reader observations as external artifacts, and hashes sources
and sidecars. Run the probe, then validate its generated manifests:

```sh
cargo run -p simparse-core --example ir_asset_probe -- assets.json /path/to/probe-output
python scripts/validate_ir.py /path/to/probe-output/case-000/system.json --check-artifacts --require-resolved
```

These probes test contract fit and reference integrity. They are not production
exporters, solver validation, or new supported formats in the simparse CLI.
