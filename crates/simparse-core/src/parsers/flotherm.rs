use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::{
    FlothermBoundary, FlothermEntity, FlothermFloxmlSummary, FlothermGridAxis,
    FlothermSolutionDomain, FlothermSource, NamedCount, Result, SimparseError,
};

pub fn inspect_flotherm_floxml(
    path: &Path,
    max_text_bytes: usize,
) -> Result<FlothermFloxmlSummary> {
    let file = std::fs::File::open(path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);

    let mut state = ParserState::new(max_text_bytes);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => state.start(local_name(event.name().as_ref()))?,
            Event::Empty(event) => {
                let name = local_name(event.name().as_ref());
                state.start(name.clone())?;
                state.end(&name);
            }
            Event::Text(event) => {
                let text = String::from_utf8_lossy(event.as_ref());
                state.text(text.trim());
            }
            Event::End(event) => state.end(&local_name(event.name().as_ref())),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    state.finish()
}

pub fn has_flotherm_root(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                return is_flotherm_root(&local_name(event.name().as_ref()));
            }
            Ok(Event::Eof) | Err(_) => return false,
            Ok(_) => {}
        }
        buffer.clear();
    }
}

struct ParserState {
    root: Option<String>,
    stack: Vec<String>,
    name: Option<String>,
    solution: Option<String>,
    dimensionality: Option<String>,
    transient: Option<bool>,
    radiation: Option<String>,
    turbulence_type: Option<String>,
    gravity_direction: Option<String>,
    ambient_temperature: Option<String>,
    datum_pressure: Option<String>,
    outer_iterations: Option<String>,
    grid: Vec<FlothermGridAxis>,
    attribute_counts: BTreeMap<String, usize>,
    attributes: Vec<FlothermEntity>,
    geometry_counts: BTreeMap<String, usize>,
    geometry: Vec<FlothermEntity>,
    sources: Vec<FlothermSource>,
    solution_domain: Option<FlothermSolutionDomain>,
    pending_attributes: Vec<PendingEntity>,
    pending_geometry: Vec<PendingEntity>,
    remaining_text_bytes: usize,
    truncated: bool,
}

impl ParserState {
    fn new(max_text_bytes: usize) -> Self {
        Self {
            root: None,
            stack: Vec::new(),
            name: None,
            solution: None,
            dimensionality: None,
            transient: None,
            radiation: None,
            turbulence_type: None,
            gravity_direction: None,
            ambient_temperature: None,
            datum_pressure: None,
            outer_iterations: None,
            grid: Vec::new(),
            attribute_counts: BTreeMap::new(),
            attributes: Vec::new(),
            geometry_counts: BTreeMap::new(),
            geometry: Vec::new(),
            sources: Vec::new(),
            solution_domain: None,
            pending_attributes: Vec::new(),
            pending_geometry: Vec::new(),
            remaining_text_bytes: max_text_bytes,
            truncated: false,
        }
    }

