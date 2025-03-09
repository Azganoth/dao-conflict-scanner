use std::io;
use std::path::PathBuf;
use std::{path::Path, process::Command};

use directories::UserDirs;

pub fn open_location(path: &Path) -> io::Result<()> {
    let absolute_path = path.canonicalize()?;
    let path_str = absolute_path
        .display()
        .to_string()
        .replace('/', "\\") // Ensure Windows-style path
        .replace(r"\\?\", ""); // Remove extended path prefix

    Command::new("explorer.exe")
        .arg(format!("/select,{}", path_str))
        .spawn()?;

    Ok(())
}

pub fn get_bioware_dir() -> Option<PathBuf> {
    Some(UserDirs::new()?.document_dir()?.join("BioWare/Dragon Age"))
}
