# Public inspection benchmark

The introduction graphic measures the cost of turning an engineering file into
shallow metadata and a compact AI Infra System IR. It reports **single-file CLI
wall time** and **IR bytes**, with input sizes and file counts for context.
It is a small, deliberately varied corpus, not a representative distribution of
customer files or a measure of extraction accuracy.

## Reproduce

Use Python 3.10+, the workspace's supported Rust toolchain, and Git. On Windows,
PowerShell 7 is used to record the CPU model. No vendor application is launched.

```powershell
python -m pip install -r benchmarks/requirements.txt
python benchmarks/compare.py
python -m unittest discover -s benchmarks -p 'test_*.py' -v
```

The runner builds the release CLI with the locked dependencies, verifies or
downloads the public files into `target/benchmark-artifacts/`, and writes
`artifacts/public-benchmark.json`, `.svg` and `.png`. Run with `--refresh` to
replace the download cache. A cached checksum mismatch fails rather than silently
changing the corpus. Native code must match HEAD; benchmark scripts can be edited.

To change only the graphic, reuse the recorded data without rerunning timings:

```powershell
python benchmarks/plot.py
```

For a smaller local run, use `--case mechanical --out target/mechanical-benchmark.json`.
The selected cases remain in manifest order. Negative controls always run.

## Corpus

[public-artifacts.json](public-artifacts.json) records repository revisions,
source URLs, licenses, byte counts and SHA-256 checksums. COMSOL files come from
`sepidehkhakzad/ComsolProjects`; the Ansys software groups and STEP files come from
`ansys/example-data`; Abaqus files come from the four repositories named in the
manifest. The cases include capacitance, heat transfer, RF, electronics cooling,
magnetics, structural models and CAD declarations.

HFSS, Icepak and Maxwell share the AEDT reader, but appear separately because
their applications differ. The runner checks the observed design types before
timing. These are software/format groups, not eight independent parser engines.
Icepak Classic and FloTHERM are supported but are not represented in this corpus.

One additional pinned source, `pyaedt/icepak/DME.stp`, starts with
`sISO-10303-21;` instead of `ISO-10303-21;`. It is kept unmodified as a malformed
header control. Its expected rejection is recorded separately and excluded from
the successful-inspection bars and file count. Unexpected failures stop the run;
no failed file is silently dropped.

## Measurement contract

- **Invocation:** `simparse inspect <file> --json --summary`. Every sample starts
  a fresh CLI process. Wall time includes startup, extraction and stdout capture;
  download, build, JSON decoding, schema validation and plotting are outside it.
- **Repetition:** one excluded warmup per file, then 11 timed rounds by default.
  Every round visits each file once in a deterministically shuffled order.
  Processes run serially, with warm filesystem cache and no cache flushing.
  The JSON records the seed, all samples, host CPU/OS, toolchain and source revision.
- **Time bars:** first take each file's median over repeats, then the median of
  those values for its group. Whiskers show the smallest and largest per-file
  medians, not confidence intervals or the spread of repeated timings. Each file
  has equal weight. A one-file group has no file-to-file range.
- **Size bars:** median and min–max across files of the `ir` field serialized as
  compact UTF-8 JSON, with literal Unicode and no whitespace. KiB means 1,024
  bytes. The full summary JSON and actual CLI stdout sizes are also recorded;
  they are larger than the IR alone.
- **Checks:** the warmup verifies success, expected format, IR schema and the
  16 KiB IR budget. All measured outputs must match that file's warmup output.
  Native warnings and source-truncation flags are recorded. These checks do not
  establish that all native model semantics were extracted correctly.

IR size reflects selected observations and references, not lossless compression
of the source. Default URNs hide paths and are descriptive handles, not resolvable
file locations; `--include-paths` produces file URIs but is outside this workload.
Retention limits and extraction depth differ by format. Absolute times depend on
the host, filesystem cache and process-launch overhead; this does not benchmark
vendor loading, geometry validation, meshing or solving. The recorded source
revision identifies the measured code independently of package release versions.

The graph makes no speedup claim against Python or vendor tools: comparable
timings would require equivalent outputs and work.
