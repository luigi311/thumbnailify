use md5::{Md5, Digest};

/// Computes the MD5 hash for the given input file path.
pub fn compute_hash(input: String) -> String {
    // Create a new Md5 hasher, update it with the URL string, and finalize to get the hash.
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}
