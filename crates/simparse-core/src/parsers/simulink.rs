//! Simulink `.slx` / `.mdl` inspection.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::{NamedCount, Result, SizeBucket};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SimulinkModelSummary {
    /// `slx`, `mdl` or `mdl-opc`.
    pub container: String,
    /// `model`, `library` or `subsystem`.
    pub kind: Option<String>,
    pub saved_release: Option<String>,
    pub saved_version: Option<String>,
    /// Stateflow and MATLAB Function blocks count once.
    pub block_count: usize,
    pub block_types: Vec<NamedCount>,
    /// Library-linked blocks by `SourceBlock`.
    pub library_links: Vec<NamedCount>,
    pub libraries: Vec<NamedCount>,
    pub subsystem_count: usize,
    pub max_depth: usize,
    pub line_count: usize,
    pub model_references: Vec<String>,
    /// Blocks by `SFBlockType`.
    pub stateflow_blocks: Vec<NamedCount>,
    pub stateflow: StateflowCounts,
    pub physical_modeling_products: Vec<String>,
    pub inports: Vec<String>,
    pub outports: Vec<String>,
    pub solver: SimulinkSolver,
    pub model_workspace: bool,
    pub size_breakdown: Vec<SizeBucket>,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StateflowCounts {
    pub charts: usize,
    pub states: usize,
    pub transitions: usize,
    pub junctions: usize,
    pub data: usize,
    pub events: usize,
}

/// Active configuration, as stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SimulinkSolver {
    pub solver_type: Option<String>,
    pub solver: Option<String>,
    pub start_time: Option<String>,
    pub stop_time: Option<String>,
    pub fixed_step: Option<String>,
    pub max_step: Option<String>,
    pub rel_tol: Option<String>,
    pub abs_tol: Option<String>,
    /// Set when the active configuration is a reference.
    pub reference: Option<String>,
    pub reference_location: Option<String>,
}

pub fn inspect_simulink_model(path: &Path) -> Result<SimulinkModelSummary> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".mdl") && !is_zip(path)? {
        inspect_mdl(path)
    } else {
        inspect_slx(path)
    }
}

pub fn looks_like_slx(path: &Path) -> bool {
    is_zip(path).unwrap_or(false)
}

pub fn looks_like_mdl(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(64 * 1024).read_to_end(&mut head).is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&head);
    if text.contains("__MWOPC_PACKAGE_BEGIN__") {
        return true;
    }
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .and_then(|line| line.strip_suffix('{'))
        .is_some_and(|section| matches!(section.trim(), "Model" | "Library" | "Subsystem"))
}

fn is_zip(path: &Path) -> Result<bool> {
    let mut magic = [0u8; 4];
    let mut file = std::fs::File::open(path)?;
    let read = file.read(&mut magic)?;
    Ok(read == 4 && magic == *b"PK\x03\x04")
}

// ---------------------------------------------------------------------------------------
// Shared model graph

#[derive(Default)]
struct Block {
    block_type: String,
    name: String,
    source_block: Option<String>,
    sf_block_type: Option<String>,
    port: Option<String>,
    model_name: Option<String>,
    child: Option<String>,
}

#[derive(Default)]
struct System {
    blocks: Vec<Block>,
    lines: usize,
}

#[derive(Default)]
struct Model {
    systems: HashMap<String, System>,
    root: Option<String>,
    kind: Option<String>,
    physical_modeling_products: Option<String>,
    solver: SimulinkSolver,
    stateflow: StateflowCounts,
}

const SOLVER_KEYS: [&str; 9] = [
    "SolverType",
    "SolverName",
    "Solver",
    "StartTime",
    "StopTime",
    "FixedStep",
    "MaxStep",
    "RelTol",
    "AbsTol",
];

