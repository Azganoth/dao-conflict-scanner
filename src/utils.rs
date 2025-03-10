use std::fs;
use std::io::Result as IoResult;
use std::path::Path;
use std::process::Command;

pub fn delete(path: &Path) -> IoResult<()> {
    fs::remove_file(path)
}

// FIX: rarely works
pub fn open_in_explorer(path: &Path) -> IoResult<()> {
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
