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

## Ansys Electronics Desktop `.aedt` / `.aedtz`

`simparse` follows AEDT section boundaries in text projects and zip-packaged
`.aedtz` archives. It reports product/version hints, design types and solution
types, solved status, variables, setups, sweeps, ports, boundaries, materials,
mesh operations, and sidecar lock/result status. For Icepak designs it also
reports thermal-fluid solution options, ambient conditions, default materials,
thermal boundaries and powers, monitors, and mesh regions. Large embedded
payload lines are skipped while the full project structure is scanned. The
serialized format name remains `hfss-aedt` for compatibility, even when an AEDT
project contains Icepak or mixed design types.

## Ansys Mechanical `.mechdb` / `.mechdat`

`simparse-hdf5` reads the shallow Mechanical HDF5 container inventory and
selected metadata streams. It reports the Mechanical release, object and stream
counts, analyses, bodies, materials, contacts, loads and conditions, results,
geometry source basenames, and sidecar status. It does not reconstruct the
Mechanical object model or load result fields.

## Icepak Classic `.tzr`

`simparse` streams the compressed tar archive without extracting its contents.
It reports the common project name, entry and file counts, uncompressed byte
total, the presence of the conventional `job` and `model` payloads, and a
bounded entry inventory. It does not decode the proprietary Classic model,
mesh, or result payloads.

## Simcenter FloTHERM FloXML `.xml` / `.floxml`

`simparse` recognizes declarative project `<xml_case>` and SmartPart
`<sm_xml_case>` roots. It streams the XML and reports model/solve options, grid
hints, named attribute and geometry inventories, heat-source powers, and
solution-domain boundaries. Generic `.xml` files are detected by content and
unrelated XML is skipped during directory scans. FloSCRIPT action logs and
decoding proprietary PDML project payloads are outside this parser's scope.

## Simcenter FloTHERM project archive `.pack`

`simparse` reads the ZIP directory without extracting or decoding project
payloads. It reports the project directory/name, compressed and uncompressed
sizes, entry counts, whether `PDProject/group` is present, whether a
`DataSets/BaseSolution` tree is present, and a bounded entry inventory.
