# simparse

`simparse` is a lightweight Rust and Python scanner for simulation project and
case metadata.

## Supported formats

| Format | Files |
|---|---|
| COMSOL | `.mph` |
| Abaqus | `.inp`, `.inc` |
| Fluent | `.cas.h5`, `.msh.h5` |
| Ansys Electronics Desktop (HFSS / Icepak) | `.aedt`, `.aedtz` |
| Ansys Mechanical | `.mechdb`, `.mechdat` |
| Icepak Classic | `.tzr` |
| Simcenter FloTHERM | `.pack`, `.xml`, `.floxml` |

## Design

`simparse` is meant to compose with existing simulation tools, not replace them.
It focuses on lightweight agent workflows: metadata, inventory, quick directory
scans, and deciding when a heavier vendor or Python tool should be called next.
See [tool design principles](docs/design-principles.md).

## CLI

```powershell
cargo run -p simparse-cli -- inspect /path/to/model.mph --json
cargo run -p simparse-cli -- inspect /path/to/thermal-model.xml --json --summary
cargo run -p simparse-cli -- inspect /path/to/model.mph --json --summary
cargo run -p simparse-cli -- scan /path/to/history --jsonl
```

Use `--summary` for a deterministic, bounded preflight view. It reports total
counts with capped samples and explicit parser limitations. Omit it when the
full shallow inventory is needed.

FloXML files commonly use the generic `.xml` suffix. `simparse` checks their
root element, so directory scans include `<xml_case>` and `<sm_xml_case>` files
while skipping unrelated XML.

## Python

```powershell
maturin develop
python -c "import simparse; print(simparse.inspect('/path/to/model.inp', summary=True))"
python -c "import simparse; print(simparse.scan(['/path/to/history'], summary=True))"
```

## Benchmark

![Public benchmark bar chart](artifacts/public-benchmark.svg)

```powershell
python benchmarks/compare.py
```

This benchmark models one practical agent path: call one external tool to scan a
group of public simulation artifacts. The bars include process startup and
library import cost. The JSON artifact also records an in-process Python
baseline for context; those numbers are useful for library-level comparison, but
are not the primary agent tool-call scenario.

The benchmark downloads public URLs listed in `benchmarks/public-artifacts.json`
into `target/` and writes `artifacts/public-benchmark.json` plus
`artifacts/public-benchmark.svg`. Vendor-native parser timings are not included
because they require locally licensed software or APIs.

## License

Apache-2.0
