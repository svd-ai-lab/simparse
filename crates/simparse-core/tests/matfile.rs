use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use hdf5_pure::{AttrValue, FileBuilder};
use simparse_core::{
    CompactFormatSummary, FormatSummary, InspectOptions, ScanOptions, SimFormat, detect_format,
    inspect_path, scan_paths, summarize_result,
};
use tempfile::tempdir;

#[derive(Clone, Copy)]
enum E {
    Le,
    Be,
}

fn u32b(e: E, v: u32) -> [u8; 4] {
    match e {
        E::Le => v.to_le_bytes(),
        E::Be => v.to_be_bytes(),
    }
}

fn element(e: E, kind: u32, data: &[u8]) -> Vec<u8> {
    let mut out = u32b(e, kind).to_vec();
    out.extend(u32b(e, data.len() as u32));
    out.extend(data);
    out.resize(out.len().div_ceil(8) * 8, 0);
    out
}

fn small(e: E, kind: u32, data: &[u8]) -> Vec<u8> {
    let mut out = u32b(e, ((data.len() as u32) << 16) | kind).to_vec();
    let mut d = data.to_vec();
    d.resize(4, 0);
    out.extend(d);
    out
}

fn matrix(e: E, class: u32, bits: u32, dims: &[u32], name: &str, extra: &[u8]) -> Vec<u8> {
    let mut body = element(e, 6, &[u32b(e, (bits << 8) | class), [0; 4]].concat());
    body.extend(element(
        e,
        5,
        &dims.iter().flat_map(|d| u32b(e, *d)).collect::<Vec<_>>(),
    ));
    body.extend(element(e, 1, name.as_bytes()));
    body.extend(extra);
    element(e, 14, &body)
}

fn header(e: E, text: &str, subsystem: u64) -> Vec<u8> {
    let mut h = text.as_bytes().to_vec();
    h.resize(116, b' ');
    h.extend(match e {
        E::Le => subsystem.to_le_bytes(),
        E::Be => subsystem.to_be_bytes(),
    });
    h.extend(match e {
        E::Le => [0x00, 0x01, b'I', b'M'],
        E::Be => [0x01, 0x00, b'M', b'I'],
    });
    h
}

fn compressed(e: E, element_bytes: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(element_bytes).unwrap();
    let data = z.finish().unwrap();
    let mut out = u32b(e, 15).to_vec();
    out.extend(u32b(e, data.len() as u32));
    out.extend(data);
    out
}

fn v5_file(e: E) -> Vec<u8> {
    let real = element(e, 9, &[0u8; 48]);
    let fields = [
        small(e, 5, &u32b(e, 8)),
        element(e, 1, b"alpha\0\0\0beta\0\0\0\0"),
    ]
    .concat();
    let mut vars = vec![
        matrix(e, 6, 0, &[2, 3], "x", &real),
        matrix(e, 9, 0x02, &[1, 4], "flags", &element(e, 2, &[1, 0, 1, 1])),
        matrix(
            e,
            4,
            0,
            &[1, 3],
            "label",
            &element(e, 4, &[97, 0, 98, 0, 99, 0]),
        ),
        matrix(e, 2, 0, &[1, 1], "cfg", &fields),
        matrix(e, 6, 0x08 | 0x04, &[1, 2], "z", &real),
        matrix(e, 5, 0, &[3, 4], "sp", &[]),
        matrix(e, 1, 0, &[2, 1], "c", &[]),
    ];
    vars.push(compressed(e, &matrix(e, 13, 0, &[5, 1], "packed", &[])));
    let mut out = header(
        e,
        "MATLAB 5.0 MAT-file, Platform: TEST64, Created on: Thu Jan  1 00:00:00 2099",
        0,
    );
    for v in vars {
        out.extend(v);
    }
    let subsystem = out.len() as u64;
    out.extend(matrix(e, 9, 0, &[1, 8], "", &[]));
    out[116..124].copy_from_slice(&match e {
        E::Le => subsystem.to_le_bytes(),
        E::Be => subsystem.to_be_bytes(),
    });
    out
}

fn check_v5(path: &std::path::Path, endian: &str) {
    assert_eq!(detect_format(path), Some(SimFormat::MatlabMat));
    let result = inspect_path(path, InspectOptions::default()).unwrap();
    let FormatSummary::MatFile(s) = &result.summary else {
        panic!("expected MAT summary");
    };
    assert_eq!(s.version, "5");
    assert_eq!(s.platform.as_deref(), Some("TEST64"));
    assert_eq!(s.endian.as_deref(), Some(endian));
    assert_eq!(s.compressed, Some(true));
    assert!(s.subsystem_data);
    assert!(!s.truncated);
    let names: Vec<_> = s.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(
        names,
        ["x", "flags", "label", "cfg", "z", "sp", "c", "packed"]
    );
    let v = |n: &str| s.variables.iter().find(|v| v.name == n).unwrap();
    assert_eq!(
        (v("x").class.as_str(), v("x").dims.clone()),
        ("double", vec![2, 3])
    );
    assert_eq!(v("flags").class, "logical");
    assert_eq!(v("label").class, "char");
    assert_eq!(v("cfg").class, "struct");
    assert_eq!(v("cfg").fields, ["alpha", "beta"]);
    assert!(v("z").complex && v("z").global);
    assert!(v("sp").sparse);
    assert_eq!(v("c").class, "cell");
    assert_eq!(
        (v("packed").class.as_str(), v("packed").dims.clone()),
        ("uint32", vec![5, 1])
    );

    let summary = summarize_result(&result);
    let CompactFormatSummary::MatFile(c) = &summary.summary else {
        panic!("expected compact MAT summary");
    };
    assert_eq!(c.variables.total, 8);
    assert!(!c.variables.truncated);
    let attrs = &result.ir.as_ref().unwrap()["dialects"]["cae"]["extensions"]["mathworks.mat"]["attributes"];
    assert_eq!(attrs["variable_count"], 8);
}