fn set_solver(solver: &mut SimulinkSolver, key: &str, value: String) {
    let slot = match key {
        "SolverType" => &mut solver.solver_type,
        "SolverName" | "Solver" => &mut solver.solver,
        "StartTime" => &mut solver.start_time,
        "StopTime" => &mut solver.stop_time,
        "FixedStep" => &mut solver.fixed_step,
        "MaxStep" => &mut solver.max_step,
        "RelTol" => &mut solver.rel_tol,
        "AbsTol" => &mut solver.abs_tol,
        _ => return,
    };
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn infer_solver_type(solver: &mut SimulinkSolver) {
    if solver.solver_type.is_some() {
        return;
    }
    let kind = match solver.solver.as_deref() {
        Some(
            "ode1" | "ode2" | "ode3" | "ode4" | "ode5" | "ode8" | "ode14x" | "ode1be"
            | "FixedStepDiscrete" | "FixedStepAuto",
        ) => "Fixed-step",
        Some(
            "ode45"
            | "ode23"
            | "ode113"
            | "ode15s"
            | "ode23s"
            | "ode23t"
            | "ode23tb"
            | "odeN"
            | "daessc"
            | "VariableStepDiscrete"
            | "VariableStepAuto",
        ) => "Variable-step",
        _ => return,
    };
    solver.solver_type = Some(kind.into());
}

fn summarize(model: Model, out: &mut SimulinkModelSummary) {
    let mut block_types: BTreeMap<String, usize> = BTreeMap::new();
    let mut links: BTreeMap<String, usize> = BTreeMap::new();
    let mut libraries: BTreeMap<String, usize> = BTreeMap::new();
    let mut sf_blocks: BTreeMap<String, usize> = BTreeMap::new();
    let mut refs = Vec::new();
    let mut inports = Vec::new();
    let mut outports = Vec::new();
    let mut visited = HashSet::new();
    let mut stack: Vec<(String, usize)> = model.root.clone().into_iter().map(|r| (r, 0)).collect();

    while let Some((id, depth)) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(system) = model.systems.get(&id) else {
            continue;
        };
        out.line_count += system.lines;
        for block in &system.blocks {
            out.block_count += 1;
            let stateflow = block
                .sf_block_type
                .as_deref()
                .filter(|t| !t.is_empty() && *t != "NONE");
            let label = stateflow.unwrap_or(&block.block_type);
            *block_types.entry(label.to_string()).or_default() += 1;
            if let Some(kind) = stateflow {
                *sf_blocks.entry(kind.to_string()).or_default() += 1;
            } else if block.block_type == "SubSystem" {
                out.subsystem_count += 1;
                if let Some(child) = &block.child {
                    out.max_depth = out.max_depth.max(depth + 1);
                    stack.push((child.clone(), depth + 1));
                }
            }
            if let Some(source) = block.source_block.as_deref().filter(|s| !s.is_empty()) {
                let source = source.replace('\n', " ");
                let library = source.split('/').next().unwrap_or_default().to_string();
                *links.entry(source).or_default() += 1;
                *libraries.entry(library).or_default() += 1;
            }
            if block.block_type == "ModelReference"
                && let Some(name) = block.model_name.as_deref().filter(|n| !n.is_empty())
            {
                refs.push(name.to_string());
            }
            if depth == 0 {
                let port = block
                    .port
                    .as_deref()
                    .and_then(|p| p.trim().parse::<usize>().ok())
                    .unwrap_or(1);
                match block.block_type.as_str() {
                    "Inport" => inports.push((port, block.name.clone())),
                    "Outport" => outports.push((port, block.name.clone())),
                    _ => {}
                }
            }
        }
    }

    out.kind = model.kind;
    out.block_types = counts(block_types);
    out.library_links = counts(links);
    out.libraries = counts(libraries);
    out.stateflow_blocks = counts(sf_blocks);
    out.stateflow = model.stateflow;
    out.solver = model.solver;
    infer_solver_type(&mut out.solver);
    refs.sort();
    refs.dedup();
    out.model_references = refs;
    inports.sort();
    outports.sort();
    out.inports = inports
        .into_iter()
        .map(|(_, n)| n.replace('\n', " "))
        .collect();
    out.outports = outports
        .into_iter()
        .map(|(_, n)| n.replace('\n', " "))
        .collect();
    out.physical_modeling_products = model
        .physical_modeling_products
        .map(|p| {
            p.split('|')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
}

fn counts(map: BTreeMap<String, usize>) -> Vec<NamedCount> {
    let mut out: Vec<NamedCount> = map
        .into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out
}

// ---------------------------------------------------------------------------------------
// XML parts (.slx and text-packaged .mdl)

fn inspect_slx(path: &Path) -> Result<SimulinkModelSummary> {
    let file = std::fs::File::open(path)?;
    let mut zip = ZipArchive::new(file)?;
    let mut parts = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().trim_start_matches('/').to_string();
        let size = entry.size();
        let text = if wanted_part(&name) {
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            Some(text)
        } else {
            None
        };
        parts.push(Part { name, size, text });
    }
    Ok(summarize_parts("slx", parts))
}

struct Part {
    name: String,
    size: u64,
    text: Option<String>,
}

fn wanted_part(name: &str) -> bool {
    name.ends_with(".xml")
        && (name == "simulink/blockdiagram.xml"
            || name.starts_with("simulink/systems/")
            || name.starts_with("simulink/stateflow/")
            || name.starts_with("simulink/configSet")
            || name == "metadata/mwcorePropertiesReleaseInfo.xml"
            || name == "metadata/coreProperties.xml")
}

fn summarize_parts(container: &str, parts: Vec<Part>) -> SimulinkModelSummary {
    let mut out = SimulinkModelSummary {
        container: container.into(),
        ..Default::default()
    };
    let mut model = Model::default();
    let mut buckets: BTreeMap<String, u64> = BTreeMap::new();
    let mut active_config = None;
    let mut config_sets = Vec::new();

    for part in &parts {
        out.entries.push(part.name.clone());
        *buckets.entry(bucket_for(&part.name).into()).or_default() += part.size;
        if part.name.contains("modelWorkspace") {
            out.model_workspace = true;
        }
        if part.name == "simulink/configSetInfo.xml"
            && let Some(text) = &part.text
        {
            active_config = active_config_part(text);
        }
    }
    for part in &parts {
        let Some(text) = &part.text else { continue };
        let name = part.name.as_str();
        if name == "simulink/configSetInfo.xml" {
            continue;
        } else if name == "simulink/blockdiagram.xml" {
            parse_system_xml(text, None, &mut model);
        } else if let Some(file) = name.strip_prefix("simulink/systems/") {
            let id = file.trim_end_matches(".xml");
            parse_system_xml(text, Some(id), &mut model);
        } else if name.starts_with("simulink/stateflow/") {
            count_stateflow_xml(text, &mut model.stateflow);
        } else if name.starts_with("simulink/configSet") {
            config_sets.push((name, text));
        } else if name == "metadata/mwcorePropertiesReleaseInfo.xml" {
            out.saved_release = element_text(text, "release");
            out.saved_version = element_text(text, "version");
        } else if name == "metadata/coreProperties.xml" && out.saved_release.is_none() {
            out.saved_release = element_text(text, "cp:version");
        }
    }

    let chosen = active_config
        .as_deref()
        .and_then(|active| config_sets.iter().find(|(n, _)| *n == active))
        .or_else(|| {
            config_sets
                .iter()
                .find(|(n, _)| n.ends_with("configSet0.xml"))
        })
        .or_else(|| config_sets.first());
    let solver = match chosen {
        Some((_, text)) => active_solver(parse_config_xml(text)),
        None => parts
            .iter()
            .find(|p| p.name == "simulink/blockdiagram.xml")
            .and_then(|p| p.text.as_deref())
            .and_then(|text| active_solver(parse_config_xml(text))),
    };
    if let Some(solver) = solver {
        model.solver = solver;
    }
    if model.root.is_none() && model.systems.contains_key("system_root") {
        model.root = Some("system_root".into());
    }

    summarize(model, &mut out);
    out.entries.sort();
    out.size_breakdown = buckets
        .into_iter()
        .map(|(bucket, bytes)| SizeBucket { bucket, bytes })
        .collect();
    out
}

fn bucket_for(name: &str) -> &'static str {
    if name.starts_with("simulink/systems/") || name == "simulink/blockdiagram.xml" {
        "systems"
    } else if name.starts_with("simulink/stateflow/") {
        "stateflow"
    } else if name.ends_with(".mxarray") || name.contains("/bdmxdata/") {
        "data"
    } else if name.starts_with("simulink/configSet") {
        "configuration"
    } else if name.starts_with("metadata/") {
        "metadata"
    } else {
        "other"
    }
}

