use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use simparse_core::{
    FormatSummary, InspectOptions, ScanOptions, SimFormat, detect_format, inspect_path, scan_paths,
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
    let text = r#"$begin 'AnsoftProject'
ProjectName='PatchProbe'
Product='ElectronicsDesktop'
$begin 'Desktop'
Version(2026, 1)
$end 'Desktop'
$begin 'Definitions'
$begin 'Materials'
$begin 'copper'
$end 'copper'
$end 'Materials'
$end 'Definitions'
$begin 'HFSSModel'
Name='PatchDesign'
SolutionType='HFSS Modal Network'
$begin 'Properties'
VariableProp('w', 'UD', '', '1mm')
$end 'Properties'
$begin 'BoundarySetup'
$begin 'Boundaries'
$begin 'Rad1'
BoundType='Radiation'
$end 'Rad1'
$begin 'P1'
BoundType='Wave Port'
$end 'P1'
$end 'Boundaries'
$end 'BoundarySetup'
$begin 'MeshSetup'
$begin 'MeshOperations'
$begin 'Length1'
$end 'Length1'
$end 'MeshOperations'
$end 'MeshSetup'
$begin 'AnalysisSetup'
$begin 'SolveSetups'
$begin 'Setup1'
SetupType='HfssDriven'
$begin 'Sweeps'
$begin 'Sweep1'
$end 'Sweep1'
$end 'Sweeps'
$end 'Setup1'
$end 'SolveSetups'
$end 'AnalysisSetup'
$end 'HFSSModel'
$end 'AnsoftProject'
$begin 'ProjectPreview'
$begin 'DesignInfo'
DesignName='PatchDesign'
Factory='HFSS'
IsSolved=false
$end 'DesignInfo'
$end 'ProjectPreview'
$begin 'ComponentBody'
VariableProp('component_false_positive')
$end 'ComponentBody'
"#;
    let aedt = tmp.path().join("patch.aedt");
    std::fs::write(&aedt, text).unwrap();

    let result = inspect_path(&aedt, InspectOptions::default()).unwrap();
    let FormatSummary::HfssAedt(summary) = result.summary else {
        panic!("expected HFSS summary");
    };
    assert_eq!(summary.project_name.as_deref(), Some("PatchProbe"));
    assert_eq!(summary.product.as_deref(), Some("ElectronicsDesktop"));
    assert_eq!(summary.version_hint.as_deref(), Some("2026.1"));
    assert!(summary.designs.iter().any(|d| d.name == "PatchDesign"));
    assert!(summary.variables.contains(&"w".to_string()));
    assert!(
        !summary
            .variables
            .contains(&"component_false_positive".to_string())
    );
    assert_eq!(summary.setups, ["Setup1"]);
    assert_eq!(summary.sweeps, ["Sweep1"]);
    assert_eq!(summary.ports, ["P1"]);
    assert_eq!(summary.boundaries, ["Rad1"]);
    assert_eq!(summary.materials, ["copper"]);
    assert_eq!(summary.mesh_operations, ["Length1"]);
    assert_eq!(
        summary.designs[0].solution_type.as_deref(),
        Some("HFSS Modal Network")
    );
    assert_eq!(summary.designs[0].is_solved, Some(false));

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
fn hfss_structural_scan_skips_large_embedded_payloads() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("large.aedt");
    let text = format!(
        "$begin 'AnsoftProject'\n\
         ProjectName='LargePayload'\n\
         $begin 'Desktop'\n\
         Version(2025, 2)\n\
         $end 'Desktop'\n\
         EmbeddedData='{}'\n\
         $end 'AnsoftProject'\n\
         $begin 'ProjectPreview'\n\
         $begin 'DesignInfo'\n\
         DesignName='PackageAntenna'\n\
         Factory='HFSS'\n\
         IsSolved=true\n\
         $end 'DesignInfo'\n\
         $end 'ProjectPreview'\n",
        "A".repeat(3 * 1024 * 1024)
    );
    std::fs::write(&path, text).unwrap();

    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::HfssAedt(summary) = result.summary else {
        panic!("expected HFSS summary");
    };
    assert_eq!(summary.project_name.as_deref(), Some("LargePayload"));
    assert_eq!(summary.version_hint.as_deref(), Some("2025.2"));
    assert_eq!(summary.designs[0].name, "PackageAntenna");
    assert_eq!(summary.designs[0].is_solved, Some(true));
    assert!(!summary.truncated);
}

