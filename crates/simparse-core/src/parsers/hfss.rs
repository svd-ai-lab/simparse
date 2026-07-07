use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

use crate::{HfssAedtSummary, HfssDesign, HfssSidecars, Result, SimparseError};

pub fn inspect_hfss_aedt(path: &Path, max_text_bytes: usize) -> Result<HfssAedtSummary> {
    let (text, source_member, truncated) = if path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".aedtz"))
        .unwrap_or(false)
    {
        read_aedtz(path, max_text_bytes)?
    } else {
        read_text_file(path, max_text_bytes)?
    };
    let sidecars = sidecars(path);
    Ok(parse_aedt_text(&text, source_member, truncated, sidecars))
}

fn read_text_file(path: &Path, max_text_bytes: usize) -> Result<(String, Option<String>, bool)> {
    let bytes = std::fs::read(path)?;
    let truncated = bytes.len() > max_text_bytes;
    let slice = &bytes[..bytes.len().min(max_text_bytes)];
    Ok((String::from_utf8_lossy(slice).into_owned(), None, truncated))
}

fn read_aedtz(path: &Path, max_text_bytes: usize) -> Result<(String, Option<String>, bool)> {
    let file = std::fs::File::open(path)?;
    let mut zip = ZipArchive::new(file)?;

    let mut selected = None;
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        if entry.name().to_ascii_lowercase().ends_with(".aedt") {
            selected = Some(index);
            break;
        }
    }
    let index = selected.ok_or_else(|| {
        SimparseError::Parse("AEDTZ archive did not contain an .aedt member".into())
    })?;

    let mut entry = zip.by_index(index)?;
    let name = entry.name().to_string();
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(max_text_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > max_text_bytes;
    bytes.truncate(max_text_bytes);
    Ok((
        String::from_utf8_lossy(&bytes).into_owned(),
        Some(name),
        truncated,
    ))
}

fn parse_aedt_text(
    text: &str,
    source_member: Option<String>,
    truncated: bool,
    sidecars: HfssSidecars,
) -> HfssAedtSummary {
    let mut project_name = None;
    let mut version_hint = None;
    let mut designs = Vec::new();
    let mut variables = Vec::new();
    let mut setups = Vec::new();
    let mut sweeps = Vec::new();
    let mut ports = Vec::new();
    let mut boundaries = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if project_name.is_none() {
            project_name = extract_assignment(trimmed, "ProjectName");
        }
        if version_hint.is_none() {
            version_hint = extract_assignment(trimmed, "Version")
                .or_else(|| extract_assignment(trimmed, "VersionId"))
                .or_else(|| extract_assignment(trimmed, "AedtVersion"));
        }
        if let Some(name) = extract_assignment(trimmed, "DesignName")
            .or_else(|| extract_begin_name(trimmed, "HFSSModel"))
        {
            let design_type = if trimmed.to_ascii_lowercase().contains("hfss") {
                Some("HFSS".to_string())
            } else {
                extract_assignment(trimmed, "DesignType")
            };
            designs.push(HfssDesign { name, design_type });
        }
        collect_named_hint(trimmed, &mut variables, &["Variable", "VariableProp"]);
        collect_named_hint(trimmed, &mut setups, &["Setup", "AnalysisSetup"]);
        collect_named_hint(trimmed, &mut sweeps, &["Sweep", "FrequencySweep"]);
        collect_named_hint(trimmed, &mut ports, &["Port", "Terminal"]);
        collect_named_hint(trimmed, &mut boundaries, &["Boundary", "Boundaries"]);
    }

    sort_dedup_designs(&mut designs);
    sort_dedup(&mut variables);
    sort_dedup(&mut setups);
    sort_dedup(&mut sweeps);
    sort_dedup(&mut ports);
    sort_dedup(&mut boundaries);

    HfssAedtSummary {
        project_name,
        version_hint,
        designs,
        variables,
        setups,
        sweeps,
        ports,
        boundaries,
        source_member,
        sidecars,
        truncated,
    }
}

fn sidecars(path: &Path) -> HfssSidecars {
    let lock_file = std::path::PathBuf::from(format!("{}.lock", path.display()));
    let results_dir = std::path::PathBuf::from(format!("{}results", path.display()));
    HfssSidecars {
        lock_file_present: lock_file.exists(),
        results_dir_present: results_dir.is_dir(),
    }
}

fn extract_assignment(line: &str, key: &str) -> Option<String> {
    for pattern in [format!("{key}="), format!("{key}:=")] {
        if let Some(rest) = line.split_once(&pattern).map(|(_, rest)| rest) {
            return extract_quoted_or_word(rest);
        }
    }
    None
}

fn extract_begin_name(line: &str, key: &str) -> Option<String> {
    if !line.contains("$begin") || !line.contains(key) {
        return None;
    }
    extract_quoted_or_word(line)
}

fn collect_named_hint(line: &str, out: &mut Vec<String>, labels: &[&str]) {
    let lower = line.to_ascii_lowercase();
    if labels
        .iter()
        .any(|label| lower.contains(&label.to_ascii_lowercase()))
        && let Some(value) = extract_quoted_or_word(line)
    {
        out.push(value);
    }
}

fn extract_quoted_or_word(text: &str) -> Option<String> {
    let text = text.trim().trim_start_matches('(').trim_start_matches(',');
    if let Some(start) = text.find('\'') {
        let tail = &text[start + 1..];
        if let Some(end) = tail.find('\'') {
            return Some(tail[..end].to_string());
        }
    }
    if let Some(start) = text.find('"') {
        let tail = &text[start + 1..];
        if let Some(end) = tail.find('"') {
            return Some(tail[..end].to_string());
        }
    }
    text.split([',', ' ', ')'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .map(ToOwned::to_owned)
}

fn sort_dedup(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

fn sort_dedup_designs(values: &mut Vec<HfssDesign>) {
    values.sort_by(|a, b| a.name.cmp(&b.name));
    values.dedup_by(|a, b| a.name == b.name);
}
