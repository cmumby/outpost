use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

pub enum AppImageType {
    One,
    Two,
}

/// AppImages are ELF executables with the bytes "AI" plus a type number at offset 8.
/// Errors read as the end of a sentence ("is not an AppImage") so callers can say
/// which file or URL they mean.
pub fn detect(path: &Path) -> Result<AppImageType, String> {
    let mut header = [0u8; 11];
    if File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_err()
    {
        return Err("is too small to be an AppImage".into());
    }

    if &header[..4] != b"\x7fELF" || &header[8..10] != b"AI" {
        return Err("is not an AppImage (if it's a link, make sure it points at the file itself, not a download page)".into());
    }
    match header[10] {
        1 => Ok(AppImageType::One),
        2 => Ok(AppImageType::Two),
        n => Err(format!("is an unknown AppImage type ({n})")),
    }
}

/// Most AppImages need libfuse2 to run (though not to install), and many newer
/// distros no longer include it by default.
pub fn libfuse2_installed() -> bool {
    let arch = std::env::consts::ARCH;
    [
        format!("/usr/lib/{arch}-linux-gnu"),
        format!("/lib/{arch}-linux-gnu"),
        "/usr/lib64".into(),
        "/lib64".into(),
        "/usr/lib".into(),
        "/lib".into(),
    ]
    .iter()
    .any(|dir| Path::new(dir).join("libfuse.so.2").exists())
}

/// The command to install libfuse2 on this distro.
pub fn libfuse2_fix() -> &'static str {
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let is = |name: &str| {
        os_release
            .lines()
            .any(|line| (line.starts_with("ID=") || line.starts_with("ID_LIKE=")) && line.contains(name))
    };
    if is("ubuntu") || is("debian") {
        "sudo apt install libfuse2t64    (on older releases: sudo apt install libfuse2)"
    } else if is("fedora") {
        "sudo dnf install fuse-libs"
    } else if is("arch") {
        "sudo pacman -S fuse2"
    } else if is("suse") {
        "sudo zypper install libfuse2"
    } else {
        "install your distribution's libfuse2 package"
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
