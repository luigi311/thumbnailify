use std::path::PathBuf;


use image::{DynamicImage, Rgba, RgbaImage};
use libthumbnailer::{write_out_thumbnail, ThumbnailError};

use crate::sizes::ThumbnailSize;

fn get_base_cache_dir() -> PathBuf {
    // Determine the base cache directory using the `dirs` crate.
    // If not available, fallback to "~/.cache".
    dirs::cache_dir().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".cache")
    })
}

/// Gets the thumbnail output path using hash and size.
/// Format: `{cache_dir}/thumbnails/{size}/{md5_hash}.png`
pub fn get_thumbnail_hash_output(hash: &str, size: ThumbnailSize) -> PathBuf {
    // Create a base directory for thumbnails.
    let base_dir = get_base_cache_dir().join("thumbnails");
    // Create the subdirectory based on the thumbnail size.
    let output_dir = base_dir.join(size.to_string());
    let output_file = format!("{}.png", hash);
    output_dir.join(output_file)
}

/// Returns the output path for a failed thumbnail marker.
/// This uses the fails folder under the thumbnails cache.
pub fn get_failed_thumbnail_output(hash: &str) -> PathBuf {
    // Build the application-specific fail directory.
    let fail_dir = get_base_cache_dir().join("thumbnails").join("fail").join("thumbnailify");
    let output_file = format!("{}.png", hash);
    fail_dir.join(output_file)
}

/// Writes a failed thumbnail using an empty (1x1 transparent) DynamicImage.
pub fn write_failed_thumbnail(fail_path: &PathBuf, source_path: &str) -> Result<(), ThumbnailError> {
    let fail_str = fail_path.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid file path",
        )
    })?;

    // Create a 1x1 transparent image.
    let failed_img: DynamicImage = DynamicImage::ImageRgba8(
        RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0]))
    );

    write_out_thumbnail(fail_str, failed_img, source_path)
}