#[test]
fn inspects_icepak_design_metadata_inside_aedt() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("package-thermal.aedt");
    std::fs::write(
        &path,
        r#"$begin 'AnsoftProject'
ProjectName='PackageThermal'
Product='ElectronicsDesktop'
$begin 'Desktop'
Version(2025, 2)
$end 'Desktop'
$begin 'IcepakModel'
Name='ThermalDesign'
'Default Fluid Material'='air'
'Default Solid Material'='Al-Extruded'
'Default Surface Material'='Steel-oxidised-surface'
AmbientTemperature='25cel'
AmbientPressure='0n_per_meter_sq'
AmbientRadiationTemperature='25cel'
$begin 'SolutionTypeOption'
SolutionTypeOption='SteadyState'
ProblemOption='TemperatureAndFlow'
$end 'SolutionTypeOption'
$begin 'BoundarySetup'
$begin 'Boundaries'
$begin 'ChipPower'
BoundType='Source'
'Thermal Condition'='Total Power'
'Total Power'='12W'
Temperature='AmbientTemp'
$end 'ChipPower'
$end 'Boundaries'
$end 'BoundarySetup'
$begin 'Monitor'
$begin 'IcepakMonitors'
$begin 'DieTop'
$end 'DieTop'
$end 'IcepakMonitors'
$end 'Monitor'
$begin 'MeshRegion'
$begin 'MeshSetup'
$begin 'MeshRegions'
$begin 'Global'
$end 'Global'
$begin 'PackageRegion'
$end 'PackageRegion'
$end 'MeshRegions'
$begin 'MeshOperations'
$begin 'DieRefinement'
$end 'DieRefinement'
$end 'MeshOperations'
$end 'MeshSetup'
$end 'MeshRegion'
$begin 'AnalysisSetup'
$begin 'SolveSetups'
$begin 'Setup1'
SetupType='IcepakSteadyState'
$end 'Setup1'
$end 'SolveSetups'
$end 'AnalysisSetup'
$end 'IcepakModel'
$end 'AnsoftProject'
$begin 'ProjectPreview'
$begin 'DesignInfo'
DesignName='ThermalDesign'
Factory='Icepak'
IsSolved=true
$end 'DesignInfo'
$end 'ProjectPreview'
"#,
    )
    .unwrap();

    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    assert_eq!(result.format, SimFormat::HfssAedt);
    let FormatSummary::HfssAedt(summary) = result.summary else {
        panic!("expected AEDT summary");
    };
    let design = summary
        .designs
        .iter()
        .find(|design| design.name == "ThermalDesign")
        .unwrap();
    assert_eq!(design.design_type.as_deref(), Some("Icepak"));
    assert_eq!(design.solution_type.as_deref(), Some("SteadyState"));
    assert_eq!(design.is_solved, Some(true));
    assert_eq!(summary.setups, ["Setup1"]);
    assert_eq!(summary.mesh_operations, ["DieRefinement"]);

    let icepak = summary.icepak.expect("expected Icepak detail");
    assert_eq!(
        icepak.designs[0].ambient_temperature.as_deref(),
        Some("25cel")
    );
    assert_eq!(
        icepak.designs[0].default_fluid_material.as_deref(),
        Some("air")
    );
    assert_eq!(
        icepak.designs[0].problem_option.as_deref(),
        Some("TemperatureAndFlow")
    );
    assert_eq!(icepak.thermal_boundaries[0].name, "ChipPower");
    assert_eq!(
        icepak.thermal_boundaries[0].thermal_condition.as_deref(),
        Some("Total Power")
    );
    assert_eq!(
        icepak.thermal_boundaries[0].total_power.as_deref(),
        Some("12W")
    );
    assert_eq!(icepak.monitors, ["DieTop"]);
    assert_eq!(icepak.mesh_regions, ["Global", "PackageRegion"]);
}

