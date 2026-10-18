//! MATLAB MAT-file inventory.
use std::collections::HashMap;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::ZlibDecoder;
use hdf5_pure::{AttrValue, DType, File};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Result, SimparseError};

const MAX_VARIABLES: usize = 10_000;
const MAX_HEADER_BYTES: u64 = 1 << 20;
const MAX_HDF5_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MatFileSummary {
    /// `4`, `5` (v5, v6, v7) or `7.3`.
    pub version: String,
    pub platform: Option<String>,
    pub created: Option<String>,
    pub endian: Option<String>,
    pub compressed: Option<bool>,
    pub subsystem_data: bool,
    pub variable_count: usize,
    pub variables: Vec<MatVariable>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MatVariable {
    pub name: String,
    /// Object variables report their stored class name.
    pub class: String,
    /// Empty when unknown.
    pub dims: Vec<u64>,
    pub complex: bool,
    pub sparse: bool,
    pub global: bool,
    pub fields: Vec<String>,
    pub stored_bytes: u64,
}

pub fn looks_like_mat(path: &Path) -> bool {
    let mut head = [0u8; 19];
    let signed = std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && (head.starts_with(b"MATLAB 5.0 MAT-file") || head.starts_with(b"MATLAB 7.3 MAT-file"));
    signed || read_v4(path).is_some()
}

pub fn inspect_mat_file(path: &Path) -> Result<MatFileSummary> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut header = [0u8; 128];
    let full = file.read_exact(&mut header).is_ok();
    if !full || !header.starts_with(b"MATLAB ") {
        let variables =
            read_v4(path).ok_or_else(|| SimparseError::Parse("not a MAT file".into()))?;
        return Ok(MatFileSummary {
            version: "4".into(),
            variable_count: variables.len(),
            variables,
            ..Default::default()
        });
    }
    let text = String::from_utf8_lossy(&header[..116]);
    let text = text.trim_end_matches(['\0', ' ']);
    let mut out = MatFileSummary {
        platform: between(text, "Platform: ", ","),
        created: between(text, "Created on: ", " HDF5").or_else(|| {
            text.split("Created on: ")
                .nth(1)
                .map(|s| s.trim().to_string())
        }),
        ..Default::default()
    };
    if text.starts_with("MATLAB 7.3") {
        out.version = "7.3".into();
        inspect_hdf5(path, len, &mut out)?;
    } else {
        out.version = "5".into();
        inspect_v5(file, len, &header, &mut out)?;
    }
    out.variable_count = out.variables.len();
    Ok(out)
}

fn read_v4(path: &Path) -> Option<Vec<MatVariable>> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut pos = 0u64;
    let mut out = Vec::new();
    while pos < len {
        let mut head = [0u8; 20];
        file.seek(SeekFrom::Start(pos)).ok()?;
        file.read_exact(&mut head).ok()?;
        let mut fields = [0u32; 5];
        let endian = [Endian::Little, Endian::Big].into_iter().find(|e| {
            let t = e.u32(&head[..4]);
            t < 2000 && (t / 100) % 10 == 0 && (t / 10) % 10 <= 5 && t % 10 <= 2
        })?;
        for (i, f) in fields.iter_mut().enumerate() {
            *f = endian.u32(&head[i * 4..]);
        }
        let [kind, rows, cols, imag, name_len] = fields;
        if imag > 1 || name_len == 0 || name_len > 4096 || (kind % 10 == 1 && imag == 1) {
            return None;
        }
        let width = [8u64, 4, 4, 2, 2, 1][((kind / 10) % 10) as usize];
        let data = u64::from(rows) * u64::from(cols) * width * (1 + u64::from(imag));
        let mut name = vec![0u8; name_len as usize];
        file.read_exact(&mut name).ok()?;
        if name.last() != Some(&0) || !name[..name.len() - 1].iter().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        let data_start = pos + 20 + u64::from(name_len);
        let mut v = MatVariable {
            name: String::from_utf8_lossy(&name[..name.len() - 1]).into(),
            class: if kind % 10 == 1 { "char" } else { "double" }.into(),
            dims: vec![u64::from(rows), u64::from(cols)],
            complex: imag == 1,
            sparse: kind % 10 == 2,
            stored_bytes: 20 + u64::from(name_len) + data,
            ..Default::default()
        };
        if v.sparse && width == 8 && rows > 0 && cols >= 2 {
            let mut cell = |col: u64| -> Option<u64> {
                let mut b = [0u8; 8];
                file.seek(SeekFrom::Start(
                    data_start + (col * u64::from(rows) + u64::from(rows) - 1) * 8,
                ))
                .ok()?;
                file.read_exact(&mut b).ok()?;
                let value = match endian {
                    Endian::Little => f64::from_le_bytes(b),
                    Endian::Big => f64::from_be_bytes(b),
                };
                (value >= 0.0).then_some(value as u64)
            };
            v.dims = vec![cell(0)?, cell(1)?];
        }
        out.push(v);
        pos = data_start + data;
    }
    (pos == len && !out.is_empty()).then_some(out)
}

