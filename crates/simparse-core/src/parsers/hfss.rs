use std::io::{BufRead, BufReader};
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
    let file = std::fs::File::open(path)?;
    let (text, truncated) = read_compacted_aedt(BufReader::new(file), max_text_bytes)?;
    Ok((text, None, truncated))
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

    let entry = zip.by_index(index)?;
    let name = entry.name().to_string();
    let (text, truncated) = read_compacted_aedt(BufReader::new(entry), max_text_bytes)?;
    Ok((text, Some(name), truncated))
}

fn read_compacted_aedt(mut reader: impl BufRead, max_text_bytes: usize) -> Result<(String, bool)> {
    let mut output = String::new();
    let mut bytes = Vec::new();
    let mut truncated = false;
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&bytes);
        let trimmed = line.trim();
        if !is_relevant_aedt_line(trimmed) {
            continue;
        }
        if output.len() + trimmed.len() + 1 > max_text_bytes {
            truncated = true;
            continue;
        }
        output.push_str(trimmed);
        output.push('\n');
    }
    Ok((output, truncated))
}

fn is_relevant_aedt_line(line: &str) -> bool {
    line.starts_with("$begin ")
        || line.starts_with("$end ")
        || line.starts_with("Version(")
        || line.starts_with("VariableProp(")
        || line.starts_with("PostProcessingVariableProp(")
        || [
            "ProjectName",
            "Product",
            "Name",
            "SolutionType",
            "DesignName",
            "Factory",
            "IsSolved",
            "BoundType",
        ]
        .iter()
        .any(|key| {
            line.strip_prefix(key)
                .is_some_and(|rest| rest.starts_with('=') || rest.starts_with(":="))
        })
}

fn parse_aedt_text(
    text: &str,
    source_member: Option<String>,
    truncated: bool,
    sidecars: HfssSidecars,
) -> HfssAedtSummary {
    let mut project_name = None;
    let mut product = None;
    let mut version_hint = None;
    let mut designs = Vec::new();
    let mut variables = Vec::new();
    let mut setups = Vec::new();
    let mut sweeps = Vec::new();
    let mut ports = Vec::new();
    let mut boundaries = Vec::new();
    let mut materials = Vec::new();
    let mut mesh_operations = Vec::new();
    let mut stack = Vec::new();
    let mut inside_project = false;
    let mut pending_models = Vec::new();
    let mut pending_design_info = Vec::new();
    let mut pending_boundaries = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(section) = extract_section_marker(trimmed, "$begin") {
            if !inside_project {
                if matches!(section.as_str(), "AnsoftProject" | "ProjectPreview") {
                    inside_project = true;
                    stack.push(section);
                }
                continue;
            }

            let parent = stack.last().map(String::as_str);
            let depth = stack.len() + 1;
            if parent == Some("AnsoftProject") && is_design_model_section(&section) {
                pending_models.push(PendingModel {
                    depth,
                    section: section.clone(),
                    name: None,
                    solution_type: None,
                });
            }
            if section == "DesignInfo" {
                pending_design_info.push(PendingDesignInfo {
                    depth,
                    name: None,
                    factory: None,
                    is_solved: None,
                });
            }
            if parent == Some("SolveSetups") && !is_admin_section(&section) {
                setups.push(section.clone());
            }
            if parent == Some("Sweeps")
                && stack.iter().any(|item| item == "SolveSetups")
                && !is_admin_section(&section)
            {
                sweeps.push(section.clone());
            }
            if matches!(
                parent,
                Some("Boundaries" | "BoundariesData" | "ExcitationsData")
            ) && !is_boundary_admin_section(&section)
            {
                pending_boundaries.push(PendingBoundary {
                    depth,
                    name: section.clone(),
                    boundary_type: None,
                });
            }
            if parent == Some("MeshOperations") && !is_admin_section(&section) {
                mesh_operations.push(section.clone());
            }
            if parent == Some("Materials")
                && stack.iter().any(|item| item == "Definitions")
                && !is_admin_section(&section)
            {
                materials.push(section.clone());
            }

            stack.push(section);
            continue;
        }

        if !inside_project {
            continue;
        }

        if let Some(section) = extract_section_marker(trimmed, "$end") {
            let depth = stack.len();
            if let Some(index) = pending_models
                .iter()
                .rposition(|pending| pending.depth == depth && pending.section == section)
            {
                let pending = pending_models.remove(index);
                if let Some(name) = pending.name {
                    merge_design(
                        &mut designs,
                        HfssDesign {
                            name,
                            design_type: Some(model_section_type(&pending.section)),
                            solution_type: pending.solution_type,
                            is_solved: None,
                        },
                    );
                }
            }
            if section == "DesignInfo"
                && let Some(index) = pending_design_info
                    .iter()
                    .rposition(|pending| pending.depth == depth)
            {
                let pending = pending_design_info.remove(index);
                if let Some(name) = pending.name {
                    merge_design(
                        &mut designs,
                        HfssDesign {
                            name,
                            design_type: pending.factory,
                            solution_type: None,
                            is_solved: pending.is_solved,
                        },
                    );
                }
            }
            if let Some(index) = pending_boundaries
                .iter()
                .rposition(|pending| pending.depth == depth && pending.name == section)
            {
                let pending = pending_boundaries.remove(index);
                if let Some(boundary_type) = pending.boundary_type {
                    if boundary_type.to_ascii_lowercase().contains("port")
                        || boundary_type.to_ascii_lowercase().contains("terminal")
                    {
                        ports.push(pending.name);
                    } else {
                        boundaries.push(pending.name);
                    }
                }
            }

            stack.pop();
            if matches!(section.as_str(), "AnsoftProject" | "ProjectPreview") {
                inside_project = false;
            }
            continue;
        }

        if stack.len() == 1 {
            project_name = project_name.or_else(|| extract_assignment(trimmed, "ProjectName"));
            product = product.or_else(|| extract_assignment(trimmed, "Product"));
        }
        if stack.last().map(String::as_str) == Some("Desktop") && version_hint.is_none() {
            version_hint = extract_version_call(trimmed);
        }
        if let Some(model) = pending_models.last_mut()
            && model.depth == stack.len()
        {
            model.name = model
                .name
                .take()
                .or_else(|| extract_assignment(trimmed, "Name"));
            model.solution_type = model
                .solution_type
                .take()
                .or_else(|| extract_assignment(trimmed, "SolutionType"));
        }
        if let Some(info) = pending_design_info.last_mut()
            && info.depth == stack.len()
        {
            info.name = info
                .name
                .take()
                .or_else(|| extract_assignment(trimmed, "DesignName"));
            info.factory = info
                .factory
                .take()
                .or_else(|| extract_assignment(trimmed, "Factory"));
            info.is_solved = info
                .is_solved
                .or_else(|| extract_bool_assignment(trimmed, "IsSolved"));
        }
        if let Some(boundary) = pending_boundaries.last_mut()
            && boundary.depth == stack.len()
        {
            boundary.boundary_type = boundary
                .boundary_type
                .take()
                .or_else(|| extract_assignment(trimmed, "BoundType"));
        }
        if let Some(name) = extract_first_quoted_call(trimmed, "VariableProp")
            .or_else(|| extract_first_quoted_call(trimmed, "PostProcessingVariableProp"))
        {
            variables.push(name);
        }
    }

    sort_dedup_designs(&mut designs);
    sort_dedup(&mut variables);
    sort_dedup(&mut setups);
    sort_dedup(&mut sweeps);
    sort_dedup(&mut ports);
    sort_dedup(&mut boundaries);
    sort_dedup(&mut materials);
    sort_dedup(&mut mesh_operations);

    HfssAedtSummary {
        project_name,
        product,
        version_hint,
        designs,
        variables,
        setups,
        sweeps,
        ports,
        boundaries,
        materials,
        mesh_operations,
        source_member,
        sidecars,
        truncated,
    }
}

