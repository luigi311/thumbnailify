use log::{debug, info, warn};
use std::{
    env,
    fs,
    fs::File,
    io,
    path::{Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH
};

use ini::Ini;
use mime_guess;
use png::Decoder;
use shell_words::split;
use which::which;

use crate::{
    error::ThumbnailError,
    file::{
        add_thumbnail_metadata, get_failed_thumbnail_output, get_file_uri, get_thumbnail_hash_output, write_failed_thumbnail
    },
    hash::compute_hash,
    sizes::ThumbnailSize,
};

/// Holds configuration parsed from a .thumbnailer file.
#[derive(Debug)]
struct ThumbnailerConfig {
    try_exec: Option<String>,
    exec_line: String,
    _mime_types: Vec<String>,
}

/// Checks whether the thumbnail file at `thumb_path` is up to date with respect
/// to the source image at `source_path`. It verifies two metadata fields in the PNG:
/// 
/// - "Thumb::MTime": the source file's modification time (in seconds since UNIX_EPOCH)
/// - "Thumb::Size": the source file's size in bytes (only checked if present)
///
/// Returns true if "Thumb::MTime" is present and matches the source file's modification time,
/// and if "Thumb::Size" is present it must match the source file's size.
pub fn is_thumbnail_up_to_date(thumb_path: &Path, source_path: &Path) -> bool {
    debug!(
        "Checking if thumbnail at {:?} is up-to-date with source {:?}",
        thumb_path, source_path
    );

    let file = match File::open(thumb_path) {
        Ok(f) => f,
        Err(e) => {
            debug!("Failed to open thumbnail {:?}: {}", thumb_path, e);
            return false;
        }
    };

    let decoder = Decoder::new(file);
    let reader = match decoder.read_info() {
        Ok(r) => r,
        Err(e) => {
            debug!("Failed to read PNG info for {:?}: {}", thumb_path, e);
            return false;
        }
    };

    let texts = &reader.info().uncompressed_latin1_text;

    let thumb_mtime_str = match texts.iter().find(|c| c.keyword == "Thumb::MTime") {
        Some(c) => &c.text,
        None => {
            debug!("Thumbnail missing 'Thumb::MTime' metadata chunk.");
            return false;
        }
    };
    let thumb_mtime = thumb_mtime_str.parse::<u64>().unwrap_or(0);

    let source_metadata = match std::fs::metadata(source_path) {
        Ok(m) => m,
        Err(e) => {
            debug!("Failed to get metadata of source {:?}: {}", source_path, e);
            return false;
        }
    };

    let source_modified_time = match source_metadata.modified() {
        Ok(mt) => mt.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        Err(e) => {
            debug!("Failed to read modified time of source {:?}: {}", source_path, e);
            return false;
        }
    };

    if thumb_mtime != source_modified_time {
        debug!(
            "Thumb::MTime mismatch: thumbnail={} source={}",
            thumb_mtime, source_modified_time
        );
        return false;
    }

    if let Some(chunk) = texts.iter().find(|c| c.keyword == "Thumb::Size") {
        let thumb_size = chunk.text.parse::<u64>().unwrap_or(0);
        let source_file_size = source_metadata.len();
        if thumb_size != source_file_size {
            debug!(
                "Thumb::Size mismatch: thumbnail={} source={}",
                thumb_size, source_file_size
            );
            return false;
        }
    }

    debug!("Thumbnail at {:?} is up-to-date with source {:?}", thumb_path, source_path);
    true
}

/// Searches standard directories for a .thumbnailer file supporting the given MIME type.
/// Looks in:
///   - $HOME/.local/share/thumbnailers
///   - $XDG_DATA_DIRS/thumbnailers
///   - /usr/share/thumbnailers
fn find_thumbnailer(mime_type: &str) -> Result<Option<ThumbnailerConfig>, ThumbnailError> {
    debug!("Searching for .thumbnailer supporting MIME type: {}", mime_type);

    let mut dirs = Vec::new();

    if let Ok(home) = env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/thumbnailers"));
    }

    if let Ok(xdg_data_dirs) = env::var("XDG_DATA_DIRS") {
        // Split the variable by `:` and collect as PathBuf
        let data_dirs: Vec<PathBuf> = xdg_data_dirs
            .split(':')
            .map(PathBuf::from)
            .collect();

        // Print the directories
        for dir in &data_dirs {
            dirs.push(dir.join("thumbnailers"));
        }
    }

    dirs.push(PathBuf::from("/usr/share/thumbnailers"));

    for dir in dirs {
        debug!("Looking for thumbnailer files in {:?}", dir);
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("thumbnailer") {
                    let conf = Ini::load_from_file(&path)?;
                    if let Some(section) = conf.section(Some("Thumbnailer Entry")) {
                        if let Some(mime_list) = section.get("MimeType") {
                            let mimes: Vec<String> = mime_list
                                .split(';')
                                .filter(|s| !s.trim().is_empty())
                                .map(|s| s.trim().to_string())
                                .collect();
                            if mimes.iter().any(|m| m == mime_type) {
                                debug!("Found thumbnailer config in {:?}", path);
                                let try_exec = section.get("TryExec").map(|s| s.to_string());
                                let exec_line = section
                                    .get("Exec")
                                    .ok_or_else(|| {
                                        io::Error::new(io::ErrorKind::Other, "Missing Exec key")
                                    })?
                                    .to_string();

                                let config = ThumbnailerConfig {
                                    try_exec,
                                    exec_line,
                                    _mime_types: mimes,
                                };
                                return Ok(Some(config));
                            }
                        }
                    }
                }
            }
        }
    }
    debug!("No .thumbnailer found for MIME type: {}", mime_type);
    Ok(None)
}