    fn start(&mut self, name: String) -> Result<()> {
        if self.root.is_none() {
            if !is_flotherm_root(&name) {
                return Err(SimparseError::Parse(format!(
                    "expected FloTHERM FloXML root <xml_case> or <sm_xml_case>, got <{name}>"
                )));
            }
            self.root = Some(name.clone());
        }

        let parent = self.stack.last().cloned();
        self.stack.push(name.clone());
        let depth = self.stack.len();

        if name.ends_with("_att") && self.stack.iter().any(|part| part == "attributes") {
            *self.attribute_counts.entry(name.clone()).or_default() += 1;
            self.pending_attributes.push(PendingEntity {
                depth,
                kind: name,
                name: None,
                values: Vec::new(),
            });
        } else if parent.as_deref() == Some("geometry") {
            *self.geometry_counts.entry(name.clone()).or_default() += 1;
            self.pending_geometry.push(PendingEntity {
                depth,
                kind: name,
                name: None,
                values: Vec::new(),
            });
        }

        if self
            .stack
            .last()
            .is_some_and(|item| item == "solution_domain")
        {
            self.solution_domain = Some(FlothermSolutionDomain {
                fluid: None,
                boundaries: Vec::new(),
            });
        }
        Ok(())
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(current) = self.stack.last().cloned() else {
            return;
        };
        if !self.is_relevant_text(&current) {
            return;
        }
        let Some(value) = self.capture(text) else {
            return;
        };

        if self.stack.len() == 2 && current == "name" {
            self.name = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "modeling", "solution"]) {
            self.solution = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "modeling", "dimensionality"]) {
            self.dimensionality = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "modeling", "transient"]) {
            self.transient = parse_bool(&value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "modeling", "radiation"]) {
            self.radiation = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "turbulence", "turbulence_type"]) {
            self.turbulence_type = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "gravity", "normal_direction"]) {
            self.gravity_direction = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "global", "ambient_temperature"]) {
            self.ambient_temperature = Some(value);
            return;
        }
        if path_ends_with(&self.stack, &["model", "global", "datum_pressure"]) {
            self.datum_pressure = Some(value);
            return;
        }
        if path_ends_with(
            &self.stack,
            &["solve", "overall_control", "outer_iterations"],
        ) {
            self.outer_iterations = Some(value);
            return;
        }

        if self.assign_grid_value(&current, &value) {
            return;
        }

        if current == "name" {
            if let Some(pending) = self.pending_attributes.last_mut()
                && pending.depth + 1 == self.stack.len()
            {
                pending.name = Some(value);
                return;
            }
            if let Some(pending) = self.pending_geometry.last_mut()
                && pending.depth + 1 == self.stack.len()
            {
                pending.name = Some(value);
                return;
            }
        }

        if current == "power"
            && let Some(source) = self
                .pending_attributes
                .iter_mut()
                .rev()
                .find(|pending| pending.kind == "source_att")
        {
            source.values.push(value);
            return;
        }

        if self.stack.iter().any(|part| part == "solution_domain") {
            let Some(domain) = self.solution_domain.as_mut() else {
                return;
            };
            if current == "fluid" {
                domain.fluid = Some(value);
                return;
            }
            if let Some(face) = current.strip_suffix("_ambient") {
                domain.boundaries.push(FlothermBoundary {
                    face: face.to_string(),
                    kind: "ambient".to_string(),
                    value,
                });
                return;
            }
            if let Some(face) = current.strip_suffix("_boundary") {
                domain.boundaries.push(FlothermBoundary {
                    face: face.to_string(),
                    kind: "boundary".to_string(),
                    value,
                });
            }
        }
    }

    fn is_relevant_text(&self, current: &str) -> bool {
        (self.stack.len() == 2 && current == "name")
            || path_ends_with(&self.stack, &["model", "modeling", "solution"])
            || path_ends_with(&self.stack, &["model", "modeling", "dimensionality"])
            || path_ends_with(&self.stack, &["model", "modeling", "transient"])
            || path_ends_with(&self.stack, &["model", "modeling", "radiation"])
            || path_ends_with(&self.stack, &["model", "turbulence", "turbulence_type"])
            || path_ends_with(&self.stack, &["model", "gravity", "normal_direction"])
            || path_ends_with(&self.stack, &["model", "global", "ambient_temperature"])
            || path_ends_with(&self.stack, &["model", "global", "datum_pressure"])
            || path_ends_with(
                &self.stack,
                &["solve", "overall_control", "outer_iterations"],
            )
            || (matches!(current, "grid_type" | "min_size" | "max_size")
                && self
                    .stack
                    .iter()
                    .any(|part| matches!(part.as_str(), "x_grid" | "y_grid" | "z_grid")))
            || (current == "name"
                && (self
                    .pending_attributes
                    .last()
                    .is_some_and(|pending| pending.depth + 1 == self.stack.len())
                    || self
                        .pending_geometry
                        .last()
                        .is_some_and(|pending| pending.depth + 1 == self.stack.len())))
            || (current == "power"
                && self
                    .pending_attributes
                    .iter()
                    .any(|pending| pending.kind == "source_att"))
            || (self.stack.iter().any(|part| part == "solution_domain")
                && (current == "fluid"
                    || current.ends_with("_ambient")
                    || current.ends_with("_boundary")))
    }

    fn assign_grid_value(&mut self, current: &str, value: &str) -> bool {
        let Some(axis) = self
            .stack
            .iter()
            .rev()
            .find(|part| matches!(part.as_str(), "x_grid" | "y_grid" | "z_grid"))
            .cloned()
        else {
            return false;
        };
        if !matches!(current, "grid_type" | "min_size" | "max_size") {
            return false;
        }
        let item = if let Some(item) = self.grid.iter_mut().find(|item| item.axis == axis) {
            item
        } else {
            self.grid.push(FlothermGridAxis {
                axis: axis.clone(),
                grid_type: None,
                min_size: None,
                max_size: None,
            });
            self.grid.last_mut().expect("grid axis was just inserted")
        };
        match current {
            "grid_type" => item.grid_type = Some(value.to_string()),
            "min_size" => item.min_size = Some(value.to_string()),
            "max_size" => item.max_size = Some(value.to_string()),
            _ => unreachable!(),
        }
        true
    }

    fn capture(&mut self, text: &str) -> Option<String> {
        if text.len() > self.remaining_text_bytes {
            self.truncated = true;
            return None;
        }
        self.remaining_text_bytes -= text.len();
        Some(text.to_string())
    }

    fn end(&mut self, name: &str) {
        let depth = self.stack.len();
        if let Some(index) = self
            .pending_attributes
            .iter()
            .rposition(|pending| pending.depth == depth && pending.kind == name)
        {
            let pending = self.pending_attributes.remove(index);
            if let Some(entity_name) = pending.name {
                if pending.kind == "source_att" {
                    self.sources.push(FlothermSource {
                        name: entity_name.clone(),
                        powers: pending.values,
                    });
                }
                self.attributes.push(FlothermEntity {
                    kind: pending.kind,
                    name: entity_name,
                });
            }
        }
        if let Some(index) = self
            .pending_geometry
            .iter()
            .rposition(|pending| pending.depth == depth && pending.kind == name)
        {
            let pending = self.pending_geometry.remove(index);
            if let Some(entity_name) = pending.name {
                self.geometry.push(FlothermEntity {
                    kind: pending.kind,
                    name: entity_name,
                });
            }
        }
        self.stack.pop();
    }

    fn finish(mut self) -> Result<FlothermFloxmlSummary> {
        let root = self.root.ok_or_else(|| {
            SimparseError::Parse("FloTHERM FloXML file did not contain a root element".into())
        })?;
        self.grid.sort_by(|a, b| a.axis.cmp(&b.axis));
        self.attributes
            .sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
        self.geometry
            .sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
        self.sources.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(domain) = self.solution_domain.as_mut() {
            domain.boundaries.sort_by(|a, b| a.face.cmp(&b.face));
        }

        Ok(FlothermFloxmlSummary {
            root,
            name: self.name,
            solution: self.solution,
            dimensionality: self.dimensionality,
            transient: self.transient,
            radiation: self.radiation,
            turbulence_type: self.turbulence_type,
            gravity_direction: self.gravity_direction,
            ambient_temperature: self.ambient_temperature,
            datum_pressure: self.datum_pressure,
            outer_iterations: self.outer_iterations,
            grid: self.grid,
            attribute_type_counts: counts(self.attribute_counts),
            attributes: self.attributes,
            geometry_type_counts: counts(self.geometry_counts),
            geometry: self.geometry,
            sources: self.sources,
            solution_domain: self.solution_domain,
            truncated: self.truncated,
        })
    }
}

struct PendingEntity {
    depth: usize,
    kind: String,
    name: Option<String>,
    values: Vec<String>,
}

fn is_flotherm_root(name: &str) -> bool {
    matches!(name, "xml_case" | "sm_xml_case")
}

fn local_name(name: &[u8]) -> String {
    String::from_utf8_lossy(name)
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn path_ends_with(stack: &[String], suffix: &[&str]) -> bool {
    stack.len() >= suffix.len()
        && stack[stack.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn counts(values: BTreeMap<String, usize>) -> Vec<NamedCount> {
    values
        .into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect()
}
