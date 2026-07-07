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

`simparse` scans text-like AEDT project payloads and zip-packaged `.aedtz`
archives for project/design hints, variables, setup and sweep names, ports,
boundaries, and sidecar lock/result status.