#[test]
fn inspects_icepak_classic_tzr_inventory_without_extracting() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("package.tzr");
    let file = std::fs::File::create(&path).unwrap();
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    append_tar_file(
        &mut archive,
        "PackageCooling/job",
        b"Icepak Classic job metadata",
    );
    append_tar_file(
        &mut archive,
        "PackageCooling/model",
        b"opaque model payload",
    );
    append_tar_file(&mut archive, "PackageCooling/geometry/board.step", b"STEP");
    archive.into_inner().unwrap().finish().unwrap();

    assert_eq!(detect_format(&path), Some(SimFormat::IcepakTzr));
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::IcepakTzr(summary) = result.summary else {
        panic!("expected Icepak TZR summary");
    };
    assert_eq!(summary.project_name.as_deref(), Some("PackageCooling"));
    assert!(summary.gzip_compressed);
    assert_eq!(summary.entry_count, 3);
    assert_eq!(summary.file_count, 3);
    assert!(summary.job_file_present);
    assert!(summary.model_file_present);
    assert_eq!(summary.entries[0].name, "PackageCooling/job");
}

#[test]
fn inspects_flotherm_floxml_project_and_smartpart_roots() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("hbm.xml");
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xml_case>
  <name>HBM_package</name>
  <model>
    <modeling><solution>flow_heat</solution><radiation>off</radiation><dimensionality>3d</dimensionality><transient>false</transient></modeling>
    <turbulence><turbulence_type>auto_algebraic</turbulence_type></turbulence>
    <gravity><normal_direction>neg_y</normal_direction></gravity>
    <global><datum_pressure>101325</datum_pressure><ambient_temperature>298.15</ambient_temperature></global>
  </model>
  <solve><overall_control><outer_iterations>500</outer_iterations></overall_control></solve>
  <grid><system_grid>
    <x_grid><min_size>0.0001</min_size><grid_type>max_size</grid_type><max_size>0.001</max_size></x_grid>
    <y_grid><min_size>0.000005</min_size><grid_type>max_size</grid_type><max_size>0.00002</max_size></y_grid>
    <z_grid><min_size>0.0001</min_size><grid_type>max_size</grid_type><max_size>0.001</max_size></z_grid>
  </system_grid></grid>
  <attributes>
    <materials><isotropic_material_att><name>Silicon</name><conductivity>148</conductivity></isotropic_material_att></materials>
    <fluids><fluid_att><name>Air</name></fluid_att></fluids>
    <sources><source_att><name>DiePower</name><source_options><option><power>8</power></option></source_options></source_att></sources>
    <thermals><thermal_att><name>ColdPlate</name><thermal_model>fixed_temperature</thermal_model></thermal_att></thermals>
  </attributes>
  <geometry>
    <cuboid><name>Die</name><material>Silicon</material></cuboid>
    <source><name>DieHeater</name><source>DiePower</source></source>
    <monitor_point><name>DieTop</name></monitor_point>
  </geometry>
  <solution_domain>
    <x_low_ambient>Ambient</x_low_ambient><x_high_ambient>Ambient</x_high_ambient>
    <y_low_boundary>symmetry</y_low_boundary><y_high_ambient>Ambient</y_high_ambient>
    <z_low_ambient>Ambient</z_low_ambient><z_high_ambient>Ambient</z_high_ambient><fluid>Air</fluid>
  </solution_domain>
