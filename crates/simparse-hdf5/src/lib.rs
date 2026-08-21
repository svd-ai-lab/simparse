use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MechanicalObjectTypeCount {
    pub type_id: u32,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MechanicalStreamInfo {
    pub name: String,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MechanicalSidecars {
    pub lock_file_present: bool,
    pub project_files_dir_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MechanicalMechdbSummary {
    pub mechanical_version: Option<String>,
    pub database_format_version: Option<String>,
    pub release_kind: Option<String>,
    pub object_count: usize,
    pub object_type_counts: Vec<MechanicalObjectTypeCount>,
    pub analyses: Vec<String>,
    pub bodies: Vec<String>,
    pub materials: Vec<String>,
    pub contacts: Vec<String>,
    pub loads_and_conditions: Vec<String>,
    pub results: Vec<String>,
    pub geometry_sources: Vec<String>,
    pub streams: Vec<MechanicalStreamInfo>,
    pub sidecars: MechanicalSidecars,
    pub content_truncated: bool,
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

pub fn inspect_mechanical_mechdb(
    path: &Path,
    max_text_bytes: usize,
) -> Result<MechanicalMechdbSummary, Hdf5InspectError> {
    let file = File::open(path).map_err(to_err)?;
    let mut groups = Vec::new();
    let mut datasets = Vec::new();
    walk_group(&file, "", &mut groups, &mut datasets)?;

    let streams = datasets
        .iter()
        .map(|dataset| MechanicalStreamInfo {
            name: dataset.path.clone(),
            used_bytes: dataset_used_bytes(dataset),
        })
        .collect();

    let mut content_truncated = false;
    let version_text = read_named_bytes(&file, &datasets, "/Version Info Stream", max_text_bytes)?
        .map(|(bytes, truncated)| {
            content_truncated |= truncated;
            decode_prefixed_utf16le(&bytes)
        });
    let (database_format_version, mechanical_version, release_kind) = version_text
        .as_deref()
        .map(parse_version_stream)
        .unwrap_or((None, None, None));

    let object_text =
        read_named_bytes(&file, &datasets, "/DS Objects Information", max_text_bytes)?
            .map(|(bytes, truncated)| {
                content_truncated |= truncated;
                decode_prefixed_utf16le(&bytes)
            })
            .unwrap_or_default();
    let objects = parse_object_inventory(&object_text);
    let mut type_counts = BTreeMap::new();
    let mut analyses = Vec::new();
    let mut bodies = Vec::new();
    let mut materials = Vec::new();
    let mut contacts = Vec::new();
    let mut loads_and_conditions = Vec::new();
    let mut results = Vec::new();
    for object in &objects {
        *type_counts.entry(object.type_id).or_insert(0usize) += 1;
        let target = match object.type_id {
            105 => Some(&mut analyses),
            400 => Some(&mut bodies),
            406 => Some(&mut materials),
            575 => Some(&mut contacts),
            403 => Some(&mut loads_and_conditions),
            520 => Some(&mut results),
            _ => None,
        };
        if let Some(target) = target
            && let Some(name) = safe_inventory_name(&object.name)
        {
            target.push(name);
        }
    }

    let geometry_sources =
        read_named_bytes(&file, &datasets, "/Part Manager Stream", max_text_bytes)?
            .and_then(|(bytes, truncated)| {
                content_truncated |= truncated;
                decompress_prefixed_zlib(&bytes, max_text_bytes).map(|(decoded, truncated)| {
                    content_truncated |= truncated;
                    extract_geometry_sources(&decoded)
                })
            })
            .unwrap_or_default();

    Ok(MechanicalMechdbSummary {
        mechanical_version,
        database_format_version,
        release_kind,
        object_count: objects.len(),
        object_type_counts: type_counts
            .into_iter()
            .map(|(type_id, count)| MechanicalObjectTypeCount { type_id, count })
            .collect(),
        analyses: sorted_dedup(analyses),
        bodies: sorted_dedup(bodies),
        materials: sorted_dedup(materials),
        contacts: sorted_dedup(contacts),
        loads_and_conditions: sorted_dedup(loads_and_conditions),
        results: sorted_dedup(results),
        geometry_sources,
        streams,
        sidecars: mechanical_sidecars(path),
        content_truncated,
    })
}

#[derive(Debug)]
struct MechanicalObject {
    type_id: u32,
    name: String,
}

fn read_named_bytes(
    file: &File,
    datasets: &[Hdf5DatasetInfo],
    path: &str,
    max_bytes: usize,
) -> Result<Option<(Vec<u8>, bool)>, Hdf5InspectError> {
    let Some(info) = datasets.iter().find(|dataset| dataset.path == path) else {
        return Ok(None);
    };
    let dataset = file.dataset(path.trim_start_matches('/')).map_err(to_err)?;
    let mut bytes = dataset.read_u8().map_err(to_err)?;
    let used = dataset_used_bytes(info).min(bytes.len() as u64) as usize;
    bytes.truncate(used);
    let truncated = bytes.len() > max_bytes;
    bytes.truncate(max_bytes);
    Ok(Some((bytes, truncated)))
}

fn dataset_used_bytes(dataset: &Hdf5DatasetInfo) -> u64 {
    dataset
        .attributes
        .iter()
        .find(|attribute| attribute.name == "UsedSize")
        .and_then(|attribute| {
            attribute
                .value
                .strip_prefix("U64(")
                .and_then(|value| value.strip_suffix(')'))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| dataset.shape.iter().copied().product())
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
    char::decode_utf16(units)
        .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn decode_prefixed_utf16le(bytes: &[u8]) -> String {
    decode_utf16le(bytes.get(4..).unwrap_or_default())
}

fn parse_version_stream(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let fields: Vec<_> = text.trim_matches(['\0', '|']).split('|').collect();
    (
        fields.first().and_then(nonempty),
        fields.get(2).and_then(nonempty),
        fields.get(4).and_then(nonempty),
    )
}

fn nonempty(value: &&str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_object_inventory(text: &str) -> Vec<MechanicalObject> {
    text.split('\0')
        .filter_map(|record| {
            let mut fields = record.trim().splitn(4, ',');
            if fields.next()?.trim() != "|" {
                return None;
            }
            let _object_id = fields.next()?.trim().parse::<u32>().ok()?;
            let type_id = fields.next()?.trim().parse::<u32>().ok()?;
            let name = fields.next()?.trim().to_string();
            Some(MechanicalObject { type_id, name })
        })
        .collect()
}

fn safe_inventory_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("[NONAME]") {
        return None;
    }
    if looks_like_path(value) {
        return value
            .replace('\\', "/")
            .rsplit('/')
            .next()
            .map(ToOwned::to_owned);
    }
    Some(value.to_string())
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.contains("\\") || value.as_bytes().get(1).copied() == Some(b':')
}

fn decompress_prefixed_zlib(bytes: &[u8], max_bytes: usize) -> Option<(Vec<u8>, bool)> {
    let payload = bytes.get(4..)?;
    if payload.first().copied() != Some(0x78) {
        return None;
    }
    let mut decoder = ZlibDecoder::new(payload).take(max_bytes as u64 + 1);
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded).ok()?;
    let truncated = decoded.len() > max_bytes;
    decoded.truncate(max_bytes);
    Some((decoded, truncated))
}

fn extract_geometry_sources(bytes: &[u8]) -> Vec<String> {
    let mut sources = Vec::new();
    let mut start = None;
    for (index, byte) in bytes.iter().copied().chain(std::iter::once(0)).enumerate() {
        if (0x20..=0x7e).contains(&byte) {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take()
            && index.saturating_sub(begin) >= 4
            && let Ok(value) = std::str::from_utf8(&bytes[begin..index])
            && is_geometry_path(value)
            && let Some(name) = value.replace('\\', "/").rsplit('/').next()
        {
            sources.push(name.to_string());
        }
    }
    sorted_dedup(sources)
}

fn is_geometry_path(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [".step", ".stp", ".iges", ".igs", ".x_t", ".x_b", ".scdoc"]
        .iter()
        .any(|extension| value.ends_with(extension))
}

fn sorted_dedup(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn mechanical_sidecars(path: &Path) -> MechanicalSidecars {
    let lock_file = PathBuf::from(format!("{}.lock", path.display()));
    let project_files_dir = path.file_stem().and_then(|stem| {
        path.parent()
            .map(|parent| parent.join(format!("{}_Mech_Files", stem.to_string_lossy())))
    });
    MechanicalSidecars {
        lock_file_present: lock_file.is_file(),
        project_files_dir_present: project_files_dir.is_some_and(|path| path.is_dir()),
    }
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
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use hdf5_pure::{AttrValue, FileBuilder};
    use std::io::Write;

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

    #[test]
    fn reads_synthetic_mechanical_inventory_without_leaking_source_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("package.mechdb");

        let version = utf16_stream("2|25.2|2025 R2|250200|RELEASE|");
        let objects = utf16_stream(
            "|, 1, 802, C:\\private\\package.mechdb\0\
             |, 2, 400, Silicon Die\0\
             |, 3, 406, Silicon\0\
             |, 4, 575, Die Contact\0\
             |, 5, 105, Thermal Stress\0\
             |, 6, 403, Cool Down\0\
             |, 7, 520, Equivalent Stress\0",
        );
        let part_manager = compressed_stream(b"C:\\private\\geometry\\package.step\0");

        let mut builder = FileBuilder::new();
        builder
            .create_dataset("Version Info Stream")
            .with_u8_data(&version)
            .set_attr("UsedSize", AttrValue::U64(version.len() as u64));
        builder
            .create_dataset("DS Objects Information")
            .with_u8_data(&objects)
            .set_attr("UsedSize", AttrValue::U64(objects.len() as u64));
        builder
            .create_dataset("Part Manager Stream")
            .with_u8_data(&part_manager)
            .set_attr("UsedSize", AttrValue::U64(part_manager.len() as u64));
        builder.write(&path).unwrap();
        std::fs::create_dir(tmp.path().join("package_Mech_Files")).unwrap();

        let summary = inspect_mechanical_mechdb(&path, 64 * 1024).unwrap();
        assert_eq!(summary.mechanical_version.as_deref(), Some("2025 R2"));
        assert_eq!(summary.database_format_version.as_deref(), Some("2"));
        assert_eq!(summary.object_count, 7);
        assert_eq!(summary.bodies, ["Silicon Die"]);
        assert_eq!(summary.materials, ["Silicon"]);
        assert_eq!(summary.contacts, ["Die Contact"]);
        assert_eq!(summary.analyses, ["Thermal Stress"]);
        assert_eq!(summary.loads_and_conditions, ["Cool Down"]);
        assert_eq!(summary.results, ["Equivalent Stress"]);
        assert_eq!(summary.geometry_sources, ["package.step"]);
        assert!(summary.sidecars.project_files_dir_present);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("private"));
    }

    fn utf16_stream(text: &str) -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn compressed_stream(text: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(text).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut bytes = (compressed.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&compressed);
        bytes
    }
}
