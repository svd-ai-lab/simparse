//! Test-only, Rust-native projections for the draft AI Infra System IR.
//! Usage: cargo run -p simparse-core --example ir_asset_probe -- assets.json output-dir
//! Sources are read-only. The output contains full native inspection sidecars and
//! compact trial manifests. This is not a production IR exporter or solver adapter.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use simparse_core::{InspectOptions, inspect_path};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const INLINE_SAMPLE: usize = 24;

fn sha256(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn reference(path: &Path, format: &str) -> Result<Value> {
    let absolute = fs::canonicalize(path)?;
    let path = absolute.to_string_lossy().replace('\\', "/");
    if path.starts_with("//?/UNC/") || (path.starts_with("//") && !path.starts_with("//?/")) {
        return Err("UNC paths are not supported by this offline probe".into());
    }
    let path = path.strip_prefix("//?/").unwrap_or(&path);
    let mut encoded = String::new();
    for byte in path.trim_start_matches('/').bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(json!({"uri": format!("file:///{encoded}"), "format": format, "sha256": sha256(&absolute)?}))
}

fn step_observations(path: &Path) -> Result<Value> {
    if fs::metadata(path)?.len() > 32 * 1024 * 1024 {
        return Err("STEP file exceeds probe read limit".into());
    }
    let text = fs::read_to_string(path)?;
    if !text.contains("ISO-10303-21;") {
        return Err("not a STEP physical file".into());
    }
    let mut entities = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for record in text.split(';') {
        let Some((left, right)) = record.trim().split_once('=') else {
            continue;
        };
        let Some(number) = left.trim().strip_prefix('#') else {
            continue;
        };
        if number.parse::<u64>().is_err() {
            continue;
        }
        let native_type = right.split('(').next().unwrap_or("").trim();
        let kind = match native_type {
            "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" => "body",
            "ADVANCED_FACE" => "face",
            "EDGE_CURVE" => "edge",
            "VERTEX_POINT" => "vertex",
            _ => continue,
        };
        *counts.entry(native_type.into()).or_default() += 1;
        entities.push(json!({"id": format!("step-{number}"), "kind": kind, "native_ref": format!("#{number}")}));
    }
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let units: BTreeSet<_> = compact
        .split("SI_UNIT(")
        .skip(1)
        .filter_map(|tail| tail.split_once(')').map(|(unit, _)| unit))
        .filter_map(|unit| unit.strip_suffix(",.METRE."))
        .collect();
    let mut observations = json!({"entities": entities, "observed_types": counts});
    if units.len() == 1 && !text.contains("CONVERSION_BASED_UNIT") {
        let unit = match units.first().copied() {
            Some("$") => Some("m"),
            Some(".MILLI.") => Some("mm"),
            Some(".MICRO.") => Some("um"),
            _ => None,
        };
        if let Some(unit) = unit {
            observations["length_unit"] = json!(unit);
        }
    }
    Ok(observations)
}

fn freecad_observations(path: &Path) -> Result<Value> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let mut entry = archive.by_name("Document.xml")?;
    if entry.size() > 4 * 1024 * 1024 {
        return Err("FreeCAD metadata exceeds probe read limit".into());
    }
    let mut xml = String::new();
    entry.read_to_string(&mut xml)?;
    let mut reader = Reader::from_str(&xml);
    let mut in_objects = false;
    let mut entities = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(ref element) if element.name().as_ref() == b"Objects" => in_objects = true,
            Event::End(ref element) if element.name().as_ref() == b"Objects" => in_objects = false,
            Event::Empty(ref element) | Event::Start(ref element)
                if in_objects && element.name().as_ref() == b"Object" =>
            {
                let mut name = None;
                let mut native_type = None;
                for attribute in element.attributes() {
                    let attribute = attribute?;
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"name" => name = Some(value),
                        b"type" => native_type = Some(value),
                        _ => {}
                    }
                }
                if let Some(name) = name {
                    let mut entity = json!({"id": name, "kind": "unknown", "native_ref": name});
                    if let Some(native_type) = native_type {
                        entity["extensions"] = json!({"freecad": {"version": "0", "attributes": {"object_type": native_type}}});
                    }
                    entities.push(entity);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(json!({"entities": entities}))
}

fn cae_projection(raw: &Value, source: Value, setup: Value) -> Value {
    let data = &raw["summary"]["data"];
    let format = raw["format"].as_str().unwrap_or("unknown");
    let namespace = match format {
        "comsol-mph" => "comsol",
        "hfss-aedt" => "ansys.aedt",
        "ansys-mechanical" => "ansys.mechanical",
        "fluent-hdf5" => "ansys.fluent",
        "abaqus-inp" => "abaqus",
        "flotherm-floxml" => "flotherm",
        _ => "source",
    };
    let mut features = Vec::new();
    // Preserve AEDT design identity separately from flattened setup inventories.
    if let Some(designs) = data["designs"].as_array() {
        for (index, design) in designs.iter().enumerate() {
            let mut feature = json!({"id": format!("design-{index}"), "kind": "analysis"});
            for (source_key, target_key) in [("name", "name"), ("design_type", "native_type")] {
                if let Some(value) = design[source_key].as_str().filter(|v| !v.is_empty()) {
                    feature[target_key] = json!(value);
                }
            }
            let mut attrs = serde_json::Map::new();
            for key in ["solution_type", "is_solved"] {
                if !design[key].is_null() {
                    attrs.insert(key.into(), design[key].clone());
                }
            }
            if !attrs.is_empty() {
                feature["extensions"] = json!({namespace: {"version": "0", "attributes": attrs}});
            }
            features.push(feature);
        }
    }
    for (key, kind) in [
        ("material_tags", "material"),
        ("materials", "material"),
        ("physics_tags", "physics"),
        ("study_tags", "study"),
        ("steps", "study"),
        ("setups", "study"),
        ("boundaries", "condition"),
        ("ports", "condition"),
        ("loads_and_conditions", "condition"),
        ("analyses", "analysis"),
        ("contacts", "interface"),
        ("mesh_operations", "mesh"),
    ] {
        if let Some(names) = data[key].as_array() {
            for (index, name) in names.iter().enumerate() {
                if let Some(name) = name.as_str().filter(|v| !v.is_empty()) {
                    features
                        .push(json!({"id": format!("{key}-{index}"), "kind": kind, "name": name}));
                }
            }
        }
    }
    let mut attrs = json!({"observed_feature_count": features.len()});
    for key in [
        "node_count",
        "element_count",
        "version_hint",
        "comsol_version",
        "mechanical_version",
        "product",
    ] {
        if data[key].is_number() || data[key].as_str().is_some_and(|value| !value.is_empty()) {
            attrs[key] = data[key].clone();
        }
    }
    for (key, label) in [
        ("parameters", "unscoped_value_count"),
        ("datasets", "dataset_count"),
        ("results", "result_object_count"),
    ] {
        if let Some(values) = data[key].as_array() {
            attrs[label] = json!(values.len());
        }
    }
    // Serialized COMSOL node properties remain in the native setup sidecar.
    // No enabled flags, parameter scopes, units or selections are guessed.
    features.truncate(INLINE_SAMPLE);
    let mut dialect = json!({"version": 0, "source": source, "features": features,
        "setup": setup, "extensions": {namespace: {"version": "0", "attributes": attrs}},
        "warnings": ["Test projection of shallow inventories; scopes and effective activation are not established."]});
    if matches!(format, "abaqus-inp" | "fluent-hdf5") {
        dialect["mesh"] = dialect["source"].clone();
    }
    dialect
}

fn probe(path: &Path, case_dir: &Path) -> Result<Value> {
    fs::create_dir_all(case_dir)?;
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let observations_path = case_dir.join("observations.json");
    let (document, format, count) = if matches!(extension.as_str(), "step" | "stp" | "fcstd") {
        let (raw, format) = if extension == "fcstd" {
            (freecad_observations(path)?, "freecad")
        } else {
            (step_observations(path)?, "step")
        };
        fs::write(&observations_path, serde_json::to_vec(&raw)?)?;
        let entities = raw["entities"]
            .as_array()
            .ok_or("entity inventory missing")?;
        let mut dialect = json!({"version": 0, "source": reference(path, format)?,
            "entities": entities.iter().take(INLINE_SAMPLE).collect::<Vec<_>>(),
            "inventory": reference(&observations_path, "json")?,
            "warnings": ["Test probe reads a shallow object inventory; topology and assembly transforms are not validated."]});
        if let Some(unit) = raw["length_unit"].as_str() {
            dialect["length_unit"] = json!(unit);
        }
        (
            json!({"schema": "ai-infra-system/v0", "dialects": {"cad": dialect}}),
            format.to_owned(),
            entities.len(),
        )
    } else {
        let result = inspect_path(path, InspectOptions::default())?;
        let format = result.format.to_string();
        let raw = serde_json::to_value(result)?;
        fs::write(&observations_path, serde_json::to_vec(&raw)?)?;
        let dialect = cae_projection(
            &raw,
            reference(path, &format)?,
            reference(&observations_path, "simparse-inspect-json")?,
        );
        let count = dialect["extensions"]
            .as_object()
            .and_then(|v| v.values().next())
            .and_then(|v| v["attributes"]["observed_feature_count"].as_u64())
            .unwrap_or(0) as usize;
        (
            json!({"schema": "ai-infra-system/v0", "dialects": {"cae": dialect}}),
            format,
            count,
        )
    };
    let bytes = serde_json::to_vec(&document)?;
    fs::write(case_dir.join("system.json"), &bytes)?;
    Ok(
        json!({"format": format, "source_bytes": fs::metadata(path)?.len(),
        "source_sha256": sha256(path)?, "manifest_bytes": bytes.len(), "observed_records": count}),
    )
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: ir_asset_probe assets.json output-dir".into());
    }
    let input = fs::read_to_string(&args[1])?;
    let paths: Vec<String> = serde_json::from_str(input.trim_start_matches('\u{feff}'))?;
    let output = Path::new(&args[2]);
    fs::create_dir_all(output)?;
    let mut results = Vec::new();
    let mut failed = false;
    for (index, path) in paths.iter().enumerate() {
        let mut result = match probe(Path::new(path), &output.join(format!("case-{index:03}"))) {
            Ok(mut value) => {
                value["ok"] = json!(true);
                value
            }
            Err(error) => {
                failed = true;
                json!({"ok": false, "error": error.to_string()})
            }
        };
        result["case"] = json!(index);
        println!("{}", serde_json::to_string(&result)?);
        result["path"] = json!(path);
        results.push(result);
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    if failed {
        return Err("one or more asset probes failed; inspect report.json".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(format: &str, data: Value) -> Value {
        cae_projection(
            &json!({"format": format, "summary": {"data": data}}),
            json!({"uri": "source.bin"}),
            json!({"uri": "native-inventory.json"}),
        )
    }

    #[test]
    fn comsol_serialized_properties_do_not_become_global_parameters() {
        let result = project(
            "comsol-mph",
            json!({"parameters": [
            {"name": "T", "value": "1|1,'300[K]'"}, {"name": "T", "value": "1|1,'400[K]'"}],
            "physics_tags": ["ht"], "is_runnable": false}),
        );
        assert!(result.get("parameters").is_none());
        assert!(result["features"][0].get("enabled").is_none());
        assert_eq!(
            result["extensions"]["comsol"]["attributes"]["unscoped_value_count"],
            2
        );
        assert_eq!(result["setup"]["uri"], "native-inventory.json");
    }

    #[test]
    fn aedt_designs_keep_solution_state_separate_from_activation() {
        let result = project(
            "hfss-aedt",
            json!({"designs": [
            {"name": "Design", "design_type": "HFSS", "solution_type": "DrivenModal", "is_solved": false},
            {"name": "Design", "design_type": "Icepak", "solution_type": "SteadyState", "is_solved": true}]}),
        );
        let features = result["features"].as_array().unwrap();
        assert_ne!(features[0]["id"], features[1]["id"]);
        assert_eq!(features[0]["native_type"], "HFSS");
        assert_eq!(features[1]["native_type"], "Icepak");
        assert!(
            features
                .iter()
                .all(|feature| feature.get("enabled").is_none())
        );
        assert_eq!(
            features[0]["extensions"]["ansys.aedt"]["attributes"]["is_solved"],
            false
        );
    }

    #[test]
    fn mechanical_contacts_and_result_objects_are_not_solve_evidence() {
        let result = project(
            "ansys-mechanical",
            json!({"analyses": ["Thermal"],
            "contacts": ["Die interface"], "loads_and_conditions": ["Temperature"],
            "results": ["Equivalent stress"], "mechanical_version": "test-version"}),
        );
        let kinds: Vec<_> = result["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["kind"].as_str().unwrap())
            .collect();
        assert!(
            kinds.contains(&"analysis")
                && kinds.contains(&"interface")
                && kinds.contains(&"condition")
        );
        assert!(result.get("results").is_none());
        assert_eq!(
            result["extensions"]["ansys.mechanical"]["attributes"]["result_object_count"],
            1
        );
    }

    #[test]
    fn large_feature_inventories_keep_the_full_sidecar() {
        let names: Vec<_> = (0..100).map(|i| format!("material-{i}")).collect();
        let result = project("comsol-mph", json!({"material_tags": names}));
        assert_eq!(result["features"].as_array().unwrap().len(), INLINE_SAMPLE);
        assert_eq!(
            result["extensions"]["comsol"]["attributes"]["observed_feature_count"],
            100
        );
        assert_eq!(result["setup"]["uri"], "native-inventory.json");
    }

    #[test]
    fn conflicting_step_units_remain_unknown() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("mixed.step");
        fs::write(
            &path,
            "ISO-10303-21;DATA;#1=SI_UNIT(.MILLI.,.METRE.);#2=SI_UNIT(.CENTI.,.METRE.);ENDSEC;",
        )?;
        assert!(step_observations(&path)?.get("length_unit").is_none());
        fs::write(
            &path,
            "ISO-10303-21;DATA;#1=SI_UNIT(.MILLI.,.METRE.);ENDSEC;",
        )?;
        assert_eq!(step_observations(&path)?["length_unit"], "mm");
        Ok(())
    }
}
