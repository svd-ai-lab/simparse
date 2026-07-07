use std::io::Write;

use simparse_core::{
    FormatSummary, InspectOptions, ScanOptions, SimFormat, inspect_path, scan_paths,
};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

#[test]
fn inspects_synthetic_comsol_mph() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("compact.mph");
    write_mph(&path);

    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    assert_eq!(result.path, None);
    assert_eq!(result.format, SimFormat::ComsolMph);
    let FormatSummary::ComsolMph(summary) = result.summary else {
        panic!("expected COMSOL summary");
    };
    assert_eq!(summary.saved_in.as_deref(), Some("COMSOL 6.4.0.272"));
    assert_eq!(summary.title.as_deref(), Some("Synthetic heat model"));
    assert!(summary.parameters.iter().any(|p| p.name == "T_in"));
    assert!(summary.physics_tags.contains(&"ht".to_string()));
    assert!(
        summary
            .size_breakdown
            .iter()
            .any(|b| b.bucket == "geometry")
    );
}

#[test]
fn inspects_abaqus_deck_with_include() {
    let tmp = tempdir().unwrap();
    std::fs::write(
        tmp.path().join("mat.inc"),
        "*MATERIAL, NAME=STEEL\n*ELASTIC\n200000., 0.3\n",
    )
    .unwrap();
    let deck = tmp.path().join("beam.inp");
    std::fs::write(
        &deck,
        "*HEADING\nTiny beam\n*INCLUDE, INPUT=mat.inc\n*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=B31, ELSET=EALL\n1,1,2\n*BEAM SECTION, ELSET=EALL, MATERIAL=STEEL\n*STEP\n*STATIC\n*BOUNDARY\n1,1,6,0\n*CLOAD\n2,2,-1\n*NODE PRINT\nU\n*END STEP\n",
    )
    .unwrap();

    let result = inspect_path(&deck, InspectOptions::default()).unwrap();
    let FormatSummary::AbaqusInp(summary) = result.summary else {
        panic!("expected Abaqus summary");
    };
    assert_eq!(summary.title.as_deref(), Some("Tiny beam"));
    assert_eq!(summary.node_count, 2);
    assert_eq!(summary.element_count, 1);
    assert!(summary.materials.contains(&"STEEL".to_string()));
    assert!(summary.boundary_keywords.contains(&"BOUNDARY".to_string()));
    assert!(summary.load_keywords.contains(&"CLOAD".to_string()));
    assert!(summary.output_keywords.contains(&"NODE PRINT".to_string()));
}

#[test]
fn inspects_hfss_aedt_and_aedtz() {
    let tmp = tempdir().unwrap();
    let text = "ProjectName='PatchProbe'\nVersion='2026.1'\n$begin 'HFSSModel'\nDesignName='PatchDesign'\nVariableProp('w')\nSetup('Setup1')\nSweep('Sweep1')\nPort('P1')\nBoundary('Rad1')\n$end 'HFSSModel'\n";
    let aedt = tmp.path().join("patch.aedt");
    std::fs::write(&aedt, text).unwrap();

    let result = inspect_path(&aedt, InspectOptions::default()).unwrap();
    let FormatSummary::HfssAedt(summary) = result.summary else {
        panic!("expected HFSS summary");
    };
    assert_eq!(summary.project_name.as_deref(), Some("PatchProbe"));
    assert!(summary.designs.iter().any(|d| d.name == "PatchDesign"));
    assert!(summary.variables.contains(&"w".to_string()));

    let aedtz = tmp.path().join("packed.aedtz");
    let file = std::fs::File::create(&aedtz).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("project/patch.aedt", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(text.as_bytes()).unwrap();
    zip.finish().unwrap();

    let result = inspect_path(&aedtz, InspectOptions::default()).unwrap();
    let FormatSummary::HfssAedt(summary) = result.summary else {
        panic!("expected HFSS summary");
    };
    assert_eq!(summary.source_member.as_deref(), Some("project/patch.aedt"));
}

#[test]
fn scan_redacts_paths_by_default() {
    let tmp = tempdir().unwrap();
    let deck = tmp.path().join("beam.inp");
    std::fs::write(&deck, "*HEADING\nRedacted\n*NODE\n1,0,0,0\n").unwrap();

    let results = scan_paths(&[tmp.path().to_path_buf()], ScanOptions::default()).unwrap();
    assert_eq!(results.len(), 1);
    let json = serde_json::to_string(&results).unwrap();
    assert!(!json.contains("C:\\Users\\"));
    assert!(!json.contains(tmp.path().to_string_lossy().as_ref()));
    assert_eq!(results[0].path, None);
}

fn write_mph(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    zip.start_file("fileversion", options).unwrap();
    zip.write_all(b"2092:COMSOL 6.4.0.272\n").unwrap();
    zip.start_file("modelinfo.xml", options).unwrap();
    zip.write_all(
        br#"<modelinfo title="Synthetic heat model" description="Generated fixture" comsolVersion="6.4" nodeType="compact" isRunnable="true"/>"#,
    )
    .unwrap();
    zip.start_file("dmodel.xml", options).unwrap();
    zip.write_all(
        br#"<Model><ModelParam tag="param"><ModelParamGroupList><ModelParamGroup tag="default"><param T="33" param="T_in" value="293[K]" reference="inlet temperature"/></ModelParamGroup></ModelParamGroupList></ModelParam><Physics tag="ht"/><Study tag="std1"/><Material tag="mat1"/></Model>"#,
    )
    .unwrap();
    zip.start_file("smodel.json", options).unwrap();
    zip.write_all(br#"{"nodes":[{"apiClass":"Physics","tag":"spf"}]}"#)
        .unwrap();
    zip.start_file("usedlicenses.txt", options).unwrap();
    zip.write_all(b"COMSOL\nHeat Transfer Module\n").unwrap();
    zip.start_file("geommanager1.mphbin", options).unwrap();
    zip.write_all(&[0, 1, 2, 3]).unwrap();
    zip.finish().unwrap();
}
