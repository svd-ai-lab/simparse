use std::path::Path;

use crate::{
    AbaqusInpSummary, FormatSummary, HfssAedtSummary, HfssDesign, NamedValue, SimparseResult,
};
use simparse_hdf5::{FluentHdf5Summary, Hdf5AttributeInfo, Hdf5DatasetInfo, Hdf5GroupInfo};

pub const SUMMARY_SAMPLE_LIMIT: usize = 8;
pub const SUMMARY_TEXT_BYTES: usize = 256;

pub fn summarize_result(result: &SimparseResult) -> SimparseSummaryResult {
    let (summary, limitations) = match &result.summary {
        FormatSummary::ComsolMph(value) => (
            CompactFormatSummary::ComsolMph(CompactComsolMphSummary {
                schema: option_text(value.schema.as_deref()),
                saved_in: option_text(value.saved_in.as_deref()),
                title: option_text(value.title.as_deref()),
                description: option_text(value.description.as_deref()),
                comsol_version: option_text(value.comsol_version.as_deref()),
                node_type: option_text(value.node_type.as_deref()),
                is_runnable: value.is_runnable,
                parameters: bounded_map(&value.parameters, compact_named_value),
                physics_tags: bounded_texts(&value.physics_tags),
                study_tags: bounded_texts(&value.study_tags),
                material_tags: bounded_texts(&value.material_tags),
                used_licenses: bounded_texts(&value.used_licenses),
                size_breakdown: bounded_map(&value.size_breakdown, |item| CompactSizeBucket {
                    bucket: bounded_text(&item.bucket),
                    bytes: item.bytes,
                }),
                entries: bounded_texts(&value.entries),
            }),
            vec![
                "Shallow archive metadata only; geometry, mesh, solution data, and model semantics are not evaluated."
                    .to_string(),
            ],
        ),
        FormatSummary::AbaqusInp(value) => (
            CompactFormatSummary::AbaqusInp(compact_abaqus(value)),
            vec![
                "Keyword inventory only; generated entities and solver semantics are not evaluated."
                    .to_string(),
            ],
        ),
        FormatSummary::FluentHdf5(value) => (
            CompactFormatSummary::FluentHdf5(compact_fluent(value)),
            vec![
                "Shallow HDF5 structure and metadata only; field arrays and case/data consistency are not evaluated."
                    .to_string(),
            ],
        ),
        FormatSummary::HfssAedt(value) => (
            CompactFormatSummary::HfssAedt(compact_hfss(value)),
            vec![
                "Text-pattern inventory only; the vendor object tree and electromagnetic model semantics are not evaluated."
                    .to_string(),
            ],
        ),
    };

    SimparseSummaryResult {
        schema_version: 1,
        view: "summary".to_string(),
        path: result.path.as_deref().map(bounded_text),
        file_name: bounded_text(&result.file_name),
        format: result.format,
        ok: result.ok,
        warnings: bounded_texts(&result.warnings),
        limitations,
        summary,
    }
}

fn compact_abaqus(value: &AbaqusInpSummary) -> CompactAbaqusInpSummary {
    CompactAbaqusInpSummary {
        title: option_text(value.title.as_deref()),
        includes: bounded_map(&value.includes, |path| bounded_path(path)),
        node_count: value.node_count,
        element_count: value.element_count,
        materials: bounded_texts(&value.materials),
        sections: bounded_texts(&value.sections),
        steps: bounded_texts(&value.steps),
        boundary_keywords: bounded_texts(&value.boundary_keywords),
        load_keywords: bounded_texts(&value.load_keywords),
        output_keywords: bounded_texts(&value.output_keywords),
        unknown_keywords: bounded_texts(&value.unknown_keywords),
    }
}

