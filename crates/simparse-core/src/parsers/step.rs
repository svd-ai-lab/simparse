//! Bounded Part 21 declaration inventory, without a CAD kernel.
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::{NamedCount, Result, SimparseError};

const SAMPLE: usize = 8;
const MAX_TYPES: usize = 128;
const TEXT_LIMIT: usize = 512;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct StepSummary {
    pub schemas: Vec<String>,
    /// Counts of observed declarations, including component types of complex instances.
    pub entity_type_counts: Vec<NamedCount>,
    pub products: Vec<StepProduct>,
    pub length_units: Vec<StepLengthUnit>,
    pub records_observed: usize,
    pub skipped_records: usize,
    pub samples_omitted: bool,
    /// True when input ended early or any record/type could not be inventoried.
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct StepProduct {
    pub native_ref: String,
    /// Part 21 string contents, with doubled apostrophes unescaped. Other escapes remain native.
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct StepLengthUnit {
    pub native_ref: String,
    pub expression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

pub fn inspect_step(path: &Path, max_record_bytes: usize) -> Result<StepSummary> {
    let mut input = BufReader::new(File::open(path)?).bytes().peekable();
    let mut record = Vec::new();
    let mut quoted = false;
    let mut comment = false;
    let mut overflow = false;
    let mut signature = false;
    let mut ended = false;
    let mut in_data = false;
    let mut data_seen = false;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut out = StepSummary {
        schemas: vec![],
        entity_type_counts: vec![],
        products: vec![],
        length_units: vec![],
        records_observed: 0,
        skipped_records: 0,
        samples_omitted: false,
        truncated: false,
    };
    let limit = max_record_bytes.clamp(1, 64 * 1024);
    while let Some(byte) = input.next() {
        let byte = byte?;
        if comment {
            if byte == b'*' && matches!(input.peek(), Some(Ok(b'/'))) {
                input.next().transpose()?;
                comment = false;
            }
            continue;
        }
        if !quoted && byte == b'/' && matches!(input.peek(), Some(Ok(b'*'))) {
            input.next().transpose()?;
            comment = true;
            if record.len() < limit {
                record.push(b' ');
            }
            continue;
        }
        if byte == b'\'' {
            if quoted && matches!(input.peek(), Some(Ok(b'\''))) {
                input.next().transpose()?;
                if record.len() + 2 <= limit {
                    record.extend_from_slice(b"''");
                } else {
                    overflow = true;
                }
                continue;
            }
            quoted = !quoted;
        }
        if byte == b';' && !quoted {
            if overflow {
                out.skipped_records += 1;
                out.truncated = true;
            } else {
                let text = String::from_utf8_lossy(&record);
                let text = text.trim().trim_start_matches('\u{feff}');
                if !signature {
                    if text != "ISO-10303-21" {
                        return Err(SimparseError::Parse(
                            "not a STEP Part 21 exchange file".into(),
                        ));
                    }
                    signature = true;
                } else if text == "END-ISO-10303-21" {
                    out.truncated |= in_data;
                    ended = true;
                } else if text == "ENDSEC" {
                    in_data = false;
                } else if text == "DATA" || text.starts_with("DATA(") {
                    in_data = true;
                    data_seen = true;
                } else if !in_data && text.starts_with("FILE_SCHEMA") {
                    out.schemas = strings(text).into_iter().take(SAMPLE).collect();
                } else if in_data && !ended {
                    observe(text, &mut out, &mut counts);
                }
            }
            record.clear();
            overflow = false;
        } else if record.len() < limit {
            record.push(byte);
        } else {
            overflow = true;
        }
    }
    if !signature {
        return Err(SimparseError::Parse(
            "not a STEP Part 21 exchange file".into(),
        ));
    }
    out.truncated |= !ended
        || !data_seen
        || quoted
        || comment
        || overflow
        || record.iter().any(|b| !b.is_ascii_whitespace());
    out.entity_type_counts = counts
        .into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect();
    Ok(out)
}

fn observe(text: &str, out: &mut StepSummary, counts: &mut BTreeMap<String, usize>) {
    let Some((id, rhs)) = text.split_once('=') else {
        out.truncated = true;
        return;
    };
    let id = id.trim();
    if id.len() > 32
        || id
            .strip_prefix('#')
            .and_then(|n| n.parse::<u64>().ok())
            .is_none()
    {
        out.truncated = true;
        return;
    }
    let components = components(rhs.trim());
    if components.is_empty() {
        out.truncated = true;
        return;
    }
    out.records_observed += 1;
    for (name, _) in &components {
        if name.len() > 64 || (!counts.contains_key(*name) && counts.len() >= MAX_TYPES) {
            out.truncated = true;
        } else {
            *counts.entry((*name).into()).or_default() += 1;
        }
    }
    if let Some((_, arguments)) = components.iter().find(|(name, _)| *name == "PRODUCT") {
        let labels = strings(arguments);
        if let Some(name) = labels.get(1).filter(|name| name.len() <= TEXT_LIMIT) {
            if out.products.len() < SAMPLE {
                out.products.push(StepProduct {
                    native_ref: id.into(),
                    name: name.clone(),
                });
            } else {
                out.samples_omitted = true;
            }
        } else {
            out.samples_omitted = true;
        }
    }
    if components.iter().any(|(name, _)| *name == "LENGTH_UNIT") {
        if out.length_units.len() < SAMPLE && rhs.len() <= TEXT_LIMIT {
            let unit = components
                .iter()
                .find(|(name, _)| *name == "SI_UNIT")
                .and_then(|(_, args)| si_length_unit(args))
                .map(str::to_string);
            out.length_units.push(StepLengthUnit {
                native_ref: id.into(),
                expression: rhs.trim().into(),
                unit,
            });
        } else {
            out.samples_omitted = true;
        }
    }
}

// Extract only the outer component calls of a simple or complex instance.
fn components(text: &str) -> Vec<(&str, &str)> {
    let bytes = text.as_bytes();
    let outer = usize::from(bytes.first() == Some(&b'('));
    let mut depth = 0;
    let mut quote = false;
    let mut start = None;
    let mut call = None;
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' {
            if quote && bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            quote = !quote;
        } else if !quote {
            if b == b'(' {
                if depth == outer
                    && let Some(s) = start.take()
                {
                    call = Some((s, i));
                }
                depth += 1;
            } else if b == b')' {
                if depth == 0 {
                    return vec![];
                }
                depth -= 1;
                if depth == outer
                    && let Some((s, open)) = call.take()
                {
                    out.push((text[s..open].trim(), &text[open + 1..i]));
                }
            } else if depth == outer && start.is_none() && (b.is_ascii_uppercase() || b == b'_') {
                start = Some(i);
            }
        }
        i += 1;
    }
    if depth != 0 || quote {
        return vec![];
    }
    out
}

fn strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    break;
                }
            }
            value.push(c);
        }
        if value.len() <= TEXT_LIMIT {
            out.push(value);
        }
    }
    out
}

fn si_length_unit(text: &str) -> Option<&'static str> {
    let (prefix, name) = text.split_once(',')?;
    if name.trim() != ".METRE." {
        return None;
    }
    match prefix.trim() {
        "$" => Some("m"),
        ".MILLI." => Some("mm"),
        ".MICRO." => Some("um"),
        ".CENTI." => Some("cm"),
        ".NANO." => Some("nm"),
        ".KILO." => Some("km"),
        _ => None,
    }
}
