"""Check real CLI projections against the standalone IR schema and file refs."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile

from validate_ir import validate_document


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, required=True)
    args = parser.parse_args()
    cli = str(args.cli.resolve())
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "part #1.step").write_text("ISO-10303-21;HEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=(LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));ENDSEC;END-ISO-10303-21;")
        (root / "job.inp").write_text("*MATERIAL, NAME=COPPER\n*STEP, NAME=Heat\n*END STEP\n")
        (root / "antenna.aedt").write_text("$begin 'AnsoftProject'\n$begin 'HFSSModel'\nName='Antenna'\nSolutionType='DrivenModal'\n$end 'HFSSModel'\n$end 'AnsoftProject'\n$begin 'ProjectPreview'\n$begin 'DesignInfo'\nDesignName='Antenna'\nFactory='HFSS'\nIsSolved=false\n$end 'DesignInfo'\n$end 'ProjectPreview'\n")
        with zipfile.ZipFile(root / "heat.mph", "w") as archive:
            archive.writestr("dmodel.xml", "<Model><Physics tag='ht'/><Study tag='std1'/></Model>")
        checks = 0
        for source in root.iterdir():
            for include_paths in (False, True):
                views = []
                for summary in (False, True):
                    flags = (["--include-paths"] if include_paths else []) + (["--summary"] if summary else [])
                    result = json.loads(subprocess.check_output([cli, "inspect", str(source), "--json", *flags], encoding="utf-8"))
                    ir = result["ir"]
                    validation = validate_document(ir, root, check_artifacts=include_paths)
                    assert not validation["errors"], validation
                    if include_paths:
                        assert not validation["unresolved"], validation
                    else:
                        assert str(root) not in json.dumps(ir)
                    assert len(json.dumps(ir, separators=(",", ":")).encode()) <= 16384
                    views.append(ir)
                    checks += 1
                assert views[0] == views[1]
        print(json.dumps({"runtime_projections_checked": checks}))


if __name__ == "__main__":
    main()
