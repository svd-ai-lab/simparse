use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::GzDecoder;

use crate::{IcepakTzrEntry, IcepakTzrSummary, Result, SimparseError};

pub fn inspect_icepak_tzr(path: &Path, max_text_bytes: usize) -> Result<IcepakTzrSummary> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0_u8; 2];
    file.read_exact(&mut magic).map_err(|_| {
        SimparseError::Parse("Icepak TZR archive is too short to contain a tar stream".into())
    })?;
    file.seek(SeekFrom::Start(0))?;

    if magic == [0x1f, 0x8b] {
        return inspect_tar(GzDecoder::new(file), true, max_text_bytes);
    }
    inspect_tar(file, false, max_text_bytes)
}

fn inspect_tar(
    reader: impl Read,
    gzip_compressed: bool,
    max_text_bytes: usize,
) -> Result<IcepakTzrSummary> {
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    let mut entry_count = 0;
    let mut file_count = 0;
    let mut directory_count = 0;
    let mut uncompressed_bytes = 0_u64;
    let mut job_file_present = false;
    let mut model_file_present = false;
    let mut common_root = None;
    let mut has_nested_entry = false;
    let mut captured_text_bytes = 0_usize;
    let mut truncated = false;

    for entry in archive.entries()? {
        let entry = entry?;
        let name = entry
            .path()?
            .to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();
        let bytes = entry.header().size()?;
        let entry_type = entry.header().entry_type();
        let kind = if entry_type.is_file() {
            file_count += 1;
            "file"
        } else if entry_type.is_dir() {
            directory_count += 1;
            "directory"
        } else if entry_type.is_symlink() {
            "symlink"
        } else if entry_type.is_hard_link() {
            "hard-link"
        } else {
            "other"
        };

        entry_count += 1;
        uncompressed_bytes = uncompressed_bytes.saturating_add(bytes);
        let basename = name.rsplit('/').next().unwrap_or(&name);
        job_file_present |= basename.eq_ignore_ascii_case("job");
        model_file_present |= basename.eq_ignore_ascii_case("model");

        if let Some((root, _)) = name.split_once('/') {
            has_nested_entry = true;
            match &common_root {
                None => common_root = Some(root.to_string()),
                Some(existing) if existing != root => common_root = Some(String::new()),
                _ => {}
            }
        } else {
            common_root = Some(String::new());
        }

        if captured_text_bytes.saturating_add(name.len()) <= max_text_bytes {
            captured_text_bytes += name.len();
            entries.push(IcepakTzrEntry {
                name,
                entry_type: kind.to_string(),
                bytes,
            });
        } else {
            truncated = true;
        }
    }

    if entry_count == 0 {
        return Err(SimparseError::Parse(
            "Icepak TZR archive did not contain any tar entries".into(),
        ));
    }

    Ok(IcepakTzrSummary {
        project_name: has_nested_entry
            .then_some(common_root)
            .flatten()
            .filter(|value| !value.is_empty()),
        gzip_compressed,
        entry_count,
        file_count,
        directory_count,
        uncompressed_bytes,
        job_file_present,
        model_file_present,
        entries,
        truncated,
    })
}
