use std::path::Path;

use crate::{
    AbaqusInpSummary, FlothermBoundary, FlothermEntity, FlothermFloxmlSummary, FlothermGridAxis,
    FlothermSource, FormatSummary, HfssAedtSummary, HfssDesign, IcepakAedtSummary, IcepakBoundary,
    IcepakDesign, IcepakTzrEntry, IcepakTzrSummary, NamedCount, NamedValue, SimparseResult,
};
use simparse_hdf5::{FluentHdf5Summary, Hdf5AttributeInfo, Hdf5DatasetInfo, Hdf5GroupInfo};
use simparse_hdf5::{MechanicalMechdbSummary, MechanicalObjectTypeCount, MechanicalStreamInfo};

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
                "Shallow AEDT section inventory only; component payloads, solved fields, and electromagnetic or thermal-fluid model semantics are not evaluated."
                    .to_string(),
            ],
        ),
        FormatSummary::AnsysMechanical(value) => (
            CompactFormatSummary::AnsysMechanical(compact_mechanical(value)),
            vec![
                "Shallow Mechanical database inventory only; mesh payloads, result arrays, material curves, and solver semantics are not evaluated."
                    .to_string(),
                "Object categories use observed Mechanical database type identifiers and may require vendor-tool confirmation across releases."
                    .to_string(),
            ],
        ),
        FormatSummary::IcepakTzr(value) => (
            CompactFormatSummary::IcepakTzr(compact_icepak_tzr(value)),
            vec![
                "Archive inventory only; Icepak Classic model payloads, mesh, results, and solver semantics are not decoded."
                    .to_string(),
            ],
        ),
        FormatSummary::FlothermFloxml(value) => (
            CompactFormatSummary::FlothermFloxml(compact_flotherm(value)),
            vec![
                "Shallow FloXML structure only; references, geometry validity, mesh generation, result fields, and solver semantics are not evaluated."
                    .to_string(),
                "Proprietary FloTHERM PDML project payloads are not decoded.".to_string(),
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
        product: option_text(value.product.as_deref()),
        version_hint: option_text(value.version_hint.as_deref()),
        designs: bounded_map(&value.designs, compact_hfss_design),
        variables: bounded_texts(&value.variables),
        setups: bounded_texts(&value.setups),
        sweeps: bounded_texts(&value.sweeps),
        ports: bounded_texts(&value.ports),
        boundaries: bounded_texts(&value.boundaries),
        materials: bounded_texts(&value.materials),
        mesh_operations: bounded_texts(&value.mesh_operations),
        icepak: value.icepak.as_ref().map(compact_icepak_aedt),
        source_member: option_text(value.source_member.as_deref()),
        lock_file_present: value.sidecars.lock_file_present,
        results_dir_present: value.sidecars.results_dir_present,
        source_truncated: value.truncated,
    }
}

fn compact_icepak_aedt(value: &IcepakAedtSummary) -> CompactIcepakAedtSummary {
    CompactIcepakAedtSummary {
        designs: bounded_map(&value.designs, compact_icepak_design),
        thermal_boundaries: bounded_map(&value.thermal_boundaries, compact_icepak_boundary),
        monitors: bounded_texts(&value.monitors),
        mesh_regions: bounded_texts(&value.mesh_regions),
    }
}

fn compact_icepak_design(value: &IcepakDesign) -> CompactIcepakDesign {
    CompactIcepakDesign {
        name: bounded_text(&value.name),
        solution_type: option_text(value.solution_type.as_deref()),
        problem_option: option_text(value.problem_option.as_deref()),
        ambient_temperature: option_text(value.ambient_temperature.as_deref()),
        ambient_pressure: option_text(value.ambient_pressure.as_deref()),
        ambient_radiation_temperature: option_text(value.ambient_radiation_temperature.as_deref()),
        default_fluid_material: option_text(value.default_fluid_material.as_deref()),
        default_solid_material: option_text(value.default_solid_material.as_deref()),
        default_surface_material: option_text(value.default_surface_material.as_deref()),
    }
}

fn compact_icepak_boundary(value: &IcepakBoundary) -> CompactIcepakBoundary {
    CompactIcepakBoundary {
        name: bounded_text(&value.name),
        boundary_type: bounded_text(&value.boundary_type),
        thermal_condition: option_text(value.thermal_condition.as_deref()),
        total_power: option_text(value.total_power.as_deref()),
        temperature: option_text(value.temperature.as_deref()),
    }
}

fn compact_icepak_tzr(value: &IcepakTzrSummary) -> CompactIcepakTzrSummary {
    CompactIcepakTzrSummary {
        project_name: option_text(value.project_name.as_deref()),
        gzip_compressed: value.gzip_compressed,
        entry_count: value.entry_count,
        file_count: value.file_count,
        directory_count: value.directory_count,
        uncompressed_bytes: value.uncompressed_bytes,
        job_file_present: value.job_file_present,
        model_file_present: value.model_file_present,
        entries: bounded_map(&value.entries, compact_icepak_tzr_entry),
        source_truncated: value.truncated,
    }
}

