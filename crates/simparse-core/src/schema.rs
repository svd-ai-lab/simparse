use std::path::PathBuf;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SimFormat {
    ComsolMph,
    AbaqusInp,
    FluentHdf5,
    HfssAedt,
    AnsysMechanical,
    IcepakTzr,
    FlothermFloxml,
}

impl FromStr for SimFormat {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Err("auto is not a concrete format".into()),
            "comsol-mph" | "mph" => Ok(Self::ComsolMph),
            "abaqus-inp" | "inp" | "inc" => Ok(Self::AbaqusInp),
            "fluent-hdf5" | "fluent-h5" | "cas.h5" | "msh.h5" => Ok(Self::FluentHdf5),
            "hfss-aedt" | "aedt" | "aedtz" => Ok(Self::HfssAedt),
            "ansys-mechanical" | "mechanical" | "mechdb" | "mechdat" => Ok(Self::AnsysMechanical),
            "icepak-tzr" | "icepak" | "tzr" => Ok(Self::IcepakTzr),
            "flotherm-floxml" | "flotherm" | "floxml" => Ok(Self::FlothermFloxml),
            other => Err(format!("unknown format: {other}")),
        }
    }
}

impl std::fmt::Display for SimFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::ComsolMph => "comsol-mph",
            Self::AbaqusInp => "abaqus-inp",
            Self::FluentHdf5 => "fluent-hdf5",
            Self::HfssAedt => "hfss-aedt",
            Self::AnsysMechanical => "ansys-mechanical",
            Self::IcepakTzr => "icepak-tzr",
            Self::FlothermFloxml => "flotherm-floxml",
        };
        f.write_str(text)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectOptions {
    pub format: Option<SimFormat>,
    pub include_paths: bool,
    pub max_text_bytes: usize,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            format: None,
            include_paths: false,
            max_text_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanOptions {
    pub recursive: bool,
    pub include_paths: bool,
    pub includes: Vec<String>,
    pub inspect: InspectOptions,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            include_paths: false,
            includes: vec![
                "*.mph".into(),
                "*.inp".into(),
                "*.inc".into(),
                "*.cas.h5".into(),
                "*.msh.h5".into(),
                "*.aedt".into(),
                "*.aedtz".into(),
                "*.mechdb".into(),
                "*.mechdat".into(),
                "*.tzr".into(),
                "*.floxml".into(),
                "*.xml".into(),
            ],
            inspect: InspectOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SimparseResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub file_name: String,
    pub format: SimFormat,
    pub ok: bool,
    pub warnings: Vec<String>,
    pub summary: FormatSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum FormatSummary {
    ComsolMph(ComsolMphSummary),
    AbaqusInp(AbaqusInpSummary),
    FluentHdf5(simparse_hdf5::FluentHdf5Summary),
    HfssAedt(HfssAedtSummary),
    AnsysMechanical(simparse_hdf5::MechanicalMechdbSummary),
    IcepakTzr(IcepakTzrSummary),
    FlothermFloxml(FlothermFloxmlSummary),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ComsolMphSummary {
    pub schema: Option<String>,
    pub saved_in: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub comsol_version: Option<String>,
    pub node_type: Option<String>,
    pub is_runnable: Option<bool>,
    pub parameters: Vec<NamedValue>,
    pub physics_tags: Vec<String>,
    pub study_tags: Vec<String>,
    pub material_tags: Vec<String>,
    pub used_licenses: Vec<String>,
    pub size_breakdown: Vec<SizeBucket>,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NamedValue {
    pub name: String,
    pub value: Option<String>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SizeBucket {
    pub bucket: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AbaqusInpSummary {
    pub title: Option<String>,
    pub includes: Vec<PathBuf>,
    pub node_count: usize,
    pub element_count: usize,
    pub materials: Vec<String>,
    pub sections: Vec<String>,
    pub steps: Vec<String>,
    pub boundary_keywords: Vec<String>,
    pub load_keywords: Vec<String>,
    pub output_keywords: Vec<String>,
    pub unknown_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct HfssAedtSummary {
    pub project_name: Option<String>,
    pub product: Option<String>,
    pub version_hint: Option<String>,
    pub designs: Vec<HfssDesign>,
    pub variables: Vec<String>,
    pub setups: Vec<String>,
    pub sweeps: Vec<String>,
    pub ports: Vec<String>,
    pub boundaries: Vec<String>,
    pub materials: Vec<String>,
    pub mesh_operations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icepak: Option<IcepakAedtSummary>,
    pub source_member: Option<String>,
    pub sidecars: HfssSidecars,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct HfssDesign {
    pub name: String,
    pub design_type: Option<String>,
    pub solution_type: Option<String>,
    pub is_solved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct HfssSidecars {
    pub lock_file_present: bool,
    pub results_dir_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IcepakAedtSummary {
    pub designs: Vec<IcepakDesign>,
    pub thermal_boundaries: Vec<IcepakBoundary>,
    pub monitors: Vec<String>,
    pub mesh_regions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IcepakDesign {
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IcepakBoundary {
    pub name: String,
    pub boundary_type: String,
    pub thermal_condition: Option<String>,
    pub total_power: Option<String>,
    pub temperature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IcepakTzrSummary {
    pub project_name: Option<String>,
    pub gzip_compressed: bool,
    pub entry_count: usize,
    pub file_count: usize,
    pub directory_count: usize,
    pub uncompressed_bytes: u64,
    pub job_file_present: bool,
    pub model_file_present: bool,
    pub entries: Vec<IcepakTzrEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IcepakTzrEntry {
    pub name: String,
    pub entry_type: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FlothermFloxmlSummary {
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
    pub grid: Vec<FlothermGridAxis>,
    pub attribute_type_counts: Vec<NamedCount>,
    pub attributes: Vec<FlothermEntity>,
    pub geometry_type_counts: Vec<NamedCount>,
    pub geometry: Vec<FlothermEntity>,
    pub sources: Vec<FlothermSource>,
    pub solution_domain: Option<FlothermSolutionDomain>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NamedCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FlothermEntity {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FlothermSource {
    pub name: String,
    pub powers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FlothermGridAxis {
    pub axis: String,
    pub grid_type: Option<String>,
    pub min_size: Option<String>,
    pub max_size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FlothermSolutionDomain {
    pub fluid: Option<String>,
    pub boundaries: Vec<FlothermBoundary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FlothermBoundary {
    pub face: String,
    pub kind: String,
    pub value: String,
}
