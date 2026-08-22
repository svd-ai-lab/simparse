from typing import Any, Literal

Format = Literal[
    "auto",
    "comsol-mph",
    "abaqus-inp",
    "fluent-hdf5",
    "hfss-aedt",
    "ansys-mechanical",
    "icepak-tzr",
    "flotherm-floxml",
]

def inspect(
    path: str,
    format: Format = "auto",
    include_paths: bool = False,
    summary: bool = False,
) -> dict[str, Any]: ...

def scan(
    paths: list[str],
    recursive: bool = True,
    include_paths: bool = False,
) -> list[dict[str, Any]]: ...
