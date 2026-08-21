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
