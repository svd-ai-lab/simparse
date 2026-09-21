use simparse_core::{
    FormatSummary, InspectOptions, ScanOptions, inspect_path, scan_paths, summarize_result,
};
use std::io::Write;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

fn step(body: &str) -> String {
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;{body}ENDSEC;END-ISO-10303-21;"
    )
}

#[test]
fn declaration_reader_handles_comments_strings_and_complex_units() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("quoted units.stp");
    std::fs::write(&path, step("/* #42=PRODUCT('fake'); */ #1=PRODUCT('p','It''s ; a plate','',());\n#2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT( .MICRO. , .METRE. ));#3=MANIFOLD_SOLID_BREP('',#4);")).unwrap();
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::Step(data) = &result.summary else {
        panic!()
    };
    assert!(!data.truncated);
    assert_eq!(data.schemas, ["AUTOMOTIVE_DESIGN"]);
    assert_eq!(data.products[0].name, "It's ; a plate");
    assert_eq!(data.length_units[0].unit.as_deref(), Some("um"));
    assert_eq!(
        data.entity_type_counts
            .iter()
            .find(|c| c.name == "PRODUCT")
            .unwrap()
            .count,
        1
    );
    let ir = result.ir.as_ref().unwrap();
    assert!(ir["dialects"]["cad"].get("length_unit").is_none());
    assert!(
        ir["dialects"]["cad"]["source"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("urn:")
    );
    assert_eq!(summarize_result(&result).ir, result.ir);
    assert_eq!(
        scan_paths(&[dir.path().into()], ScanOptions::default())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn partial_and_oversized_records_are_explicit_and_recoverable() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bounded.step");
    std::fs::write(
        &path,
        step(&format!(
            "#1=PRODUCT('p','{}','',());#2=MANIFOLD_SOLID_BREP('',#3);",
            "z".repeat(1000)
        )),
    )
    .unwrap();
    let result = inspect_path(
        &path,
        InspectOptions {
            max_text_bytes: 100,
            ..Default::default()
        },
    )
    .unwrap();
    let FormatSummary::Step(data) = result.summary else {
        panic!()
    };
    assert!(data.truncated);
    assert_eq!(data.skipped_records, 1);
    assert_eq!(data.entity_type_counts[0].name, "MANIFOLD_SOLID_BREP");
    for input in [
        "ISO-10303-21; DATA; #1=PRODUCT('open",
        "ISO-10303-21; HEADER; ENDSEC; END-ISO-10303-21;",
    ] {
        std::fs::write(&path, input).unwrap();
        let FormatSummary::Step(data) = inspect_path(&path, InspectOptions::default())
            .unwrap()
            .summary
        else {
            panic!()
        };
        assert!(data.truncated);
    }
    std::fs::write(&path, "not STEP").unwrap();
    assert!(inspect_path(&path, InspectOptions::default()).is_err());
}

#[test]
fn samples_and_unit_expressions_stay_bounded_without_inventing_global_scale() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("many.step");
    let mut body = (0..1000)
        .map(|i| format!("#{i}=PRODUCT('p','part{i}','',());"))
        .collect::<String>();
    body.push_str("#1001=(LENGTH_UNIT() SI_UNIT($,.METRE.));#1002=(LENGTH_UNIT() CONVERSION_BASED_UNIT('inch',#1003));");
    std::fs::write(&path, step(&body)).unwrap();
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::Step(data) = &result.summary else {
        panic!()
    };
    assert_eq!(data.products.len(), 8);
    assert!(data.samples_omitted);
    assert!(!data.truncated);
    assert_eq!(data.length_units[1].unit, None);
    assert!(
        data.length_units[1]
            .expression
            .contains("CONVERSION_BASED_UNIT")
    );
    assert!(serde_json::to_vec(&result).unwrap().len() < 32768);
}

#[test]
fn explicit_source_uri_is_encoded_and_resolves_to_the_input() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("part #1 温度.step");
    std::fs::write(&path, step("#1=PRODUCT('p','plate','',());")).unwrap();
    let result = inspect_path(
        &path,
        InspectOptions {
            include_paths: true,
            ..Default::default()
        },
    )
    .unwrap();
    let ir = result.ir.unwrap();
    let uri = ir["dialects"]["cad"]["source"]["uri"].as_str().unwrap();
    assert!(uri.starts_with("file:///"));
    assert!(uri.ends_with("part%20%231%20%E6%B8%A9%E5%BA%A6.step"));
    assert!(ir["dialects"]["cad"]["source"].get("sha256").is_none());
}

#[test]
fn cae_ir_preserves_presence_without_guessing_enabled_flags_or_parameters() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("unknown.mph");
    let file = std::fs::File::create(&path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("dmodel.xml", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"<Model><Physics tag='ht'/><Material tag='mat1'/><Study tag='std1'/><property param='active' value='false'/></Model>").unwrap();
    archive.finish().unwrap();
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let ir = result.ir.unwrap();
    let cae = &ir["dialects"]["cae"];
    assert_eq!(cae["features"].as_array().unwrap().len(), 3);
    assert!(
        cae["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f.get("enabled").is_none())
    );
    assert!(cae.get("parameters").is_none());
    assert!(cae.get("results").is_none());
}

#[test]
fn oversized_names_are_omitted_whole_with_explicit_counts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large.inp");
    let contents = (0..100)
        .map(|i| {
            format!(
                "*MATERIAL, NAME=MAT{i}{}\n",
                "X".repeat(if i == 0 { 600 } else { 500 })
            )
        })
        .collect::<String>();
    std::fs::write(&path, contents).unwrap();
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let ir = result.ir.unwrap();
    let cae = &ir["dialects"]["cae"];
    let attrs = &cae["extensions"]["abaqus"]["attributes"];
    let n = cae["features"].as_array().unwrap().len();
    assert!(n <= 24);
    assert_eq!(attrs["observed_feature_count"], 100);
    assert_eq!(attrs["omitted_feature_count"], 100 - n);
    assert!(serde_json::to_vec(&ir).unwrap().len() <= 16384);
}

#[test]
fn long_product_identifier_does_not_shift_name_or_description() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("long.step");
    std::fs::write(
        &path,
        step(&format!(
            "#1=PRODUCT('{}','actual name','description',());",
            "x".repeat(600)
        )),
    )
    .unwrap();
    let FormatSummary::Step(data) = inspect_path(&path, InspectOptions::default())
        .unwrap()
        .summary
    else {
        panic!()
    };
    assert_eq!(data.products[0].name, "actual name");
}
