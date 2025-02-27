use log::debug;
use md5::{Md5, Digest};

/// Computes the MD5 hash for the given input file path.
pub fn compute_hash(input: &str) -> String {
    debug!("Computing MD5 hash for input: {}", input);
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let hash = format!("{:x}", result);

    debug!("MD5 hash for input={} is {}", input, hash);
    hash
}
