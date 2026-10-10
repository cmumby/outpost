use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// Downloads `url` into `dest`, calling `progress(downloaded, total)` as bytes arrive.
/// `total` is `None` when the server doesn't say how big the file is.
pub fn download(url: &str, dest: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Outpost/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        // The default 30-second limit covers the whole download, which is far too
        // short for a large AppImage.
        .timeout(None)
        .build()
        .map_err(|e| describe(&e))?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|e| format!("download failed: {}", describe(&e)))?;

    let total = response.content_length();
    let mut file = File::create(dest).map_err(|e| format!("could not create {}: {e}", dest.display()))?;
    let mut buffer = vec![0; 64 * 1024];
    let mut downloaded = 0;
    loop {
        let n = response
            .read(&mut buffer)
            .map_err(|e| format!("download interrupted: {}", describe(&e)))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])
            .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
        downloaded += n as u64;
        progress(downloaded, total);
    }
    Ok(())
}

/// reqwest's own messages are vague ("error sending request"); the useful detail,
/// like "dns error" or "connection refused", is in the chain of causes.
fn describe(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    message
}
