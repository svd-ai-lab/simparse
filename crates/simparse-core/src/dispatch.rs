use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::parsers::{abaqus, comsol, hfss, inspect_fluent_hdf5};
use crate::{
    FormatSummary, InspectOptions, Result, ScanOptions, SimFormat, SimparseError, SimparseResult,
};

pub fn detect_format(path: &Path) -> Option<SimFormat> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".mph") {
        Some(SimFormat::ComsolMph)
    } else if name.ends_with(".inp") || name.ends_with(".inc") {
        Some(SimFormat::AbaqusInp)
    } else if name.ends_with(".cas.h5") || name.ends_with(".msh.h5") {
        Some(SimFormat::FluentHdf5)
    } else if name.ends_with(".aedt") || name.ends_with(".aedtz") {
        Some(SimFormat::HfssAedt)
    } else {
        None
    }
}

pub fn inspect_path(path: impl AsRef<Path>, options: InspectOptions) -> Result<SimparseResult> {
    let path = path.as_ref();
    let format = options
        .format
        .or_else(|| detect_format(path))
        .ok_or_else(|| SimparseError::UnsupportedFormat(path.display().to_string()))?;

    let summary = match format {
        SimFormat::ComsolMph => FormatSummary::ComsolMph(comsol::inspect_comsol_mph(path)?),
        SimFormat::AbaqusInp => FormatSummary::AbaqusInp(abaqus::inspect_abaqus_inp(path)?),
        SimFormat::FluentHdf5 => FormatSummary::FluentHdf5(inspect_fluent_hdf5(path)?),
        SimFormat::HfssAedt => {
            FormatSummary::HfssAedt(hfss::inspect_hfss_aedt(path, options.max_text_bytes)?)
        }
    };

    Ok(SimparseResult {
        path: options.include_paths.then(|| path.display().to_string()),
        file_name: path
            .file_name()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string()),
        format,
        ok: true,
        warnings: Vec::new(),
        summary,
    })
}

pub fn scan_paths(paths: &[PathBuf], options: ScanOptions) -> Result<Vec<SimparseResult>> {
    let mut out = Vec::new();
    for root in paths {
        if root.is_file() {
            if should_include(root, &options.includes) {
                let mut inspect = options.inspect.clone();
                inspect.include_paths = options.include_paths;
                out.push(inspect_path(root, inspect)?);
            }
            continue;
        }

        let walker = if options.recursive {
            WalkDir::new(root)
        } else {
            WalkDir::new(root).max_depth(1)
        };

        for entry in walker.into_iter().filter_map(std::result::Result::ok) {
            let path = entry.path();
            if path.is_file() && should_include(path, &options.includes) {
                let mut inspect = options.inspect.clone();
                inspect.include_paths = options.include_paths;
                out.push(inspect_path(path, inspect)?);
            }
        }
    }
    Ok(out)
}

fn should_include(path: &Path, patterns: &[String]) -> bool {
    let name = match path.file_name() {
        Some(name) => name.to_string_lossy().to_ascii_lowercase(),
        None => return false,
    };
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim().to_ascii_lowercase();
        if pattern == "*" {
            true
        } else if let Some(suffix) = pattern.strip_prefix("*.") {
            name.ends_with(&format!(".{suffix}"))
        } else if let Some(suffix) = pattern.strip_prefix('*') {
            name.ends_with(suffix)
        } else {
            name == pattern
        }
    })
}