fn active_config_part(text: &str) -> Option<String> {
    let mut reader = Reader::from_str(text);
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"ConfigSet" => {
                let part = attr(&e, "PartName");
                if attr(&e, "Active").as_deref() == Some("true") {
                    return part.map(|p| p.trim_start_matches('/').to_string());
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

fn attr(event: &BytesStart<'_>, key: &str) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .map(|a| {
            a.unescape_value()
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(a.value.as_ref()).into_owned())
        })
}

fn push_text(buf: &mut String, event: &Event<'_>) {
    match event {
        Event::Text(t) => {
            if let Ok(text) = t.decode() {
                buf.push_str(&text);
            }
        }
        Event::CData(t) => buf.push_str(&String::from_utf8_lossy(t.as_ref())),
        Event::GeneralRef(r) => {
            if let Ok(Some(c)) = r.resolve_char_ref() {
                buf.push(c);
            } else if let Ok(name) = r.decode()
                && let Some(value) = quick_xml::escape::resolve_predefined_entity(&name)
            {
                buf.push_str(value);
            }
        }
        _ => {}
    }
}

fn element_text(text: &str, element: &str) -> Option<String> {
    let mut reader = Reader::from_str(text);
    let mut inside = false;
    let mut buf = String::new();
    loop {
        let event = reader.read_event().ok()?;
        match &event {
            Event::Start(e) if e.name().as_ref() == element.as_bytes() => inside = true,
            Event::End(e) if inside && e.name().as_ref() == element.as_bytes() => {
                let value = buf.trim().to_string();
                return (!value.is_empty()).then_some(value);
            }
            Event::Eof => return None,
            other if inside => push_text(&mut buf, other),
            _ => {}
        }
    }
}

