use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::{ComsolMphSummary, NamedValue, Result, SizeBucket};

pub fn inspect_comsol_mph(path: &Path) -> Result<ComsolMphSummary> {
    let file = std::fs::File::open(path)?;
    let mut zip = ZipArchive::new(file)?;

    let mut entries = Vec::new();
    let mut size_by_bucket: BTreeMap<String, u64> = BTreeMap::new();
    let mut fileversion = None;
    let mut modelinfo = None;
    let mut dmodel = None;
    let mut smodel = None;
    let mut used_licenses = Vec::new();

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().to_string();
        let size = entry.size();
        entries.push(name.clone());
        *size_by_bucket
            .entry(bucket_for(&name).to_string())
            .or_default() += size;

        match name.as_str() {
            "fileversion" => fileversion = Some(read_zip_text(&mut entry)?),
            "modelinfo.xml" => modelinfo = Some(read_zip_text(&mut entry)?),
            "dmodel.xml" => dmodel = Some(read_zip_text(&mut entry)?),
            "smodel.json" => smodel = Some(read_zip_text(&mut entry)?),
            "usedlicenses.txt" => {
                used_licenses = read_zip_text(&mut entry)?
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            _ => {}
        }
    }

    entries.sort();
    let (schema, saved_in) = parse_fileversion(fileversion.as_deref());
    let info = modelinfo
        .as_deref()
        .map(parse_modelinfo)
        .transpose()?
        .unwrap_or_default();
    let tree = dmodel
        .as_deref()
        .map(parse_model_tree)
        .transpose()?
        .unwrap_or_default();
    let smodel_tags = smodel
        .as_deref()
        .map(parse_smodel_tags)
        .transpose()?
        .unwrap_or_default();

    Ok(ComsolMphSummary {
        schema,
        saved_in,
        title: info.title,
        description: info.description,
        comsol_version: info.comsol_version,
        node_type: info.node_type,
        is_runnable: info.is_runnable,
        parameters: tree.parameters,
        physics_tags: merge_sorted(tree.physics_tags, smodel_tags.physics_tags),
        study_tags: merge_sorted(tree.study_tags, smodel_tags.study_tags),
        material_tags: merge_sorted(tree.material_tags, smodel_tags.material_tags),
        used_licenses,
        size_breakdown: size_by_bucket
            .into_iter()
            .map(|(bucket, bytes)| SizeBucket { bucket, bytes })
            .collect(),
        entries,
    })
}

fn read_zip_text<R: Read>(reader: &mut R) -> Result<String> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    Ok(text)
}

fn bucket_for(name: &str) -> &'static str {
    let low = name.to_ascii_lowercase();
    if low.contains("solution") || low.starts_with("savepoint") {
        "solution"
    } else if low.contains("mesh") {
        "mesh"
    } else if low.contains("geometry") || low.contains("geommanager") {
        "geometry"
    } else if low.ends_with(".mphbin") {
        "binary"
    } else {
        "data"
    }
}

fn parse_fileversion(text: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None);
    };
    let trimmed = text.trim();
    if let Some((schema, saved)) = trimmed.split_once(':') {
        (Some(schema.trim().into()), Some(saved.trim().into()))
    } else if trimmed.is_empty() {
        (None, None)
    } else {
        (None, Some(trimmed.into()))
    }
}

#[derive(Default)]
struct ModelInfo {
    title: Option<String>,
    description: Option<String>,
    comsol_version: Option<String>,
    node_type: Option<String>,
    is_runnable: Option<bool>,
}