#[derive(Debug)]
struct PendingModel {
    depth: usize,
    section: String,
    name: Option<String>,
    solution_type: Option<String>,
}

#[derive(Debug)]
struct PendingDesignInfo {
    depth: usize,
    name: Option<String>,
    factory: Option<String>,
    is_solved: Option<bool>,
}

#[derive(Debug)]
struct PendingBoundary {
    depth: usize,
    name: String,
    boundary_type: Option<String>,
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
    let (left, right) = line.split_once('=')?;
    let left = left.trim().trim_matches('\'');
    if left != key && left.strip_suffix(':') != Some(key) {
        return None;
    }
    extract_quoted_or_word(right)
}

fn extract_bool_assignment(line: &str, key: &str) -> Option<bool> {
    extract_assignment(line, key).and_then(|value| match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    })
}

fn extract_section_marker(line: &str, marker: &str) -> Option<String> {
    let rest = line.strip_prefix(marker)?.trim_start();
    if !rest.starts_with('\'') {
        return None;
    }
    extract_quoted_or_word(rest)
}

fn extract_version_call(line: &str) -> Option<String> {
    let args = line.strip_prefix("Version(")?.strip_suffix(')')?;
    let mut values = args.split(',').map(str::trim);
    let major = values.next()?.parse::<u32>().ok()?;
    let minor = values.next()?.parse::<u32>().ok()?;
    Some(format!("{major}.{minor}"))
}

fn extract_first_quoted_call(line: &str, call: &str) -> Option<String> {
    let rest = line.strip_prefix(call)?.strip_prefix('(')?;
    extract_quoted_or_word(rest)
}

fn is_design_model_section(section: &str) -> bool {
    section.ends_with("Model")
        && !matches!(
            section,
            "SubModel" | "UserDefinedModel" | "OperandUserDefinedModel"
        )
}

fn model_section_type(section: &str) -> String {
    section
        .strip_suffix("Model")
        .filter(|value| !value.is_empty())
        .unwrap_or(section)
        .to_string()
}

fn is_admin_section(section: &str) -> bool {
    matches!(
        section,
        "Data" | "Properties" | "NextUniqueID" | "MoveBackwards"
    )
}

fn is_boundary_admin_section(section: &str) -> bool {
    is_admin_section(section)
        || matches!(
            section,
            "BoundariesDesc"
                | "BoundariesIDMap"
                | "BoundariesData"
                | "BoundariesInstData"
                | "ExcitationsDesc"
                | "ExcitationsIDMap"
                | "ExcitationsData"
                | "ExcitationsInstData"
        )
}

fn merge_design(designs: &mut Vec<HfssDesign>, incoming: HfssDesign) {
    if let Some(existing) = designs.iter_mut().find(|item| item.name == incoming.name) {
        if incoming.design_type.is_some() {
            existing.design_type = incoming.design_type;
        }
        if incoming.solution_type.is_some() {
            existing.solution_type = incoming.solution_type;
        }
        if incoming.is_solved.is_some() {
            existing.is_solved = incoming.is_solved;
        }
    } else {
        designs.push(incoming);
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
