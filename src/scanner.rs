use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Error as AnyhowError;
use thiserror::Error as ThisError;
use walkdir::WalkDir;

use crate::erf::ErfFile;

const IGNORED_FILES: &[&str] = &["manifest.xml", "credits.txt", "readme.txt"];

#[derive(Debug, ThisError)]
pub enum ScanError {
    #[error("ERF file parse error at {path}: {source}")]
    ErfError {
        path: PathBuf,
        #[source]
        source: AnyhowError,
    },
}

pub type Conflicts = HashMap<String, Vec<PathBuf>>;

/// Scans a BioWare directory for file conflicts.
/// Returns a map where keys are duplicate file/resource names and values are lists of file paths.
pub fn scan_for_conflicts(bioware_dir: &Path) -> Result<Conflicts, ScanError> {
    let mut conflicts = Conflicts::new();
    let override_dir = bioware_dir.join("packages/core/override");

    for entry in WalkDir::new(bioware_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        if path.starts_with(&override_dir) {
            process_loose_file(path, &mut conflicts);
        } else if path
            .extension()
            .map_or(false, |ext| ext.eq_ignore_ascii_case("erf"))
        {
            process_erf_file(path, &mut conflicts)?;
        }
    }

    conflicts.retain(|key, paths| paths.len() > 1 && !should_ignore(key));

    for p in conflicts.values_mut() {
        p.sort();
    }

    Ok(conflicts)
}

fn process_loose_file(path: &Path, conflicts: &mut Conflicts) {
    if let Some(file_name) = path.file_name() {
        conflicts
            .entry(file_name.to_string_lossy().into_owned())
            .or_default()
            .push(path.to_path_buf());
    }
}

fn process_erf_file(path: &Path, conflicts: &mut Conflicts) -> Result<(), ScanError> {
    let erf = ErfFile::open(path).map_err(|source| ScanError::ErfError {
        path: path.to_path_buf(),
        source,
    })?;

    for entry in erf.toc {
        conflicts
            .entry(entry.name)
            .or_default()
            .push(path.to_path_buf());
    }

    Ok(())
}

fn should_ignore(name: &str) -> bool {
    let lowercase_name = name.to_ascii_lowercase();
    IGNORED_FILES.iter().any(|&f| f == lowercase_name)
}
