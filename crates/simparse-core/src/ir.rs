//! Small projections of existing observations. Never infer solver state.
use crate::{FormatSummary, SimparseResult};
use serde_json::{Map, Value, json};

const FEATURES: usize = 24;
const TEXT_BYTES: usize = 512;
const MANIFEST_BYTES: usize = 16 * 1024;

fn text(value: &str) -> Option<&str> {
    (!value.is_empty() && value.len() <= TEXT_BYTES).then_some(value)
}

fn encode(value: &str, path: bool) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || b"-._~".contains(&byte)
            || (path && b"/:".contains(&byte))
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn source(result: &SimparseResult) -> Value {
    if let Some(path) = &result.path
        && let Ok(path) = std::path::absolute(path)
    {
        let path = path.to_string_lossy().replace('\\', "/");
        let path = path
            .strip_prefix("//?/UNC/")
            .map(|s| format!("//{s}"))
            .unwrap_or_else(|| path.strip_prefix("//?/").unwrap_or(&path).into());
        let uri = if let Some(unc) = path.strip_prefix("//") {
            format!("file://{}", encode(unc, true))
        } else {
            format!("file:///{}", encode(path.trim_start_matches('/'), true))
        };
        if uri.len() <= 4096 {
            return json!({"uri": uri, "format": result.format});
        }
    }
    let name = text(&result.file_name).unwrap_or("source");
    json!({"uri": format!("urn:simparse:source:{}", encode(name, false)), "format": result.format})
}

struct Features {
    items: Vec<Value>,
    observed: usize,
}

impl Features {
    fn names(&mut self, prefix: &str, kind: &str, names: &[String]) {
        for (index, name) in names.iter().enumerate() {
            self.observed += 1;
            if self.items.len() < FEATURES
                && let Some(name) = text(name)
            {
                self.items
                    .push(json!({"id": format!("{prefix}-{index}"), "kind": kind, "name": name}));
            }
        }
    }
}

