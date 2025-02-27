use std::{fs::File, io::BufWriter, path::PathBuf, time::UNIX_EPOCH};


use image::{DynamicImage, Rgba, RgbaImage};
use png::Encoder;
use url::Url;

use crate::{error::ThumbnailError, sizes::ThumbnailSize};

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

/// Attempts to convert the file path into a file URI.
pub fn get_file_uri(input: &str) -> Result<String, ThumbnailError> {
    // Attempt to canonicalize the input to get the full file path.
    // If canonicalize fails, fall back to the raw `input` PathBuf.
    let canonical = std::fs::canonicalize(input).unwrap_or_else(|_| PathBuf::from(input));
    let url = Url::from_file_path(&canonical).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Failed to convert file path to URL",
        )
    })?;
    Ok(url.to_string())
}

/// Writes out the thumbnail as a PNG, embedding:
/// - `Thumb::URI`
/// - `Thumb::Size`
/// - `Thumb::MTime`
pub fn write_out_thumbnail(
    image_path: &str,
    img: DynamicImage,
    source_image_path: &str,
) -> Result<(), ThumbnailError> {
    let file = File::create(image_path)?;

    let rgba_image: RgbaImage = img.to_rgba8();
    let (width, height) = rgba_image.dimensions();
    let buffer = rgba_image.into_raw();

    let mut encoder = Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    encoder.add_text_chunk("Software".to_string(), "Thumbnailify".to_string())
        .map_err(map_png_err)?;

    let uri = get_file_uri(source_image_path)?;
    encoder.add_text_chunk("Thumb::URI".to_string(), uri)
        .map_err(map_png_err)?;

    let metadata = std::fs::metadata(source_image_path)?;

    let size = metadata.len();
    encoder.add_text_chunk("Thumb::Size".to_string(), size.to_string())
        .map_err(map_png_err)?;

    let modified_time = metadata.modified()?;
    let mtime_unix = modified_time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    encoder.add_text_chunk("Thumb::MTime".to_string(), mtime_unix.to_string())
        .map_err(map_png_err)?;

    let mut writer = encoder.write_header().map_err(map_png_err)?;
    writer.write_image_data(&buffer).map_err(map_png_err)?;

    Ok(())
}

// Helper function to map `png::EncodingError` -> `ThumbnailError`.
fn map_png_err(err: png::EncodingError) -> ThumbnailError {
    // We'll convert it to `image::ImageError::IoError`, which then becomes `ThumbnailError::Image(...)`.
    // Or, since we have direct Io errors in our enum, we can do that. But let's keep it simple:
    ThumbnailError::Image(image::ImageError::IoError(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("PNG encoding error: {err}"),
    )))
}