fn compact_icepak_tzr_entry(value: &IcepakTzrEntry) -> CompactIcepakTzrEntry {
    CompactIcepakTzrEntry {
        name: bounded_text(&value.name),
        entry_type: bounded_text(&value.entry_type),
        bytes: value.bytes,
    }
}

fn compact_flotherm(value: &FlothermFloxmlSummary) -> CompactFlothermFloxmlSummary {
    CompactFlothermFloxmlSummary {
        root: bounded_text(&value.root),
        name: option_text(value.name.as_deref()),
        solution: option_text(value.solution.as_deref()),
        dimensionality: option_text(value.dimensionality.as_deref()),
        transient: value.transient,
        radiation: option_text(value.radiation.as_deref()),
        turbulence_type: option_text(value.turbulence_type.as_deref()),
        gravity_direction: option_text(value.gravity_direction.as_deref()),
        ambient_temperature: option_text(value.ambient_temperature.as_deref()),
        datum_pressure: option_text(value.datum_pressure.as_deref()),
        outer_iterations: option_text(value.outer_iterations.as_deref()),
        grid: bounded_map(&value.grid, compact_flotherm_grid),
        attribute_type_counts: bounded_map(&value.attribute_type_counts, compact_named_count),
        attributes: bounded_map(&value.attributes, compact_flotherm_entity),
        geometry_type_counts: bounded_map(&value.geometry_type_counts, compact_named_count),
        geometry: bounded_map(&value.geometry, compact_flotherm_entity),
        sources: bounded_map(&value.sources, compact_flotherm_source),
        solution_domain: value.solution_domain.as_ref().map(|domain| {
            CompactFlothermSolutionDomain {
                fluid: option_text(domain.fluid.as_deref()),
                boundaries: bounded_map(&domain.boundaries, compact_flotherm_boundary),
            }
        }),
        source_truncated: value.truncated,
    }
}

fn compact_named_count(value: &NamedCount) -> CompactNamedCount {
    CompactNamedCount {
        name: bounded_text(&value.name),
        count: value.count,
    }
}

fn compact_flotherm_entity(value: &FlothermEntity) -> CompactFlothermEntity {
    CompactFlothermEntity {
        kind: bounded_text(&value.kind),
        name: bounded_text(&value.name),
    }
}

fn compact_flotherm_source(value: &FlothermSource) -> CompactFlothermSource {
    CompactFlothermSource {
        name: bounded_text(&value.name),
        powers: bounded_texts(&value.powers),
    }
}

fn compact_flotherm_grid(value: &FlothermGridAxis) -> CompactFlothermGridAxis {
    CompactFlothermGridAxis {
        axis: bounded_text(&value.axis),
        grid_type: option_text(value.grid_type.as_deref()),
        min_size: option_text(value.min_size.as_deref()),
        max_size: option_text(value.max_size.as_deref()),
    }
}

fn compact_flotherm_boundary(value: &FlothermBoundary) -> CompactFlothermBoundary {
    CompactFlothermBoundary {
        face: bounded_text(&value.face),
        kind: bounded_text(&value.kind),
        value: bounded_text(&value.value),
    }
}

fn compact_mechanical(value: &MechanicalMechdbSummary) -> CompactMechanicalMechdbSummary {
    CompactMechanicalMechdbSummary {
        mechanical_version: option_text(value.mechanical_version.as_deref()),
        database_format_version: option_text(value.database_format_version.as_deref()),
        release_kind: option_text(value.release_kind.as_deref()),
        object_count: value.object_count,
        object_type_counts: bounded_map(&value.object_type_counts, compact_mechanical_type_count),
        analyses: bounded_texts(&value.analyses),
        bodies: bounded_texts(&value.bodies),
        materials: bounded_texts(&value.materials),
        contacts: bounded_texts(&value.contacts),
        loads_and_conditions: bounded_texts(&value.loads_and_conditions),
        results: bounded_texts(&value.results),
        geometry_sources: bounded_texts(&value.geometry_sources),
        streams: bounded_map(&value.streams, compact_mechanical_stream),
        lock_file_present: value.sidecars.lock_file_present,
        project_files_dir_present: value.sidecars.project_files_dir_present,
        content_truncated: value.content_truncated,
    }
}

fn compact_mechanical_type_count(
    value: &MechanicalObjectTypeCount,
) -> CompactMechanicalObjectTypeCount {
    CompactMechanicalObjectTypeCount {
        type_id: value.type_id,
        count: value.count,
    }
}

fn compact_mechanical_stream(value: &MechanicalStreamInfo) -> CompactMechanicalStreamInfo {
    CompactMechanicalStreamInfo {
        name: bounded_text(&value.name),
        used_bytes: value.used_bytes,
    }
}

