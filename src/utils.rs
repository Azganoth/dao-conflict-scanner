use std::{fs, io::Result as IoResult, path::Path, process::Command};

pub fn delete(path: &Path) -> IoResult<()> {
    fs::remove_file(path)
}

pub fn open_in_explorer(path: &Path) -> IoResult<()> {
    let absolute_path = path.canonicalize()?;
    let path_str = absolute_path
        .display()
        .to_string()
        .replace('/', "\\")
        .replace(r"\\?\", "");

    // FIX: rarely works
    Command::new("explorer.exe")
        .arg(format!("/select,{}", path_str))
        .spawn()?;

    Ok(())
}
