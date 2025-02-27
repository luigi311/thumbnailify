use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use ini::Ini;
use libthumbnailer::file::get_file_uri;
use libthumbnailer::thumbnail::is_thumbnail_up_to_date;
use mime_guess;
use which::which;

use crate::file::{get_failed_thumbnail_output, get_thumbnail_hash_output};
use crate::hash::compute_hash;
use crate::sizes::ThumbnailSize;

/// Holds configuration parsed from a .thumbnailer file.
#[derive(Debug)]
struct ThumbnailerConfig {
    try_exec: Option<String>,
    exec_line: String,
    _mime_types: Vec<String>,
}


/// Searches standard directories for a .thumbnailer file supporting the given MIME type.
/// Looks in:
///   - /usr/share/thumbnailers
///   - $HOME/.local/share/thumbnailers
fn find_thumbnailer(mime_type: &str) -> io::Result<Option<ThumbnailerConfig>> {
    let mut dirs = Vec::new();

    if let Ok(home) = env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/thumbnailers"));
    }

    // Then check the system-wide thumbnailers.
    dirs.push(PathBuf::from("/usr/share/thumbnailers"));

    for dir in dirs {
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("thumbnailer") {
                    let conf = Ini::load_from_file(&path)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to load {:?}: {}", path, e)))?;
                    if let Some(section) = conf.section(Some("Thumbnailer Entry")) {
                        if let Some(mime_list) = section.get("MimeType") {
                            let mimes: Vec<String> = mime_list
                                .split(';')
                                .filter(|s| !s.trim().is_empty())
                                .map(|s| s.trim().to_string())
                                .collect();
                            if mimes.iter().any(|m| m == mime_type) {
                                let try_exec = section.get("TryExec").map(|s| s.to_string());
                                let exec_line = section.get("Exec")
                                    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Missing Exec key"))?
                                    .to_string();
                                return Ok(Some(ThumbnailerConfig {
                                    try_exec,
                                    exec_line,
                                    _mime_types: mimes,
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Builds command arguments by replacing tokens in the Exec string.
/// Supported tokens:
///   - %s : maximum desired size (pixels)
///   - %u : URI of the file
///   - %i : input file’s basename
///   - %o : output thumbnail file path
///   - %% : literal '%'
fn build_command_args(
    exec_line: &str,
    size: u32,
    file_uri: &str,
    input: &Path,
    output: &Path,
) -> Vec<String> {
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    exec_line
        .split_whitespace()
        .map(|token| {
            token
                .replace("%%", "%")
                .replace("%s", &size.to_string())
                .replace("%u", file_uri)
                .replace("%i", file_name)
                .replace("%o", output.to_str().unwrap_or(""))
        })
        .collect()
}


/// Generates a thumbnail for the given file using the GNOME thumbnailer approach.
/// 
/// This function:
/// 1. Computes the file URI and MD5 hash.
/// 2. Determines the cache output path using your helper (`get_thumbnail_hash_output`).
/// 3. Checks for an existing cached thumbnail.
/// 4. Detects the file’s MIME type and searches for an appropriate thumbnailer.
/// 5. Substitutes tokens into the Exec command and executes the thumbnailer.
/// 6. On failure, writes a fail marker using your helper (`get_failed_thumbnail_output`).
pub fn generate_thumbnail(file: &Path, size: ThumbnailSize) -> io::Result<PathBuf> {
    // Canonicalize the file and create a file URI.
    let abs_path = file.canonicalize()?;
    let file_str = abs_path.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid file path",
        )
    })?;
    let file_uri = get_file_uri(file_str)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to get file URI: {}", e)))?;

    // Compute the MD5 hash from the file URI.
    let hash = compute_hash(&file_uri);

    // Determine the expected output thumbnail path.
    let thumb_path = get_thumbnail_hash_output(&hash, size);

    // 3a. If the thumbnail already exists and is up to date, return it immediately.
    if thumb_path.exists() && is_thumbnail_up_to_date(&thumb_path, file_str) {
        return Ok(thumb_path);
    }

    // Determine the file's MIME type.
    let mime = mime_guess::from_path(file).first_or_octet_stream();
    let mime_type = mime.essence_str();

    // Look for a thumbnailer that supports this MIME type.
    let config = match find_thumbnailer(mime_type)? {
        Some(conf) => conf,
        None => {
            return Err(io::Error::new(io::ErrorKind::Other, "No thumbnailer found for this MIME type"));
        }
    };

    // If TryExec is specified, ensure that the executable exists.
    if let Some(ref exec_name) = config.try_exec {
        if which(exec_name).is_err() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Thumbnailer executable not found"));
        }
    }

    // Create a temporary file in the same directory as the final thumbnail.
    // Using `tempfile_in` ensures that the temp file is on the same filesystem
    // so that we can atomically persist (rename) it.
    let thumb_dir = thumb_path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "Thumbnail path has no parent directory")
    })?;
    fs::create_dir_all(thumb_dir)?;

    let named_temp = tempfile::Builder::new()
        .prefix("thumb-")
        .suffix(".png.tmp")
        .tempfile_in(thumb_dir)?;

    let temp_path = named_temp.path().to_owned();

    // Build the command using the Exec line from the thumbnailer config,
    // but pass the temporary file path as the output.
    let dimension = size.to_dimension();
    let args = build_command_args(&config.exec_line, dimension, &file_uri, file, &temp_path);

    // The first token is expected to be the executable.
    let executable = args.get(0)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Empty command"))?;
    let cmd_args = &args[1..];

    // Execute the external thumbnailer.
    let status = Command::new(&executable)
        .args(cmd_args)
        .status()?;

    if status.success() {
        // Persist the temporary file atomically to the final thumbnail path.
        named_temp.persist(&thumb_path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to persist thumbnail: {}", e)))?;
        Ok(thumb_path)
    } else {
        // Clean up the temporary file (it will be deleted when dropped).
        drop(named_temp);
        // Thumbnail generation failed; write a failure marker.
        let fail_marker = get_failed_thumbnail_output(&hash);
        if let Some(parent) = fail_marker.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(&fail_marker)?;
        Err(io::Error::new(io::ErrorKind::Other, "Thumbnailer process failed"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use serial_test::serial;
    use tempfile::tempdir;
    use temp_env::with_var;
    
    use crate::generate_thumbnail;
    use crate::sizes::ThumbnailSize;

    #[test]
    #[serial] // Ensure this test runs in isolation.
    fn test_generate_thumbnail() {
        // Create a temporary directory for the thumbnail cache.
        let temp_dir = tempdir().expect("Failed to create temporary directory for cache");
        let temp_cache = temp_dir.path();

        // Use `temp_env` to temporarily set the XDG_CACHE_HOME environment variable.
        with_var("XDG_CACHE_HOME", Some(temp_cache), || {

            let test_image = PathBuf::from("../tests/images/nasa-4019x4019.png");
            
            // Call your generate_thumbnail function.
            let thumb_path = generate_thumbnail(&test_image, ThumbnailSize::Normal)
                .expect("Thumbnail generation failed");

            // Check that the thumbnail file exists.
            assert!(
                thumb_path.exists(),
                "Thumbnail file was not created at {:?}",
                thumb_path
            );
        });
    }
}