fn compact_fluent(value: &FluentHdf5Summary) -> CompactFluentHdf5Summary {
    CompactFluentHdf5Summary {
        root_attributes: bounded_map(&value.root_attributes, compact_attribute),
        groups: bounded_map(&value.groups, compact_group),
        datasets: bounded_map(&value.datasets, compact_dataset),
        zone_paths: bounded_texts(&value.hints.zone_paths),
        boundary_paths: bounded_texts(&value.hints.boundary_paths),
        settings_paths: bounded_texts(&value.hints.settings_paths),
    }
}

fn compact_group(value: &Hdf5GroupInfo) -> CompactHdf5Group {
    CompactHdf5Group {
        path: bounded_text(&value.path),
        attributes: bounded_map(&value.attributes, compact_attribute),
    }
}

fn compact_dataset(value: &Hdf5DatasetInfo) -> CompactHdf5Dataset {
    CompactHdf5Dataset {
        path: bounded_text(&value.path),
        shape: bounded_map(&value.shape, |item| *item),
        dtype: bounded_text(&value.dtype),
        attributes: bounded_map(&value.attributes, compact_attribute),
    }
}

fn compact_attribute(value: &Hdf5AttributeInfo) -> CompactHdf5Attribute {
    CompactHdf5Attribute {
        name: bounded_text(&value.name),
        value: bounded_text(&value.value),
    }
}

fn compact_hfss(value: &HfssAedtSummary) -> CompactHfssAedtSummary {
    CompactHfssAedtSummary {
        project_name: option_text(value.project_name.as_deref()),
        version_hint: option_text(value.version_hint.as_deref()),
        designs: bounded_map(&value.designs, compact_hfss_design),
        variables: bounded_texts(&value.variables),
        setups: bounded_texts(&value.setups),
        sweeps: bounded_texts(&value.sweeps),
        ports: bounded_texts(&value.ports),
        boundaries: bounded_texts(&value.boundaries),
        source_member: option_text(value.source_member.as_deref()),
        lock_file_present: value.sidecars.lock_file_present,
        results_dir_present: value.sidecars.results_dir_present,
        source_truncated: value.truncated,
    }
}

fn compact_hfss_design(value: &HfssDesign) -> CompactHfssDesign {
    CompactHfssDesign {
        name: bounded_text(&value.name),
        design_type: option_text(value.design_type.as_deref()),
    }
}

fn compact_named_value(value: &NamedValue) -> CompactNamedValue {
    CompactNamedValue {
        name: bounded_text(&value.name),
        value: option_text(value.value.as_deref()),
        reference: option_text(value.reference.as_deref()),
    }
}

fn bounded_texts(values: &[String]) -> BoundedList<String> {
    bounded_map(values, |value| bounded_text(value))
}

fn bounded_path(value: &Path) -> String {
    bounded_text(&value.to_string_lossy())
}

fn option_text(value: Option<&str>) -> Option<String> {
    value.map(bounded_text)
}

