use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::erf::ErfFile;
use walkdir::WalkDir;

pub type DuplicateGroups = HashMap<String, Vec<PathBuf>>;

fn should_ignore(name: &String) -> bool {
    let name = name.to_lowercase();
    name == "manifest.xml" || name == "credits.txt" || name == "readme.txt"
}

pub fn find_duplicates(bioware_dir: &Path) -> io::Result<DuplicateGroups> {
    if !bioware_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Directory does not exist: {}", bioware_dir.display()),
        ));
    }

    let mut duplicates = DuplicateGroups::new();
    let override_dir = bioware_dir.join("packages/core/override");

    for entry in WalkDir::new(bioware_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let entry_path = entry.path();

        // Handle loose files
        if entry_path.starts_with(&override_dir) {
            if let Some(file_name) = entry_path.file_name() {
                duplicates
                    .entry(file_name.to_string_lossy().into())
                    .or_insert_with(Vec::new)
                    .push(entry_path.to_path_buf());
            }
        }
        // Handle packed files (ERF)
        else if entry_path.extension().map_or(false, |ext| ext == "erf") {
            if let Ok(erf) = ErfFile::open(entry_path) {
                for toc_entry in erf.toc {
                    duplicates
                        .entry(toc_entry.name)
                        .or_insert_with(Vec::new)
                        .push(entry_path.to_path_buf());
                }
            }
        }
    }

    duplicates.retain(|key, paths| paths.len() > 1 && !should_ignore(key));

    Ok(duplicates)
}
