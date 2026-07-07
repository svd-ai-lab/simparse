use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::{AbaqusInpSummary, Result};

pub fn inspect_abaqus_inp(path: &Path) -> Result<AbaqusInpSummary> {
    let mut summary = AbaqusInpSummary {
        title: None,
        includes: Vec::new(),
        node_count: 0,
        element_count: 0,
        materials: Vec::new(),
        sections: Vec::new(),
        steps: Vec::new(),
        boundary_keywords: Vec::new(),
        load_keywords: Vec::new(),
        output_keywords: Vec::new(),
        unknown_keywords: Vec::new(),
    };
    let mut visited = HashSet::new();
    inspect_deck(
        path,
        path.parent().unwrap_or_else(|| Path::new("")),
        &mut visited,
        &mut summary,
    )?;
    sort_dedup(&mut summary.materials);
    sort_dedup(&mut summary.sections);
    sort_dedup(&mut summary.steps);
    sort_dedup(&mut summary.boundary_keywords);
    sort_dedup(&mut summary.load_keywords);
    sort_dedup(&mut summary.output_keywords);
    sort_dedup(&mut summary.unknown_keywords);
    Ok(summary)
}

fn inspect_deck(
    path: &Path,
    base: &Path,
    visited: &mut HashSet<PathBuf>,
    summary: &mut AbaqusInpSummary,
) -> Result<()> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical) {
        return Ok(());
    }

    let text = std::fs::read_to_string(path)?;
    let mut current_keyword = String::new();
    let mut title_pending = false;
    let mut in_step = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("**") {
            continue;
        }

        if let Some(rest) = line.strip_prefix('*') {
            title_pending = false;
            let (keyword, params) = parse_keyword(rest);
            current_keyword = keyword.clone();

            match keyword.as_str() {
                "HEADING" => title_pending = true,
                "INCLUDE" => {
                    if let Some(include) = params.get("INPUT") {
                        let rel = PathBuf::from(include);
                        summary.includes.push(rel.clone());
                        let next = if rel.is_absolute() {
                            rel
                        } else {
                            base.join(&rel)
                        };
                        if next.is_file() {
                            let next_base = next.parent().unwrap_or(base).to_path_buf();
                            inspect_deck(&next, &next_base, visited, summary)?;
                        }
                    }
                }
                "MATERIAL" => {
                    if let Some(name) = params.get("NAME") {
                        summary.materials.push(name.clone());
                    }
                }
                "STEP" => in_step = true,
                "END STEP" => in_step = false,
                "BOUNDARY" => summary.boundary_keywords.push(keyword),
                "CLOAD" | "DLOAD" | "DSLOAD" | "TEMPERATURE" | "CFLUX" | "DFLUX" => {
                    summary.load_keywords.push(keyword)
                }
                key if key.contains("OUTPUT")
                    || key.ends_with("PRINT")
                    || key.ends_with("FILE") =>
                {
                    summary.output_keywords.push(keyword)
                }
                key if key.contains("SECTION") => {
                    let label = params
                        .get("MATERIAL")
                        .map(|m| format!("{key}:{m}"))
                        .unwrap_or_else(|| key.to_string());
                    summary.sections.push(label);
                }
                key if in_step
                    && matches!(key, "STATIC" | "DYNAMIC" | "HEAT TRANSFER" | "FREQUENCY") =>
                {
                    summary.steps.push(key.to_string());
                }
                key if !known_keyword(key) => summary.unknown_keywords.push(key.to_string()),
                _ => {}
            }
            continue;
        }

        if title_pending && summary.title.is_none() {
            summary.title = Some(line.to_string());
            title_pending = false;
        }

        match current_keyword.as_str() {
            "NODE" => summary.node_count += 1,
            "ELEMENT" => summary.element_count += 1,
            _ => {}
        }
    }

    Ok(())
}

fn parse_keyword(rest: &str) -> (String, std::collections::BTreeMap<String, String>) {
    let mut parts = rest.split(',').map(str::trim).filter(|p| !p.is_empty());
    let keyword = parts.next().unwrap_or("").to_ascii_uppercase();
    let mut params = std::collections::BTreeMap::new();
    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            params.insert(key.trim().to_ascii_uppercase(), value.trim().to_string());
        } else {
            params.insert(part.to_ascii_uppercase(), String::new());
        }
    }
    (keyword, params)
}

fn known_keyword(keyword: &str) -> bool {
    const KNOWN: &[&str] = &[
        "HEADING",
        "NODE",
        "ELEMENT",
        "MATERIAL",
        "ELASTIC",
        "DENSITY",
        "SOLID SECTION",
        "SHELL SECTION",
        "BEAM SECTION",
        "NSET",
        "ELSET",
        "STEP",
        "END STEP",
        "STATIC",
        "DYNAMIC",
        "HEAT TRANSFER",
        "BOUNDARY",
        "CLOAD",
        "DLOAD",
        "DSLOAD",
        "OUTPUT",
        "NODE OUTPUT",
        "ELEMENT OUTPUT",
        "NODE PRINT",
        "NODE FILE",
        "EL PRINT",
        "EL FILE",
        "INCLUDE",
    ];
    KNOWN.contains(&keyword) || keyword.contains("SECTION")
}

fn sort_dedup(values: &mut Vec<String>) {
    let set: BTreeSet<_> = values.drain(..).collect();
    values.extend(set);
}