enum Frame {
    System(String),
    Block(Block),
    Param(bool, String),
    Other,
}

fn block_param(name: &str) -> bool {
    matches!(
        name,
        "SourceBlock" | "SFBlockType" | "Port" | "ModelName" | "ModelNameDialog"
    )
}

fn parse_system_xml(text: &str, id: Option<&str>, model: &mut Model) {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Frame> = Vec::new();
    let mut inline = model.systems.len();
    let mut buf = String::new();

    while let Ok(event) = reader.read_event() {
        let empty = matches!(event, Event::Empty(_));
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let tag = e.name().as_ref().to_vec();
                let frame = match tag.as_slice() {
                    b"Model" | b"Library" | b"Subsystem" if id.is_none() && stack.len() <= 1 => {
                        model.kind = Some(String::from_utf8_lossy(&tag).to_ascii_lowercase());
                        Frame::Other
                    }
                    b"System" => {
                        if let Some(reference) = attr(e, "Ref") {
                            match stack.last_mut() {
                                Some(Frame::Block(block)) => block.child = Some(reference),
                                _ if model.root.is_none() => model.root = Some(reference),
                                _ => {}
                            }
                            Frame::Other
                        } else {
                            let sys_id = match (id, stack.is_empty()) {
                                (Some(id), true) => id.to_string(),
                                _ => {
                                    inline += 1;
                                    format!("inline_{inline}")
                                }
                            };
                            match stack.last_mut() {
                                Some(Frame::Block(block)) => block.child = Some(sys_id.clone()),
                                _ if id.is_none() && model.root.is_none() => {
                                    model.root = Some(sys_id.clone())
                                }
                                _ => {}
                            }
                            model.systems.entry(sys_id.clone()).or_default();
                            Frame::System(sys_id)
                        }
                    }
                    b"Block" if matches!(stack.last(), Some(Frame::System(_))) => {
                        Frame::Block(Block {
                            block_type: attr(e, "BlockType").unwrap_or_default(),
                            name: attr(e, "Name").unwrap_or_default(),
                            ..Default::default()
                        })
                    }
                    b"Line" => {
                        if let Some(Frame::System(sys)) = stack.last()
                            && let Some(system) = model.systems.get_mut(sys)
                        {
                            system.lines += 1;
                        }
                        Frame::Other
                    }
                    b"P" => {
                        let name = attr(e, "Name").unwrap_or_default();
                        let on_block = matches!(stack.last(), Some(Frame::Block(_)));
                        if (on_block && block_param(&name)) || name == "PhysicalModelingProducts" {
                            buf.clear();
                            Frame::Param(on_block, name)
                        } else {
                            Frame::Other
                        }
                    }
                    _ => Frame::Other,
                };
                stack.push(frame);
                if empty {
                    close_frame(&mut stack, &buf, model);
                }
            }
            Event::End(_) => close_frame(&mut stack, &buf, model),
            Event::Eof => break,
            other => {
                if matches!(stack.last(), Some(Frame::Param(..))) {
                    push_text(&mut buf, other);
                }
            }
        }
    }
}