fn compact_hfss_design(value: &HfssDesign) -> CompactHfssDesign {
    CompactHfssDesign {
        name: bounded_text(&value.name),
        design_type: option_text(value.design_type.as_deref()),
        solution_type: option_text(value.solution_type.as_deref()),
        is_solved: value.is_solved,
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
    AnsysMechanical(CompactMechanicalMechdbSummary),
    IcepakTzr(CompactIcepakTzrSummary),
    FlothermFloxml(CompactFlothermFloxmlSummary),
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
    pub product: Option<String>,
    pub version_hint: Option<String>,
    pub designs: BoundedList<CompactHfssDesign>,
    pub variables: BoundedList<String>,
    pub setups: BoundedList<String>,
    pub sweeps: BoundedList<String>,
    pub ports: BoundedList<String>,
    pub boundaries: BoundedList<String>,
    pub materials: BoundedList<String>,
    pub mesh_operations: BoundedList<String>,
    pub icepak: Option<CompactIcepakAedtSummary>,
    pub source_member: Option<String>,
    pub lock_file_present: bool,
    pub results_dir_present: bool,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactIcepakAedtSummary {
    pub designs: BoundedList<CompactIcepakDesign>,
    pub thermal_boundaries: BoundedList<CompactIcepakBoundary>,
    pub monitors: BoundedList<String>,
    pub mesh_regions: BoundedList<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactIcepakDesign {
    pub name: String,
    pub solution_type: Option<String>,
    pub problem_option: Option<String>,
    pub ambient_temperature: Option<String>,
    pub ambient_pressure: Option<String>,
    pub ambient_radiation_temperature: Option<String>,
    pub default_fluid_material: Option<String>,
    pub default_solid_material: Option<String>,
    pub default_surface_material: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactIcepakBoundary {
    pub name: String,
    pub boundary_type: String,
    pub thermal_condition: Option<String>,
    pub total_power: Option<String>,
    pub temperature: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactIcepakTzrSummary {
    pub project_name: Option<String>,
    pub gzip_compressed: bool,
    pub entry_count: usize,
    pub file_count: usize,
    pub directory_count: usize,
    pub uncompressed_bytes: u64,
    pub job_file_present: bool,
    pub model_file_present: bool,
    pub entries: BoundedList<CompactIcepakTzrEntry>,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactIcepakTzrEntry {
    pub name: String,
    pub entry_type: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFlothermFloxmlSummary {
    pub root: String,
    pub name: Option<String>,
    pub solution: Option<String>,
    pub dimensionality: Option<String>,
    pub transient: Option<bool>,
    pub radiation: Option<String>,
    pub turbulence_type: Option<String>,
    pub gravity_direction: Option<String>,
    pub ambient_temperature: Option<String>,
    pub datum_pressure: Option<String>,
    pub outer_iterations: Option<String>,
    pub grid: BoundedList<CompactFlothermGridAxis>,
    pub attribute_type_counts: BoundedList<CompactNamedCount>,
    pub attributes: BoundedList<CompactFlothermEntity>,
    pub geometry_type_counts: BoundedList<CompactNamedCount>,
    pub geometry: BoundedList<CompactFlothermEntity>,
    pub sources: BoundedList<CompactFlothermSource>,
    pub solution_domain: Option<CompactFlothermSolutionDomain>,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactNamedCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFlothermEntity {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFlothermSource {
    pub name: String,
    pub powers: BoundedList<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFlothermGridAxis {
    pub axis: String,
    pub grid_type: Option<String>,
    pub min_size: Option<String>,
    pub max_size: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFlothermSolutionDomain {
    pub fluid: Option<String>,
    pub boundaries: BoundedList<CompactFlothermBoundary>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactFlothermBoundary {
    pub face: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CompactMechanicalMechdbSummary {
    pub mechanical_version: Option<String>,
    pub database_format_version: Option<String>,
    pub release_kind: Option<String>,
    pub object_count: usize,
    pub object_type_counts: BoundedList<CompactMechanicalObjectTypeCount>,
    pub analyses: BoundedList<String>,
    pub bodies: BoundedList<String>,
    pub materials: BoundedList<String>,
    pub contacts: BoundedList<String>,
    pub loads_and_conditions: BoundedList<String>,
    pub results: BoundedList<String>,
    pub geometry_sources: BoundedList<String>,
    pub streams: BoundedList<CompactMechanicalStreamInfo>,
    pub lock_file_present: bool,
    pub project_files_dir_present: bool,
    pub content_truncated: bool,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactMechanicalObjectTypeCount {
    pub type_id: u32,
    pub count: usize,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactMechanicalStreamInfo {
    pub name: String,
    pub used_bytes: u64,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct CompactHfssDesign {
    pub name: String,
    pub design_type: Option<String>,
    pub solution_type: Option<String>,
    pub is_solved: Option<bool>,
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