fn bounded_text(value: &str) -> String {
    if value.len() <= SUMMARY_TEXT_BYTES {
        return value.to_string();
    }
    let mut end = SUMMARY_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn bounded_map<T, U, F>(values: &[T], mut map: F) -> BoundedList<U>
where
    F: FnMut(&T) -> U,
{
    BoundedList {
        total: values.len(),
        sample: values
            .iter()
            .take(SUMMARY_SAMPLE_LIMIT)
            .map(&mut map)
            .collect(),
        truncated: values.len() > SUMMARY_SAMPLE_LIMIT,
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct BoundedList<T> {
    pub total: usize,
    pub sample: Vec<T>,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SimparseSummaryResult {
    pub schema_version: u32,
    pub view: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub file_name: String,
    pub format: crate::SimFormat,
    pub ok: bool,
    pub warnings: BoundedList<String>,
    pub limitations: Vec<String>,
    pub summary: CompactFormatSummary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum CompactFormatSummary {
    ComsolMph(CompactComsolMphSummary),
    AbaqusInp(CompactAbaqusInpSummary),
    FluentHdf5(CompactFluentHdf5Summary),
    HfssAedt(CompactHfssAedtSummary),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactComsolMphSummary {
    pub schema: Option<String>,
    pub saved_in: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub comsol_version: Option<String>,
    pub node_type: Option<String>,
    pub is_runnable: Option<bool>,
    pub parameters: BoundedList<CompactNamedValue>,
    pub physics_tags: BoundedList<String>,
    pub study_tags: BoundedList<String>,
    pub material_tags: BoundedList<String>,
    pub used_licenses: BoundedList<String>,
    pub size_breakdown: BoundedList<CompactSizeBucket>,
    pub entries: BoundedList<String>,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactNamedValue {
    pub name: String,
    pub value: Option<String>,
    pub reference: Option<String>,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactSizeBucket {
    pub bucket: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactAbaqusInpSummary {
    pub title: Option<String>,
    pub includes: BoundedList<String>,
    pub node_count: usize,
    pub element_count: usize,
    pub materials: BoundedList<String>,
    pub sections: BoundedList<String>,
    pub steps: BoundedList<String>,
    pub boundary_keywords: BoundedList<String>,
    pub load_keywords: BoundedList<String>,
    pub output_keywords: BoundedList<String>,
    pub unknown_keywords: BoundedList<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFluentHdf5Summary {
    pub root_attributes: BoundedList<CompactHdf5Attribute>,
    pub groups: BoundedList<CompactHdf5Group>,
    pub datasets: BoundedList<CompactHdf5Dataset>,
    pub zone_paths: BoundedList<String>,
    pub boundary_paths: BoundedList<String>,
    pub settings_paths: BoundedList<String>,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactHdf5Attribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactHdf5Group {
    pub path: String,
    pub attributes: BoundedList<CompactHdf5Attribute>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactHdf5Dataset {
    pub path: String,
    pub shape: BoundedList<u64>,
    pub dtype: String,
    pub attributes: BoundedList<CompactHdf5Attribute>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactHfssAedtSummary {
    pub project_name: Option<String>,
    pub version_hint: Option<String>,
    pub designs: BoundedList<CompactHfssDesign>,
    pub variables: BoundedList<String>,
    pub setups: BoundedList<String>,
    pub sweeps: BoundedList<String>,
    pub ports: BoundedList<String>,
    pub boundaries: BoundedList<String>,
    pub source_member: Option<String>,
    pub lock_file_present: bool,
    pub results_dir_present: bool,
    pub source_truncated: bool,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactHfssDesign {
    pub name: String,
    pub design_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AbaqusInpSummary, SimFormat, SimparseResult};

    #[test]
    fn summary_caps_lists_and_utf8_text() {
        let values: Vec<_> = (0..20)
            .map(|index| format!("材料-{index}").repeat(80))
            .collect();
        let result = SimparseResult {
            path: None,
            file_name: "large.inp".to_string(),
            format: SimFormat::AbaqusInp,
            ok: true,
            warnings: values.clone(),
            summary: FormatSummary::AbaqusInp(AbaqusInpSummary {
                title: Some("温度".repeat(200)),
                includes: Vec::new(),
                node_count: 0,
                element_count: 0,
                materials: values,
                sections: Vec::new(),
                steps: Vec::new(),
                boundary_keywords: Vec::new(),
                load_keywords: Vec::new(),
                output_keywords: Vec::new(),
                unknown_keywords: Vec::new(),
            }),
        };

        let summary = summarize_result(&result);
        assert_eq!(summary.warnings.total, 20);
        assert_eq!(summary.warnings.sample.len(), SUMMARY_SAMPLE_LIMIT);
        let CompactFormatSummary::AbaqusInp(ref data) = summary.summary else {
            panic!("expected Abaqus summary")
        };
        assert_eq!(data.materials.total, 20);
        assert!(data.materials.truncated);
        assert!(data.title.as_ref().unwrap().len() <= SUMMARY_TEXT_BYTES + '…'.len_utf8());
        assert!(serde_json::to_vec(&summary).unwrap().len() < 16 * 1024);
    }
}