pub(crate) fn project(result: &SimparseResult) -> Value {
    let source = source(result);
    let mut attrs = Map::new();
    let mut features = Features {
        items: vec![],
        observed: 0,
    };
    let namespace;
    let mut dialect = json!({"version": 0, "source": source, "setup": source,
        "warnings": ["Selected shallow observations only. Feature scopes, effective activation, physical validity and result freshness are unknown."]});
    match &result.summary {
        FormatSummary::Step(step) => {
            namespace = "step";
            dialect = json!({"version": 0, "source": source, "inventory": source,
                "warnings": ["Counts describe observed STEP declarations, not assembly instances or validated topology. Length-unit declarations are not resolved to representation contexts; geometry validity and meshability are unknown."]});
            attrs.insert("records_observed".into(), json!(step.records_observed));
            attrs.insert("truncated".into(), json!(step.truncated));
            attrs.insert("samples_omitted".into(), json!(step.samples_omitted));
            for (index, unit) in step.length_units.iter().enumerate() {
                attrs.insert(
                    format!("length_unit_declaration_{index}"),
                    json!(format!("{} = {}", unit.native_ref, unit.expression)),
                );
            }
            for name in [
                "PRODUCT",
                "MANIFOLD_SOLID_BREP",
                "BREP_WITH_VOIDS",
                "SHELL_BASED_SURFACE_MODEL",
                "ADVANCED_FACE",
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
            ] {
                if let Some(count) = step.entity_type_counts.iter().find(|c| c.name == name) {
                    attrs.insert(
                        format!("observed_{}_count", name.to_ascii_lowercase()),
                        json!(count.count),
                    );
                }
            }
        }
        FormatSummary::ComsolMph(data) => {
            namespace = "comsol";
            features.names("physics", "physics", &data.physics_tags);
            features.names("materials", "material", &data.material_tags);
            features.names("studies", "study", &data.study_tags);
            attrs.insert("unscoped_value_count".into(), json!(data.parameters.len()));
            if let Some(version) = data.comsol_version.as_deref().and_then(text) {
                attrs.insert("software_version".into(), json!(version));
            }
        }
        FormatSummary::HfssAedt(data) => {
            namespace = "ansys.aedt";
            for (index, design) in data.designs.iter().enumerate() {
                features.observed += 1;
                if features.items.len() >= FEATURES {
                    continue;
                }
                let mut feature = json!({"id": format!("design-{index}"), "kind": "analysis"});
                if let Some(name) = text(&design.name) {
                    feature["name"] = json!(name);
                }
                if let Some(kind) = design.design_type.as_deref().and_then(text) {
                    feature["native_type"] = json!(kind);
                }
                let mut native = Map::new();
                if let Some(value) = design.is_solved {
                    native.insert("is_solved".into(), json!(value));
                }
                if let Some(value) = design.solution_type.as_deref().and_then(text) {
                    native.insert("solution_type".into(), json!(value));
                }
                if !native.is_empty() {
                    feature["extensions"] =
                        json!({namespace: {"version": "0", "attributes": native}});
                }
                features.items.push(feature);
            }
            features.names("materials", "material", &data.materials);
            features.names("setups", "study", &data.setups);
            features.names("boundaries", "condition", &data.boundaries);
            features.names("ports", "condition", &data.ports);
            features.names("mesh", "mesh", &data.mesh_operations);
            attrs.insert("source_truncated".into(), json!(data.truncated));
        }
        FormatSummary::AnsysMechanical(data) => {
            namespace = "ansys.mechanical";
            features.names("analyses", "analysis", &data.analyses);
            features.names("materials", "material", &data.materials);
            features.names("contacts", "interface", &data.contacts);
            features.names("conditions", "condition", &data.loads_and_conditions);
            attrs.insert("result_object_count".into(), json!(data.results.len()));
            attrs.insert("source_truncated".into(), json!(data.content_truncated));
        }
        FormatSummary::AbaqusInp(data) => {
            namespace = "abaqus";
            features.names("materials", "material", &data.materials);
            features.names("steps", "study", &data.steps);
            // Keywords indicate categories, not individual boundary objects.
            attrs.insert("node_records".into(), json!(data.node_count));
            attrs.insert("element_records".into(), json!(data.element_count));
        }
        FormatSummary::FluentHdf5(data) => {
            namespace = "ansys.fluent";
            attrs.insert("observed_dataset_count".into(), json!(data.datasets.len()));
        }
        FormatSummary::FlothermFloxml(data) => {
            namespace = "flotherm";
            attrs.insert("source_truncated".into(), json!(data.truncated));
        }
        FormatSummary::FlothermPack(data) => {
            namespace = "flotherm";
            attrs.insert("archive_entry_count".into(), json!(data.entry_count));
            attrs.insert("source_truncated".into(), json!(data.truncated));
        }
        FormatSummary::IcepakTzr(data) => {
            namespace = "ansys.icepak";
            attrs.insert("archive_entry_count".into(), json!(data.entry_count));
            attrs.insert("source_truncated".into(), json!(data.truncated));
        }
    }
    let key = if matches!(result.summary, FormatSummary::Step(_)) {
        "cad"
    } else {
        "cae"
    };
    if key == "cae" {
        attrs.insert("observed_feature_count".into(), json!(features.observed));
        if !features.items.is_empty() {
            dialect["features"] = json!(features.items);
        }
        attrs.insert(
            "omitted_feature_count".into(),
            json!(features.observed - features.items.len()),
        );
    }
    dialect["extensions"] = json!({namespace: {"version": "0", "attributes": attrs}});
    let mut out = json!({"schema": "ai-infra-system/v0", "dialects": {key: dialect}});
    if source["uri"]
        .as_str()
        .is_some_and(|s| s.starts_with("urn:"))
    {
        out["warnings"] = json!([
            "Source location is hidden. The source URN is a descriptive handle, not a unique identity or resolvable file path. Use include_paths for file URIs."
        ]);
    }
    // Drop whole features, never shorten an identity or expression to fit.
    while serde_json::to_vec(&out).is_ok_and(|v| v.len() > MANIFEST_BYTES) {
        let Some(items) = out["dialects"][key]["features"].as_array_mut() else {
            break;
        };
        if items.pop().is_none() {
            break;
        }
        let omitted = features.observed - items.len();
        out["dialects"][key]["extensions"][namespace]["attributes"]["omitted_feature_count"] =
            json!(omitted);
    }
    out
}
