use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use crate::erf::ErfFile;
use walkdir::WalkDir;

pub type DuplicateGroups = HashMap<String, Vec<PathBuf>>;

const IGNORED_FILES: &[&str] = &["manifest.xml", "credits.txt", "readme.txt"];

pub fn find_duplicates(bioware_dir: &Path) -> io::Result<DuplicateGroups> {
    let mut duplicates = DuplicateGroups::new();
    let override_dir = bioware_dir.join("packages/core/override");

    for entry in WalkDir::new(bioware_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.starts_with(&override_dir) {
            process_loose_file(path, &mut duplicates);
        } else if is_erf_file(path) {
            process_erf_file(path, &mut duplicates)?;
        }
    }

    duplicates.retain(|key, paths| paths.len() > 1 && !should_ignore(key));
    Ok(duplicates)
}

fn is_erf_file(path: &Path) -> bool {
    path.extension().map_or(false, |ext| ext == "erf")
}

fn process_loose_file(path: &Path, duplicates: &mut DuplicateGroups) {
    if let Some(file_name) = path.file_name() {
        duplicates
            .entry(file_name.to_string_lossy().into_owned())
            .or_default()
            .push(path.to_path_buf());
    }
}

fn process_erf_file(path: &Path, duplicates: &mut DuplicateGroups) -> io::Result<()> {
    if let Ok(erf) = ErfFile::open(path) {
        for entry in erf.toc {
            duplicates
                .entry(entry.name)
                .or_default()
                .push(path.to_path_buf());
        }
    }
    Ok(())
}

fn should_ignore(name: &str) -> bool {
    let lower_name = name.to_lowercase();
    IGNORED_FILES.iter().any(|&f| lower_name == f)
}
