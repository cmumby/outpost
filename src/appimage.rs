use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

pub enum AppImageType {
    One,
    Two,
}

/// AppImages are ELF executables with the bytes "AI" plus a type number at offset 8.
pub fn detect(path: &Path) -> Result<AppImageType, String> {
    let mut header = [0u8; 11];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;

    if &header[..4] != b"\x7fELF" || &header[8..10] != b"AI" {
        return Err(format!("{} is not an AppImage", path.display()));
    }
    match header[10] {
        1 => Ok(AppImageType::One),
        2 => Ok(AppImageType::Two),
        n => Err(format!("{} is an unknown AppImage type ({n})", path.display())),
    }
}

/// Runs the AppImage's built-in `--appimage-extract <pattern>`, which unpacks the
/// matching files into `<workdir>/squashfs-root`. This doesn't need FUSE.
///
/// The pattern is matched against paths inside the image, and `*` doesn't cross
/// `/`, so `*.desktop` only matches files at the top level.
pub fn extract(appimage: &Path, workdir: &Path, pattern: &str) -> Result<(), String> {
    let output = Command::new(appimage)
        .arg("--appimage-extract")
        .arg(pattern)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run {}: {e}", appimage.display()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "extracting from the AppImage failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