fn between(text: &str, start: &str, end: &str) -> Option<String> {
    let rest = text.split_once(start)?.1;
    let value = rest.split_once(end).map_or(rest, |(v, _)| v).trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        match self {
            Self::Little => u32::from_le_bytes(a),
            Self::Big => u32::from_be_bytes(a),
        }
    }

    fn u64(self, b: &[u8]) -> u64 {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        match self {
            Self::Little => u64::from_le_bytes(a),
            Self::Big => u64::from_be_bytes(a),
        }
    }
}

const MI_INT8: u32 = 1;
const MI_INT32: u32 = 5;
const MI_UINT32: u32 = 6;
const MI_MATRIX: u32 = 14;
const MI_COMPRESSED: u32 = 15;

fn inspect_v5(
    file: std::fs::File,
    len: u64,
    header: &[u8; 128],
    out: &mut MatFileSummary,
) -> Result<()> {
    let endian = match &header[126..128] {
        b"IM" => Endian::Little,
        b"MI" => Endian::Big,
        _ => return Err(SimparseError::Parse("unknown MAT endian indicator".into())),
    };
    out.endian = Some(match endian {
        Endian::Little => "little".into(),
        Endian::Big => "big".into(),
    });
    let raw = &header[116..124];
    let subsystem = (!raw.iter().all(|b| *b == 0 || *b == b' ')).then(|| endian.u64(raw));
    let mut compressed = false;
    let mut reader = BufReader::new(file);
    let mut pos = 128u64;
    while pos + 8 <= len {
        reader.seek(SeekFrom::Start(pos))?;
        let mut tag = [0u8; 8];
        reader.read_exact(&mut tag)?;
        let first = endian.u32(&tag[..4]);
        let (kind, size, total) = if first >> 16 != 0 {
            (first & 0xffff, u64::from(first >> 16), 8)
        } else {
            let size = u64::from(endian.u32(&tag[4..]));
            let total = if first == MI_COMPRESSED {
                8 + size
            } else {
                8 + size.div_ceil(8) * 8
            };
            (first, size, total)
        };
        if pos + total > len + 7 {
            out.truncated = true;
            break;
        }
        if Some(pos) == subsystem {
            out.subsystem_data = true;
        } else if kind == MI_MATRIX || kind == MI_COMPRESSED {
            let mut buf = Vec::new();
            let limited = (&mut reader).take(size.min(MAX_HEADER_BYTES));
            if kind == MI_COMPRESSED {
                compressed = true;
                let _ = ZlibDecoder::new(limited)
                    .take(MAX_HEADER_BYTES)
                    .read_to_end(&mut buf);
                buf = strip_matrix_tag(&buf, endian).unwrap_or_default();
            } else {
                limited.take(MAX_HEADER_BYTES).read_to_end(&mut buf)?;
            }
            match parse_matrix(&buf, endian) {
                Some(mut variable) if out.variables.len() < MAX_VARIABLES => {
                    variable.stored_bytes = size;
                    out.variables.push(variable);
                }
                Some(_) => out.truncated = true,
                None => out.truncated = true,
            }
        }
        pos += total;
    }
    out.compressed = Some(compressed);
    Ok(())
}