/// Builds command arguments by replacing tokens in the Exec string.
///
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
) -> Result<Vec<String>, ThumbnailError> {
    debug!("Building command args from exec_line: {}", exec_line);

    let full_input_path = input.canonicalize()?;
    let full_input_path_str = full_input_path.to_str().ok_or_else(|| {
        ThumbnailError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid path: {:?}", input),
        ))
    })?;

    let tokens = split(exec_line)?;
    let replaced: Vec<String> = tokens
        .into_iter()
        .map(|token| {
            token
                .replace("%%", "%")
                .replace("%s", &size.to_string())
                .replace("%u", file_uri)
                .replace("%i", full_input_path_str)
                .replace("%o", output.to_str().unwrap_or(""))
        })
        .collect();

    debug!("Command tokens after substitution: {:?}", replaced);
    Ok(replaced)
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
pub fn generate_thumbnail(file: &Path, size: ThumbnailSize) -> Result<PathBuf, ThumbnailError> {
    info!("Generating thumbnail for {:?} with size {:?}", file, size);

    // Canonicalize the file and create a file URI.
    let abs_path = file.canonicalize()?;
    let file_str = abs_path.to_str().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid file path")
    })?;
    let file_uri = get_file_uri(file)?;

    // Compute the MD5 hash from the file URI.
    let hash = compute_hash(&file_uri);

    // Check if the fail marker exists and is up to date
    let fail_path = get_failed_thumbnail_output(&hash);
    if fail_path.exists() && is_thumbnail_up_to_date(&fail_path, file) {
        info!(
            "A fail marker exists and is up-to-date, returning fail marker at {:?}",
            fail_path
        );
        return Ok(fail_path);
    }

    // Determine the expected output thumbnail path.
    let thumb_path = get_thumbnail_hash_output(&hash, size);

    // If the thumbnail already exists and is up to date, return it immediately.
    if thumb_path.exists() && is_thumbnail_up_to_date(&thumb_path, file) {
        info!(
            "Cached thumbnail at {:?} is up-to-date, returning it",
            thumb_path
        );
        return Ok(thumb_path);
    }

    // Determine the file's MIME type.
    let mime = mime_guess::from_path(file).first_or_octet_stream();
    let mime_type = mime.essence_str();
    debug!("Detected MIME type for {:?} as {}", file, mime_type);

    // Look for a thumbnailer that supports this MIME type.
    let config = match find_thumbnailer(mime_type)? {
        Some(conf) => {
            debug!("Using thumbnailer config: {:?}", conf);
            conf
        }
        None => {
            warn!("No thumbnailer found for MIME type {}", mime_type);
            return Err(ThumbnailError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "No thumbnailer found for this MIME type",
            )));
        }
    };

    // If TryExec is specified, ensure that the executable exists.
    if let Some(ref exec_name) = config.try_exec {
        if which(exec_name).is_err() {
            warn!(
                "TryExec specified ({}) but could not be found on PATH.",
                exec_name
            );
            return Err(ThumbnailError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Thumbnailer executable not found",
            )));
        }
    }

    // Prepare a temporary file in the same directory as the final thumbnail.
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

    // Build the command using the Exec line from the thumbnailer config.
    let dimension = size.to_dimension();
    let args = build_command_args(&config.exec_line, dimension, &file_uri, &file, &temp_path)?;

    // The first token is the executable; the rest are arguments.
    let executable = args
        .get(0)
        .cloned()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Empty command"))?;
    let cmd_args = &args[1..];

    debug!("Executing thumbnailer: {:?} {:?}", executable, cmd_args);

    // Check if Bubblewrap ("bwrap") is available.
    let status = if let Ok(bwrap_path) = which("bwrap") {
        debug!("Running thumbnail command under bubblewrap sandbox.");
        let mut command = Command::new(bwrap_path);
        // Minimal sandbox setup
        command.args(&["--ro-bind", "/usr", "/usr"]);
        command.args(&["--ro-bind-try", "/etc/ld.so.cache", "/etc/ld.so.cache"]);
        command.args(&["--ro-bind-try", "/etc/alternatives", "/etc/alternatives"]);

        let usrmerged_dirs = ["bin", "lib64", "lib", "sbin"];
        for dir in &usrmerged_dirs {
            let absolute_dir = format!("/{}", dir);
            if Path::new(&absolute_dir).exists() {
                if let Ok(meta) = fs::symlink_metadata(&absolute_dir) {
                    if meta.file_type().is_symlink() {
                        let symlink_target = format!("/usr/{}", dir);
                        command.args(&["--symlink", &symlink_target, &absolute_dir]);
                    } else {
                        command.args(&["--ro-bind", &absolute_dir, &absolute_dir]);
                    }
                }
            }
        }

        command.args(&["--proc", "/proc"]);
        command.args(&["--dev", "/dev"]);
        command.args(&["--chdir", "/"]);
        command.args(&["--setenv", "GIO_USE_VFS", "local"]);
        command.args(&["--unshare-all", "--die-with-parent"]);

        // Bind the thumbnail output directory so our temporary file is visible.
        let thumb_dir_str = thumb_dir.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Invalid thumb_dir path")
        })?;
        command.args(&["--bind", thumb_dir_str, thumb_dir_str]);

        // **Bind the source file** so that the sandboxed process can access it.
        command.args(&["--ro-bind", file_str, file_str]);

        // Append the external command.
        command.arg("--");
        command.arg(&executable);
        command.args(cmd_args);

        debug!("Final bubblewrap command: {:?}", command);
        command.status()?
    } else {
        debug!("Running thumbnail command directly (no bwrap).");
        Command::new(&executable).args(cmd_args).status()?
    };

    if status.success() {
        add_thumbnail_metadata(&temp_path, &abs_path)?;

        info!("Thumbnail command succeeded; persisting thumbnail to {:?}", thumb_path);        
        named_temp.persist(&thumb_path)?;
        Ok(thumb_path)
    } else {
        warn!(
            "Thumbnail command failed with status: {:?}. Generating fail marker.",
            status.code()
        );
        // Clean up temp file
        drop(named_temp);

        // Write fail marker
        let fail_marker = get_failed_thumbnail_output(&hash);
        if let Some(parent) = fail_marker.parent() {
            fs::create_dir_all(parent)?;
        }
        
        write_failed_thumbnail(&fail_marker, &fail_path)?;
        add_thumbnail_metadata(&fail_path, &abs_path)?;

        Err(ThumbnailError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Thumbnailer process failed",
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use serial_test::serial;
    use tempfile::tempdir;
    use temp_env::with_var;
    
    use crate::file::{get_failed_thumbnail_output, get_file_uri};
    use crate::generate_thumbnail;
    use crate::hash::compute_hash;
    use crate::ThumbnailSize;

    #[test]
    #[serial] // Ensure this test runs in isolation.
    fn test_generate_thumbnail() {
        // Create a temporary directory for the thumbnail cache.
        let temp_dir = tempdir().expect("Failed to create temporary directory for cache");
        let temp_cache = temp_dir.path();

        // Use `temp_env` to temporarily set the XDG_CACHE_HOME environment variable.
        with_var("XDG_CACHE_HOME", Some(temp_cache), || {

            let test_image = PathBuf::from("tests/images/nasa-4019x4019.png");
            
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

    #[test]
    #[serial] // Serialize tests modifying env vars
    fn test_generate_thumbnail_broken_image() {
        // Create a temporary directory for the thumbnail cache.
        let temp_dir = tempdir().expect("Failed to create temporary directory for cache");
        let temp_cache = temp_dir.path();

        // Use temp_env to temporarily set XDG_CACHE_HOME.
        with_var("XDG_CACHE_HOME", Some(temp_cache), || {
            let test_image = PathBuf::from("tests/images/broken.png");
            
            // Attempt to generate a thumbnail for the broken image.
            let result = generate_thumbnail(&test_image, ThumbnailSize::Normal);
            assert!(result.is_err(), "Expected thumbnail generation to fail for a broken image");

            // Verify that a failure marker file was created.
            let file_uri = get_file_uri(&test_image)
                .expect("Failed to get file URI for broken image");
            let hash = compute_hash(&file_uri);
            let fail_marker = get_failed_thumbnail_output(&hash);
            assert!(
                fail_marker.exists(),
                "Failure marker file should exist for a broken image at {:?}",
                fail_marker
            );
        });
    }
}
