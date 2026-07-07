use std::path::Path;

use hdf5_pure::{AttrValue, File};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Hdf5InspectError {
    #[error("hdf5 read failed: {0}")]
    Hdf5(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Hdf5AttributeInfo {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Hdf5GroupInfo {
    pub path: String,
    pub attributes: Vec<Hdf5AttributeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Hdf5DatasetInfo {
    pub path: String,
    pub shape: Vec<u64>,
    pub dtype: String,
    pub attributes: Vec<Hdf5AttributeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FluentHdf5Hints {
    pub zone_paths: Vec<String>,
    pub boundary_paths: Vec<String>,
    pub settings_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FluentHdf5Summary {
    pub root_attributes: Vec<Hdf5AttributeInfo>,
    pub groups: Vec<Hdf5GroupInfo>,
    pub datasets: Vec<Hdf5DatasetInfo>,
    pub hints: FluentHdf5Hints,
}

pub fn inspect_fluent_hdf5(path: &Path) -> Result<FluentHdf5Summary, Hdf5InspectError> {
    let file = File::open(path).map_err(to_err)?;
    let root = file.root();

    let mut groups = Vec::new();
    let mut datasets = Vec::new();
    walk_group(&file, "", &mut groups, &mut datasets)?;

    let mut summary = FluentHdf5Summary {
        root_attributes: attrs_to_vec(root.attrs().map_err(to_err)?),
        groups,
        datasets,
        hints: FluentHdf5Hints {
            zone_paths: Vec::new(),
            boundary_paths: Vec::new(),
            settings_paths: Vec::new(),
        },
    };
    summary.hints = collect_hints(&summary);
    Ok(summary)
}

fn walk_group(
    file: &File,
    path: &str,
    groups: &mut Vec<Hdf5GroupInfo>,
    datasets: &mut Vec<Hdf5DatasetInfo>,
) -> Result<(), Hdf5InspectError> {
    let group = if path.is_empty() {
        file.root()
    } else {
        file.group(path).map_err(to_err)?
    };

    let mut child_groups = group.groups().map_err(to_err)?;
    child_groups.sort();
    for child in child_groups {
        let child_path = join_hdf5(path, &child);
        let child_group = file.group(&child_path).map_err(to_err)?;
        groups.push(Hdf5GroupInfo {
            path: format!("/{child_path}"),
            attributes: attrs_to_vec(child_group.attrs().map_err(to_err)?),
        });
        walk_group(file, &child_path, groups, datasets)?;
    }

    let mut child_datasets = group.datasets().map_err(to_err)?;
    child_datasets.sort();
    for child in child_datasets {
        let child_path = join_hdf5(path, &child);
        let dataset = file.dataset(&child_path).map_err(to_err)?;
        datasets.push(Hdf5DatasetInfo {
            path: format!("/{child_path}"),
            shape: dataset.shape().map_err(to_err)?,
            dtype: format!("{:?}", dataset.dtype().map_err(to_err)?),
            attributes: attrs_to_vec(dataset.attrs().map_err(to_err)?),
        });
    }

    Ok(())
}

fn join_hdf5(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

fn attrs_to_vec(attrs: std::collections::HashMap<String, AttrValue>) -> Vec<Hdf5AttributeInfo> {
    let mut out: Vec<_> = attrs
        .into_iter()
        .map(|(name, value)| Hdf5AttributeInfo {
            name,
            value: format!("{value:?}"),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn collect_hints(summary: &FluentHdf5Summary) -> FluentHdf5Hints {
    let mut zone_paths = Vec::new();
    let mut boundary_paths = Vec::new();
    let mut settings_paths = Vec::new();

    for path in summary
        .groups
        .iter()
        .map(|g| g.path.as_str())
        .chain(summary.datasets.iter().map(|d| d.path.as_str()))
    {
        let low = path.to_ascii_lowercase();
        if low.contains("zone") {
            zone_paths.push(path.to_string());
        }
        if low.contains("boundary") || low.contains("boundaries") || low.contains("bc") {
            boundary_paths.push(path.to_string());
        }
        if low.contains("setting") || low.contains("solver") || low.contains("model") {
            settings_paths.push(path.to_string());
        }
    }

    FluentHdf5Hints {
        zone_paths,
        boundary_paths,
        settings_paths,
    }
}

fn to_err(error: hdf5_pure::Error) -> Hdf5InspectError {
    Hdf5InspectError::Hdf5(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hdf5_pure::{AttrValue, FileBuilder};

    #[test]
    fn reads_synthetic_hdf5_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mini.cas.h5");

        let mut builder = FileBuilder::new();
        builder.set_attr("fluent_version", AttrValue::String("synthetic".into()));

        let mut settings = builder.create_group("settings");
        settings.set_attr("solver", AttrValue::String("pressure-based".into()));
        settings.create_dataset("models").with_i32_data(&[1, 2, 3]);
        builder.add_group(settings.finish());

        let mut zones = builder.create_group("cell_zones");
        zones.create_dataset("fluid").with_i32_data(&[42]);
        builder.add_group(zones.finish());

        builder.write(&path).unwrap();

        let summary = inspect_fluent_hdf5(&path).unwrap();
        assert_eq!(summary.root_attributes[0].name, "fluent_version");
        assert!(summary.groups.iter().any(|g| g.path == "/settings"));
        assert!(
            summary
                .datasets
                .iter()
                .any(|d| d.path == "/settings/models")
        );
        assert!(
            summary
                .hints
                .zone_paths
                .iter()
                .any(|p| p.contains("cell_zones"))
        );
    }
}