fn close_frame(stack: &mut Vec<Frame>, buf: &str, model: &mut Model) {
    match stack.pop() {
        Some(Frame::Param(on_block, name)) => {
            let value = buf.trim().to_string();
            if name == "PhysicalModelingProducts" {
                model.physical_modeling_products = Some(value);
            } else if on_block && let Some(Frame::Block(block)) = stack.last_mut() {
                set_block_param(block, &name, value);
            }
        }
        Some(Frame::Block(block)) => {
            if let Some(Frame::System(sys)) = stack.last()
                && let Some(system) = model.systems.get_mut(sys)
            {
                system.blocks.push(block);
            }
        }
        _ => {}
    }
}

fn set_block_param(block: &mut Block, name: &str, value: String) {
    match name {
        "SourceBlock" => block.source_block = Some(value),
        "SFBlockType" => block.sf_block_type = Some(value),
        "Port" => block.port = Some(value),
        "ModelName" => block.model_name = Some(value),
        "ModelNameDialog" if block.model_name.is_none() => block.model_name = Some(value),
        _ => {}
    }
}

#[derive(Default)]
struct ChartCounts {
    eml: bool,
    counts: StateflowCounts,
}

fn bump(counts: &mut StateflowCounts, tag: &[u8]) {
    match tag {
        b"state" => counts.states += 1,
        b"transition" => counts.transitions += 1,
        b"junction" => counts.junctions += 1,
        b"data" => counts.data += 1,
        b"event" => counts.events += 1,
        _ => {}
    }
}

fn add(total: &mut StateflowCounts, part: &StateflowCounts) {
    total.charts += part.charts;
    total.states += part.states;
    total.transitions += part.transitions;
    total.junctions += part.junctions;
    total.data += part.data;
    total.events += part.events;
}

fn count_stateflow_xml(text: &str, total: &mut StateflowCounts) {
    let mut reader = Reader::from_str(text);
    let mut chart: Option<(usize, ChartCounts)> = None;
    let mut depth = 0usize;
    let mut chart_param: Option<String> = None;
    let mut buf = String::new();
    while let Ok(event) = reader.read_event() {
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                let tag = e.name().as_ref().to_vec();
                if tag == b"chart" && attr(e, "Ref").is_none() && !empty {
                    chart = Some((depth, ChartCounts::default()));
                } else if let Some((chart_depth, counts)) = chart.as_mut() {
                    if tag == b"P" && depth == *chart_depth + 1 && !empty {
                        chart_param = attr(e, "Name");
                        buf.clear();
                    }
                    bump(&mut counts.counts, &tag);
                } else if attr(e, "Ref").is_none() {
                    bump(total, &tag);
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                if e.name().as_ref() == b"P" && chart_param.is_some() {
                    let name = chart_param.take().unwrap_or_default();
                    if name == "type"
                        && buf.contains("EML")
                        && let Some((_, counts)) = chart.as_mut()
                    {
                        counts.eml = true;
                    }
                } else if e.name().as_ref() == b"chart"
                    && let Some((chart_depth, counts)) = chart.take_if(|(d, _)| *d == depth)
                {
                    let _ = chart_depth;
                    if !counts.eml {
                        add(total, &counts.counts);
                        total.charts += 1;
                    }
                }
            }
            Event::Eof => break,
            other if chart_param.is_some() => push_text(&mut buf, other),
            _ => {}
        }
    }
}

#[derive(Default)]
struct Configs {
    sets: Vec<(Option<String>, SimulinkSolver)>,
    active: Option<String>,
}