</xml_case>
"#,
    )
    .unwrap();

    assert_eq!(detect_format(&path), Some(SimFormat::FlothermFloxml));
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::FlothermFloxml(summary) = result.summary else {
        panic!("expected FloTHERM FloXML summary");
    };
    assert_eq!(summary.root, "xml_case");
    assert_eq!(summary.name.as_deref(), Some("HBM_package"));
    assert_eq!(summary.solution.as_deref(), Some("flow_heat"));
    assert_eq!(summary.transient, Some(false));
    assert_eq!(summary.grid.len(), 3);
    assert!(
        summary
            .attribute_type_counts
            .iter()
            .any(|item| item.name == "isotropic_material_att" && item.count == 1)
    );
    assert!(
        summary
            .geometry
            .iter()
            .any(|item| item.kind == "monitor_point" && item.name == "DieTop")
    );
    assert_eq!(summary.sources[0].name, "DiePower");
    assert_eq!(summary.sources[0].powers, ["8"]);
    let domain = summary.solution_domain.unwrap();
    assert_eq!(domain.fluid.as_deref(), Some("Air"));
    assert_eq!(domain.boundaries.len(), 6);

    let smartpart = tmp.path().join("package.floxml");
    std::fs::write(&smartpart, "<sm_xml_case><name>QFN</name></sm_xml_case>").unwrap();
    let result = inspect_path(&smartpart, InspectOptions::default()).unwrap();
    let FormatSummary::FlothermFloxml(summary) = result.summary else {
        panic!("expected SmartPart FloXML summary");
    };
    assert_eq!(summary.root, "sm_xml_case");
    assert_eq!(summary.name.as_deref(), Some("QFN"));
}

#[test]
fn inspects_flotherm_pack_inventory_without_extracting() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("mobile-demo.pack");
    let file = std::fs::File::create(&path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive
        .add_directory(
            "Mobile_Demo.AE152DF44810B5C3E9EF/DataSets/BaseSolution/",
            options,
        )
        .unwrap();
    archive
        .start_file("Mobile_Demo.AE152DF44810B5C3E9EF/PDProject/group", options)
        .unwrap();
    archive.write_all(b"opaque PDML project data").unwrap();
    archive
        .start_file(
            "Mobile_Demo.AE152DF44810B5C3E9EF/DataSets/BaseSolution/solution.cat",
            options,
        )
        .unwrap();
    archive.write_all(b"solution catalogue").unwrap();
    archive.finish().unwrap();

    assert_eq!(detect_format(&path), Some(SimFormat::FlothermPack));
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::FlothermPack(summary) = result.summary else {
        panic!("expected FloTHERM pack summary");
    };
    assert_eq!(summary.project_name.as_deref(), Some("Mobile_Demo"));
    assert_eq!(
        summary.project_directory.as_deref(),
        Some("Mobile_Demo.AE152DF44810B5C3E9EF")
    );
    assert_eq!(summary.entry_count, 3);
    assert_eq!(summary.file_count, 2);
    assert_eq!(summary.directory_count, 1);
    assert!(summary.project_data_present);
    assert!(summary.base_solution_present);
    assert_eq!(summary.entries.len(), 3);
}

#[test]
fn scan_detects_floxml_by_content_and_skips_unrelated_xml() {
    let tmp = tempdir().unwrap();
    std::fs::write(
        tmp.path().join("model.xml"),
        "<xml_case><name>ThermalModel</name></xml_case>",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("notes.xml"),
        "<notes><name>Not a model</name></notes>",
    )
    .unwrap();

    let results = scan_paths(&[tmp.path().to_path_buf()], ScanOptions::default()).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "model.xml");
    assert_eq!(results[0].format, SimFormat::FlothermFloxml);
}

#[test]
fn detects_ansys_mechanical_extensions() {
    assert_eq!(
        detect_format(std::path::Path::new("package.mechdb")),
        Some(SimFormat::AnsysMechanical)
    );
    assert_eq!(
        detect_format(std::path::Path::new("archive.mechdat")),
        Some(SimFormat::AnsysMechanical)
    );
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

fn append_tar_file(archive: &mut tar::Builder<GzEncoder<std::fs::File>>, name: &str, data: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o644);
    header.set_size(data.len() as u64);
    header.set_cksum();
    archive.append_data(&mut header, name, data).unwrap();
}
