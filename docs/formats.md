# Format coverage

## COMSOL `.mph`

`simparse` treats `.mph` as a ZIP container. It reads `fileversion`,
`modelinfo.xml`, `dmodel.xml`, optional `smodel.json`, `usedlicenses.txt`, and
records binary `.mphbin` blocks by category and size only.

## Abaqus `.inp` / `.inc`

`simparse` reads keyword-driven input decks, include graphs, counts of node and
element rows, materials, sections, steps, boundary and load keywords, and
output requests.

## Fluent `.cas.h5` / `.msh.h5`

`simparse-hdf5` reads the HDF5 object tree, attributes, dataset shapes and
datatypes, plus lightweight hints for zones, boundaries, and settings when
names are discoverable. Result `.dat.h5` extraction is out of scope for v0.1.

## HFSS / AEDT `.aedt` / `.aedtz`

`simparse` follows AEDT section boundaries in text projects and zip-packaged
`.aedtz` archives. It reports product/version hints, design types and solution
types, solved status, variables, setups, sweeps, ports, boundaries, materials,
mesh operations, and sidecar lock/result status. Large embedded payload lines
are skipped while the full project structure is scanned.

## Ansys Mechanical `.mechdb` / `.mechdat`

`simparse-hdf5` reads the shallow Mechanical HDF5 container inventory and
selected metadata streams. It reports the Mechanical release, object and stream
counts, analyses, bodies, materials, contacts, loads and conditions, results,
geometry source basenames, and sidecar status. It does not reconstruct the
Mechanical object model or load result fields.