fn parse_config_xml(text: &str) -> Configs {
    let mut reader = Reader::from_str(text);
    let mut out = Configs::default();
    let mut current: Option<(usize, usize)> = None;
    let mut in_solver = 0usize;
    let mut key: Option<String> = None;
    let mut depth = 0usize;
    let mut buf = String::new();
    while let Ok(event) = reader.read_event() {
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                if !empty {
                    depth += 1;
                }
                let name = e.name().as_ref().to_vec();
                if name == b"Object" {
                    let class = attr(e, "ClassName");
                    let id = attr(e, "ObjectID");
                    if attr(e, "Reference").as_deref() == Some("true") {
                        if attr(e, "PropName").as_deref() == Some("ActiveConfigurationSet") {
                            out.active = id;
                        }
                    } else if matches!(
                        class.as_deref(),
                        Some("Simulink.ConfigSet" | "Simulink.ConfigSetRef")
                    ) && current.is_none()
                        && !empty
                    {
                        out.sets.push((id, SimulinkSolver::default()));
                        current = Some((depth, out.sets.len() - 1));
                    } else if class.as_deref() == Some("Simulink.SolverCC") && !empty {
                        in_solver = depth;
                    }
                } else if name == b"P"
                    && !empty
                    && let Some((config_depth, _)) = current
                {
                    let param = attr(e, "Name").unwrap_or_default();
                    let reference_param = depth == config_depth + 1
                        && matches!(
                            param.as_str(),
                            "SourceName" | "WSVarName" | "SourceLocation"
                        );
                    if reference_param || (in_solver > 0 && SOLVER_KEYS.contains(&param.as_str())) {
                        key = Some(param);
                        buf.clear();
                    }
                }
            }
            Event::End(_) => {
                if let Some(name) = key.take()
                    && let Some((_, index)) = current
                {
                    let solver = &mut out.sets[index].1;
                    let value = buf.trim().to_string();
                    match name.as_str() {
                        "SourceName" | "WSVarName" => {
                            solver.reference.get_or_insert(value);
                        }
                        "SourceLocation" => solver.reference_location = Some(value),
                        _ => set_solver(solver, &name, value),
                    }
                }
                if depth == in_solver {
                    in_solver = 0;
                }
                if current.is_some_and(|(d, _)| d == depth) {
                    current = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            other if key.is_some() => push_text(&mut buf, other),
            _ => {}
        }
    }
    out
}

fn active_solver(configs: Configs) -> Option<SimulinkSolver> {
    let Configs { mut sets, active } = configs;
    let index = active
        .and_then(|id| {
            sets.iter()
                .position(|(i, _)| i.as_deref() == Some(id.as_str()))
        })
        .or_else(|| sets.iter().position(|(_, s)| s.reference.is_none()))
        .or((!sets.is_empty()).then_some(0))?;
    Some(sets.swap_remove(index).1)
}

// ---------------------------------------------------------------------------------------
// .mdl text

const OPC_PART: &str = "__MWOPC_PART_BEGIN__";

fn inspect_mdl(path: &Path) -> Result<SimulinkModelSummary> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    if text.contains(OPC_PART) {
        Ok(summarize_parts("mdl-opc", opc_text_parts(&text)))
    } else {
        Ok(inspect_classic_mdl(&text))
    }
}