fn strip_matrix_tag(buf: &[u8], endian: Endian) -> Option<Vec<u8>> {
    (buf.len() >= 8 && endian.u32(&buf[..4]) == MI_MATRIX).then(|| buf[8..].to_vec())
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
    endian: Endian,
}

impl<'a> Cursor<'a> {
    fn next(&mut self) -> Option<(u32, &'a [u8])> {
        let tag = self.buf.get(self.pos..self.pos + 8)?;
        let first = self.endian.u32(&tag[..4]);
        if first >> 16 != 0 {
            let size = (first >> 16) as usize;
            let data = self.buf.get(self.pos + 4..self.pos + 4 + size.min(4))?;
            self.pos += 8;
            return Some((first & 0xffff, data));
        }
        let size = self.endian.u32(&tag[4..]) as usize;
        let data = self.buf.get(self.pos + 8..self.pos + 8 + size)?;
        self.pos += 8 + size.div_ceil(8) * 8;
        Some((first, data))
    }

    fn text(&mut self) -> Option<String> {
        let (kind, data) = self.next()?;
        (kind == MI_INT8).then(|| {
            String::from_utf8_lossy(data)
                .trim_end_matches('\0')
                .to_string()
        })
    }
}

fn parse_matrix(buf: &[u8], endian: Endian) -> Option<MatVariable> {
    let mut c = Cursor {
        buf,
        pos: 0,
        endian,
    };
    let (kind, flags) = c.next()?;
    if kind != MI_UINT32 || flags.len() < 4 {
        return None;
    }
    let word = endian.u32(flags);
    let class_id = word & 0xff;
    let bits = (word >> 8) & 0xff;
    let mut v = MatVariable {
        complex: bits & 0x08 != 0,
        global: bits & 0x04 != 0,
        sparse: class_id == 5,
        ..Default::default()
    };
    let logical = bits & 0x02 != 0;
    if class_id == 17 {
        v.name = c.text()?;
        let _type_system = c.text()?;
        v.class = c.text()?;
        return Some(v);
    }
    let (kind, dims) = c.next()?;
    if kind != MI_INT32 {
        return None;
    }
    v.dims = dims
        .chunks_exact(4)
        .map(|d| u64::from(endian.u32(d)))
        .collect();
    v.name = c.text()?;
    v.class = match class_id {
        1 => "cell".into(),
        2 => "struct".into(),
        3 => c.text().unwrap_or_default(),
        4 => "char".into(),
        5 | 9 if logical => "logical".into(),
        5 | 6 => "double".into(),
        7 => "single".into(),
        8 => "int8".into(),
        9 => "uint8".into(),
        10 => "int16".into(),
        11 => "uint16".into(),
        12 => "int32".into(),
        13 => "uint32".into(),
        14 => "int64".into(),
        15 => "uint64".into(),
        16 => "function_handle".into(),
        other => format!("class-{other}"),
    };
    if matches!(class_id, 2 | 3)
        && let Some((MI_INT32, width)) = c.next()
        && width.len() >= 4
        && let Some((MI_INT8, names)) = c.next()
    {
        let width = endian.u32(width) as usize;
        if width > 0 {
            v.fields = names
                .chunks(width)
                .map(|n| {
                    String::from_utf8_lossy(n)
                        .trim_end_matches('\0')
                        .to_string()
                })
                .filter(|n| !n.is_empty())
                .collect();
        }
    }
    Some(v)
}

fn inspect_hdf5(path: &Path, len: u64, out: &mut MatFileSummary) -> Result<()> {
    if len > MAX_HDF5_BYTES {
        out.truncated = true;
        return Ok(());
    }
    let file = File::open(path).map_err(hdf5_err)?;
    let root = file.root();
    let mut groups = root.groups().map_err(hdf5_err)?;
    let datasets = root.datasets().map_err(hdf5_err)?;
    out.subsystem_data = groups.iter().any(|g| g == "#subsystem#");
    groups.retain(|g| !g.starts_with('#'));
    let mut names: Vec<(String, bool)> = datasets
        .into_iter()
        .filter(|d| !d.starts_with('#'))
        .map(|d| (d, true))
        .chain(groups.into_iter().map(|g| (g, false)))
        .collect();
    names.sort();
    for (name, is_dataset) in names {
        if out.variables.len() >= MAX_VARIABLES {
            out.truncated = true;
            break;
        }
        let variable = if is_dataset {
            hdf5_dataset(&file, &name)
        } else {
            hdf5_group(&file, &name)
        };
        match variable {
            Ok(v) => out.variables.push(v),
            Err(_) => out.truncated = true,
        }
    }
    Ok(())
}

