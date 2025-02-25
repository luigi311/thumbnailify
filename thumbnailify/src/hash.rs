use std::path::PathBuf;
use url::Url;
use md5::{Md5, Digest};

/// Computes the MD5 hash for the given input file path. It canonicalizes the path,
/// converts it to a file URL, and returns the hexadecimal hash string.
pub fn compute_hash(input: &str) -> String {
    // Attempt to canonicalize the input to get the full file path.
    let canonical = std::fs::canonicalize(input).unwrap_or_else(|_| PathBuf::from(input));
    // Create a file URL from the canonical path.
    let url = Url::from_file_path(&canonical)
        .expect("Failed to convert file path to URL");
    let url_str = url.to_string();

    // Create a new Md5 hasher, update it with the URL string, and finalize to get the hash.
    let mut hasher = Md5::new();
    hasher.update(url_str.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}