fn opc_text_parts(text: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix(OPC_PART) {
            if let Some((name, body)) = current.take() {
                parts.push(text_part(name, body));
            }
            current = Some((
                name.trim().trim_start_matches('/').to_string(),
                String::new(),
            ));
        } else if line.starts_with("__MWOPC_PACKAGE_END") {
            break;
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((name, body)) = current {
        parts.push(text_part(name, body));
    }
    parts
}

fn text_part(name: String, body: String) -> Part {
    let size = body.len() as u64;
    let text = wanted_part(&name).then_some(body);
    Part { name, size, text }
}

fn inspect_classic_mdl(text: &str) -> SimulinkModelSummary {
    let mut out = SimulinkModelSummary {
        container: "mdl".into(),
        ..Default::default()
    };
    let mut model = Model::default();
    let mut sections: Vec<String> = Vec::new();
    let mut systems: Vec<String> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut next_id = 0usize;
    let mut in_solver = false;
    let mut sf_object: Option<SfObject> = None;
    let mut sf_objects: Vec<SfObject> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parent = sections.last().map(String::as_str);
        let in_stateflow = sections.iter().any(|s| s == "Stateflow");
        if let Some(name) = line.strip_suffix('{').map(str::trim) {
            match name {
                "Model" | "Library" if sections.is_empty() => {
                    model.kind = Some(name.to_ascii_lowercase())
                }
                "System" if !in_stateflow => {
                    next_id += 1;
                    let id = format!("mdl_{next_id}");
                    if let Some(block) = blocks.last_mut() {
                        block.child = Some(id.clone());
                    } else if model.root.is_none() {
                        model.root = Some(id.clone());
                    }
                    model.systems.entry(id.clone()).or_default();
                    systems.push(id);
                }
                "Block" if parent == Some("System") => blocks.push(Block::default()),
                "Line" if parent == Some("System") => {
                    if let Some(system) = systems.last().and_then(|s| model.systems.get_mut(s)) {
                        system.lines += 1;
                    }
                }
                "Simulink.SolverCC" => in_solver = true,
                "chart" | "state" | "transition" | "junction" | "data" | "event"
                    if parent == Some("Stateflow") =>
                {
                    sf_object = Some(SfObject {
                        kind: name.to_string(),
                        ..Default::default()
                    });
                }
                _ => {}
            }
            sections.push(name.to_string());
            continue;
        }
        if line == "}" {
            let Some(name) = sections.pop() else { continue };
            let parent = sections.last().map(String::as_str);
            match name.as_str() {
                "System" if !sections.iter().any(|s| s == "Stateflow") => {
                    systems.pop();
                }
                "Block" if parent == Some("System") => {
                    if let Some(block) = blocks.pop()
                        && let Some(system) = systems.last().and_then(|s| model.systems.get_mut(s))
                    {
                        system.blocks.push(block);
                    }
                }
                "Simulink.SolverCC" => in_solver = false,
                _ if parent == Some("Stateflow") => sf_objects.extend(sf_object.take()),
                _ => {}
            }
            continue;
        }
        let (key, value) = match line.split_once(char::is_whitespace) {
            Some((k, v)) => (k, unquote(v)),
            None => (line, String::new()),
        };
        if key == "PhysicalModelingProducts" {
            model.physical_modeling_products = Some(value.clone());
        }
        match parent {
            Some("Block") if !blocks.is_empty() => {
                let block = blocks.last_mut().unwrap();
                match key {
                    "BlockType" => block.block_type = value,
                    "Name" => block.name = value,
                    k if block_param(k) => set_block_param(block, k, value),
                    _ => {}
                }
            }
            Some("Model") | Some("Library") if sections.len() == 1 => {
                if key == "Version" {
                    out.saved_version = Some(value)
                } else if SOLVER_KEYS.contains(&key) {
                    set_solver(&mut model.solver, key, value)
                }
            }
            Some(_) if in_solver && SOLVER_KEYS.contains(&key) => {
                set_solver(&mut model.solver, key, value)
            }
            _ if in_stateflow && sections.len() >= 2 => {
                if let Some(object) = sf_object.as_mut()
                    && sections[sections.len() - 2] == "Stateflow"
                {
                    match key {
                        "id" => object.id = value.parse().ok(),
                        "chart" => object.chart = value.parse().ok(),
                        "linkNode" => {
                            object.parent = value
                                .trim_matches(|c| c == '[' || c == ']')
                                .split_whitespace()
                                .next()
                                .and_then(|v| v.parse().ok())
                        }
                        "type" => object.eml = value.contains("EML"),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let eml: HashSet<u64> = sf_objects
        .iter()
        .filter(|o| o.kind == "chart" && o.eml)
        .filter_map(|o| o.id)
        .collect();
    for object in &sf_objects {
        if object.kind == "chart" {
            if !object.eml {
                model.stateflow.charts += 1;
            }
        } else if !object
            .chart
            .or(object.parent)
            .is_some_and(|c| eml.contains(&c))
        {
            bump(&mut model.stateflow, object.kind.as_bytes());
        }
    }

    summarize(model, &mut out);
    out.size_breakdown = vec![SizeBucket {
        bucket: "text".into(),
        bytes: text.len() as u64,
    }];
    out
}

#[derive(Default)]
struct SfObject {
    kind: String,
    id: Option<u64>,
    chart: Option<u64>,
    parent: Option<u64>,
    eml: bool,
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1]
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        value.to_string()
    }
}