#[test]
fn inspects_v5_little_and_big_endian() {
    let tmp = tempdir().unwrap();
    for (e, name, endian) in [(E::Le, "le.mat", "little"), (E::Be, "be.mat", "big")] {
        let path = tmp.path().join(name);
        std::fs::write(&path, v5_file(e)).unwrap();
        check_v5(&path, endian);
    }
}

#[test]
fn inspects_v73_hdf5() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("big.mat");
    let mut b = FileBuilder::new();
    b.with_userblock(512);
    b.create_dataset("x")
        .with_f64_data(&[0.0; 6])
        .with_shape(&[3, 2])
        .set_attr("MATLAB_class", AttrValue::AsciiString("double".into()));
    b.create_dataset("e")
        .with_u64_data(&[0, 3])
        .set_attr("MATLAB_class", AttrValue::AsciiString("double".into()))
        .set_attr("MATLAB_empty", AttrValue::U32(1));
    b.create_dataset("g")
        .with_f64_data(&[1.0])
        .set_attr("MATLAB_class", AttrValue::AsciiString("double".into()))
        .set_attr("MATLAB_global", AttrValue::U32(1));
    let mut st = b.create_group("cfg");
    st.set_attr("MATLAB_class", AttrValue::AsciiString("struct".into()));
    st.set_attr(
        "MATLAB_fields",
        AttrValue::AsciiStringArray(vec!["a".into(), "b".into()]),
    );
    st.create_dataset("a").with_f64_data(&[1.0]);
    st.create_dataset("b").with_f64_data(&[2.0]);
    b.add_group(st.finish());
    let refs = b.create_group("#refs#");
    b.add_group(refs.finish());
    let sub = b.create_group("#subsystem#");
    b.add_group(sub.finish());
    b.write(&path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let head = header(
        E::Le,
        "MATLAB 7.3 MAT-file, Platform: TEST64, Created on: Thu Jan  1 00:00:00 2099 HDF5 schema 1.00 .",
        0,
    );
    bytes[..128].copy_from_slice(&head);
    std::fs::write(&path, bytes).unwrap();

    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::MatFile(s) = &result.summary else {
        panic!("expected MAT summary");
    };
    assert_eq!(s.version, "7.3");
    assert_eq!(s.created.as_deref(), Some("Thu Jan  1 00:00:00 2099"));
    assert!(s.subsystem_data);
    let names: Vec<_> = s.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["cfg", "e", "g", "x"]);
    let v = |n: &str| s.variables.iter().find(|v| v.name == n).unwrap();
    assert_eq!(v("x").dims, [2, 3]);
    assert_eq!(v("e").dims, [0, 3]);
    assert!(v("g").global);
    assert_eq!(v("cfg").class, "struct");
    assert_eq!(v("cfg").fields, ["a", "b"]);
}

#[test]
fn skips_files_without_a_mat_header() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("ok.mat"), v5_file(E::Le)).unwrap();
    std::fs::write(tmp.path().join("empty.mat"), b"").unwrap();
    std::fs::write(tmp.path().join("other.mat"), b"not a MAT file at all").unwrap();
    let results = scan_paths(&[tmp.path().to_path_buf()], ScanOptions::default()).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].format, SimFormat::MatlabMat);
}

fn v4_matrix(kind: u32, rows: u32, cols: u32, imag: u32, name: &str, data: &[f64]) -> Vec<u8> {
    let mut out: Vec<u8> = [kind, rows, cols, imag, name.len() as u32 + 1]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    out.extend(name.as_bytes());
    out.push(0);
    out.extend(data.iter().flat_map(|v| v.to_le_bytes()));
    out
}

#[test]
fn inspects_level4_files() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("old.mat");
    let bytes = [
        v4_matrix(0, 2, 3, 0, "a", &[0.0; 6]),
        v4_matrix(0, 1, 2, 1, "z", &[0.0; 4]),
        v4_matrix(1, 1, 2, 0, "t", &[104.0, 105.0]),
        v4_matrix(2, 2, 3, 0, "s", &[1.0, 5.0, 2.0, 7.0, 3.0, 0.0]),
    ]
    .concat();
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(detect_format(&path), Some(SimFormat::MatlabMat));
    let result = inspect_path(&path, InspectOptions::default()).unwrap();
    let FormatSummary::MatFile(s) = &result.summary else {
        panic!("expected MAT summary");
    };
    assert_eq!(s.version, "4");
    let got: Vec<_> = s
        .variables
        .iter()
        .map(|v| {
            (
                v.name.as_str(),
                v.class.as_str(),
                v.dims.clone(),
                v.complex,
                v.sparse,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("a", "double", vec![2, 3], false, false),
            ("z", "double", vec![1, 2], true, false),
            ("t", "char", vec![1, 2], false, false),
            ("s", "double", vec![5, 7], false, true),
        ]
    );

    let junk = tmp.path().join("junk.mat");
    std::fs::write(&junk, [0u8, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]).unwrap();
    assert_eq!(detect_format(&junk), None);
}
