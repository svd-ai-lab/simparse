# Tool design principles

`simparse` is designed as a lightweight agent tool, not as a replacement for
simulation software, vendor APIs, or mature parser libraries.

## Compose with existing tools

When an existing tool already reads a format well, use it. `simparse` should
make those tools easier to compose with, not duplicate their full semantics.

The project focuses on small, stable outputs that are useful before a heavier
tool is needed: metadata, inventory, file shape, setup hints, and references to
large payloads.

## Optimize the agent gap

The target use case is an agent calling a tool to quickly inspect a file or scan
a directory of historical simulation projects. The useful optimization space is
where existing paths are too heavy, slow to start, hard to install, license
dependent, or too eager to load full project/result data.

Good fits include:

- scanning many project or case files for metadata
- reading container inventories without loading large binary payloads
- finding model names, setup names, variables, materials, zones, or boundaries
- producing privacy-conscious JSON for downstream agent workflows
- deciding which heavier vendor or Python tool should be called next

Poor fits include:

- replacing vendor-native project semantics
- fully interpreting solver behavior
- loading large result datasets by default
- optimizing a path that is already simple, reliable, and fast enough

## Stay lightweight

Prefer ordinary text, ZIP, XML, JSON, and shallow HDF5 reads when they answer the
question. Add deeper scaffolding only when it improves reuse, correctness, or
composability.

Performance claims should be tied to a concrete scenario. In particular,
agent tool-call latency, in-process parser cost, and vendor-native parser cost
are different measurements and should not be collapsed into one generic
"faster" claim.