fn parse_modelinfo(text: &str) -> Result<ModelInfo> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut out = ModelInfo::default();
    let mut current = String::new();

    loop {
        match reader.read_event()? {
            Event::Start(event) | Event::Empty(event) => {
                current = String::from_utf8_lossy(event.name().as_ref()).to_string();
                for attr in event.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                    assign_modelinfo(&mut out, &key, &value);
                }
            }
            Event::Text(text) => {
                let value = String::from_utf8_lossy(text.as_ref()).trim().to_string();
                if !value.is_empty() {
                    assign_modelinfo(&mut out, &current, &value);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn assign_modelinfo(out: &mut ModelInfo, key: &str, value: &str) {
    match key.to_ascii_lowercase().as_str() {
        "title" => out.title = Some(value.into()),
        "description" => out.description = Some(value.into()),
        "comsolversion" | "version" => out.comsol_version = Some(value.into()),
        "nodetype" => out.node_type = Some(value.into()),
        "isrunnable" | "runnable" => out.is_runnable = parse_bool(value),
        _ => {}
    }
}

#[derive(Default)]
struct TreeInfo {
    parameters: Vec<NamedValue>,
    physics_tags: Vec<String>,
    study_tags: Vec<String>,
    material_tags: Vec<String>,
}

fn parse_model_tree(text: &str) -> Result<TreeInfo> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut out = TreeInfo::default();

    loop {
        match reader.read_event()? {
            Event::Start(event) | Event::Empty(event) => {
                let element = String::from_utf8_lossy(event.name().as_ref()).to_string();
                let mut attrs = BTreeMap::new();
                for attr in event.attributes().flatten() {
                    attrs.insert(
                        String::from_utf8_lossy(attr.key.as_ref()).to_string(),
                        String::from_utf8_lossy(attr.value.as_ref()).to_string(),
                    );
                }
                let element_low = element.to_ascii_lowercase();
                if let Some(name) = attrs.get("param") {
                    out.parameters.push(NamedValue {
                        name: name.clone(),
                        value: attrs.get("value").cloned(),
                        reference: attrs.get("reference").cloned(),
                    });
                }
                if let Some(tag) = attrs.get("tag") {
                    if element_low == "physics" {
                        out.physics_tags.push(tag.clone());
                    } else if element_low == "study" {
                        out.study_tags.push(tag.clone());
                    } else if element_low == "material" {
                        out.material_tags.push(tag.clone());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    sort_dedup(&mut out.physics_tags);
    sort_dedup(&mut out.study_tags);
    sort_dedup(&mut out.material_tags);
    Ok(out)
}

fn parse_smodel_tags(text: &str) -> Result<TreeInfo> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let mut out = TreeInfo::default();
    collect_smodel_node(&value, &mut out);
    sort_dedup(&mut out.physics_tags);
    sort_dedup(&mut out.study_tags);
    sort_dedup(&mut out.material_tags);
    Ok(out)
}

fn collect_smodel_node(value: &serde_json::Value, out: &mut TreeInfo) {
    match value {
        serde_json::Value::Object(map) => {
            let tag = map.get("tag").and_then(serde_json::Value::as_str);
            let api = map
                .get("apiClass")
                .or_else(|| map.get("type"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Some(tag) = tag {
                if api == "physics" {
                    out.physics_tags.push(tag.into());
                } else if api == "study" {
                    out.study_tags.push(tag.into());
                } else if api == "material" {
                    out.material_tags.push(tag.into());
                }
            }
            for child in map.values() {
                collect_smodel_node(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_smodel_node(item, out);
            }
        }
        _ => {}
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn merge_sorted(mut a: Vec<String>, mut b: Vec<String>) -> Vec<String> {
    a.append(&mut b);
    sort_dedup(&mut a);
    a
}

fn sort_dedup(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

#[cfg(test)]
mod tests {
    use super::{parse_model_tree, parse_smodel_tags};

    #[test]
    fn xml_inventory_distinguishes_objects_from_lists_and_features() {
        let tree = parse_model_tree(
            r#"<Model>
              <PhysicsList tag="physics"><Physics tag="heat">
                <PhysicsFeature tag="flux"/><PhysicsProp tag="settings"/>
              </Physics></PhysicsList>
              <MultiphysicsCoupling tag="coupling"/>
              <StudyList tag="study"><Study tag="analysis">
                <StudyFeature tag="step"/>
              </Study></StudyList>
              <MaterialList tag="material"><Material tag="copper">
                <MaterialModel tag="properties"/>
              </Material></MaterialList>
            </Model>"#,
        )
        .unwrap();
        assert_eq!(tree.physics_tags, ["heat"]);
        assert_eq!(tree.study_tags, ["analysis"]);
        assert_eq!(tree.material_tags, ["copper"]);
    }

    #[test]
    fn json_inventory_recurses_without_including_child_classes() {
        let tree = parse_smodel_tags(
            r#"{"nodes":[
              {"apiClass":"PhysicsList","tag":"physics","nodes":[
                {"apiClass":"Physics","tag":"heat","nodes":[
                  {"apiClass":"PhysicsFeature","tag":"flux"}]},
                {"apiClass":"Physics","tag":"flow"},
                {"apiClass":"Physics","tag":"heat"}]},
              {"apiClass":"MultiphysicsCoupling","tag":"coupling"},
              {"type":"Study","tag":"analysis","nodes":[
                {"type":"StudyFeature","tag":"step"}]},
              {"type":"material","tag":"copper","nodes":[
                {"type":"MaterialModel","tag":"properties"}]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(tree.physics_tags, ["flow", "heat"]);
        assert_eq!(tree.study_tags, ["analysis"]);
        assert_eq!(tree.material_tags, ["copper"]);
    }
}