fn hdf5_err(error: hdf5_pure::Error) -> SimparseError {
    SimparseError::Hdf5(error.to_string())
}

fn attr_text(attrs: &HashMap<String, AttrValue>, key: &str) -> Option<String> {
    match attrs.get(key)? {
        AttrValue::String(s) | AttrValue::AsciiString(s) => Some(s.trim_end_matches('\0').into()),
        _ => None,
    }
}

fn attr_u64(attrs: &HashMap<String, AttrValue>, key: &str) -> Option<u64> {
    match attrs.get(key)? {
        AttrValue::U32(v) => Some(u64::from(*v)),
        AttrValue::U64(v) => Some(*v),
        AttrValue::I32(v) => u64::try_from(*v).ok(),
        AttrValue::I64(v) => u64::try_from(*v).ok(),
        AttrValue::F64(v) if *v >= 0.0 => Some(*v as u64),
        _ => None,
    }
}

fn attr_list(attrs: &HashMap<String, AttrValue>, key: &str) -> Vec<String> {
    match attrs.get(key) {
        Some(
            AttrValue::StringArray(v)
            | AttrValue::AsciiStringArray(v)
            | AttrValue::VarLenAsciiArray(v),
        ) => v.clone(),
        _ => Vec::new(),
    }
}

fn hdf5_dataset(file: &File, name: &str) -> std::result::Result<MatVariable, hdf5_pure::Error> {
    let ds = file.dataset(name)?;
    let attrs = ds.attrs()?;
    let class = attr_text(&attrs, "MATLAB_class").unwrap_or_else(|| "unknown".into());
    let mut dims: Vec<u64> = ds.shape()?.into_iter().rev().collect();
    if attr_u64(&attrs, "MATLAB_empty") == Some(1) {
        dims = ds.read_u64().unwrap_or_default();
    }
    if attrs.contains_key("MATLAB_object_decode") {
        dims.clear();
    }
    let complex = matches!(ds.dtype()?, DType::Compound(f) if f.iter().any(|(n, _)| n == "imag"));
    Ok(MatVariable {
        name: name.into(),
        class,
        dims,
        complex,
        global: attr_u64(&attrs, "MATLAB_global") == Some(1),
        ..Default::default()
    })
}

fn hdf5_group(file: &File, name: &str) -> std::result::Result<MatVariable, hdf5_pure::Error> {
    let group = file.group(name)?;
    let attrs = group.attrs()?;
    let class = attr_text(&attrs, "MATLAB_class").unwrap_or_else(|| "struct".into());
    let mut v = MatVariable {
        name: name.into(),
        class,
        global: attr_u64(&attrs, "MATLAB_global") == Some(1),
        ..Default::default()
    };
    if let Some(rows) = attr_u64(&attrs, "MATLAB_sparse") {
        v.sparse = true;
        let cols = group
            .dataset("jc")
            .and_then(|jc| jc.shape())
            .map(|s| s.iter().product::<u64>().saturating_sub(1))
            .unwrap_or(0);
        v.dims = vec![rows, cols];
        v.complex = group
            .dataset("data")
            .and_then(|d| d.dtype())
            .is_ok_and(|t| matches!(t, DType::Compound(f) if f.iter().any(|(n, _)| n == "imag")));
        return Ok(v);
    }
    let mut members = group.datasets()?;
    members.extend(group.groups()?);
    members.sort();
    v.fields = attr_list(&attrs, "MATLAB_fields");
    if v.fields.is_empty() {
        v.fields = members.clone();
    }
    v.dims = members
        .iter()
        .find_map(|m| {
            let ds = group.dataset(m).ok()?;
            matches!(ds.dtype().ok()?, DType::ObjectReference)
                .then(|| ds.shape().ok().map(|s| s.into_iter().rev().collect()))
                .flatten()
        })
        .unwrap_or_else(|| vec![1, 1]);
    Ok(v)
}
