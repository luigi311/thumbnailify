use std::{fs::File, io::BufWriter, path::PathBuf, time::UNIX_EPOCH};
use jxl_oxide::integration::JxlDecoder;
use url::Url;
use std::path::Path;
use image::{DynamicImage, ImageReader, Limits, RgbaImage};
use png::Encoder;


use crate::{ThumbnailError, ThumbnailSize};


/// Parses the input file and returns a `DynamicImage`.
pub fn parse_file(input: &str) -> Result<DynamicImage, ThumbnailError> {
    let path = Path::new(input);

    // Check if file exists.
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File {input} not found")
        ).into());
    }

    // Determine the file extension to decide how to parse.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let dyn_img = match ext.as_str() {
        "jxl" => {
            let file = File::open(input)?;
            let decoder = JxlDecoder::new(file)
                .map_err(|e| image::ImageError::Decoding(
                    image::error::DecodingError::new(
                        image::error::ImageFormatHint::PathExtension("jxl".into()),
                        e
                    )
                ))?;
            image::DynamicImage::from_decoder(decoder)?
        }
        _ => {
            let mut reader = ImageReader::open(input)?;

            // Set the memory limit to 1GB
            let mut limits = Limits::default();
            limits.max_alloc = Some(1024 * 1024 * 1024);
            reader.limits(limits);

            reader.with_guessed_format()?.decode()?
        }
    };

    Ok(dyn_img)
}

/// Gets the thumbnail output path using hash and size.
/// Format: `{cache_dir}/thumbnails/{size}/{md5_hash}.png`
pub fn get_thumbnail_hash_output(hash: &str, size: ThumbnailSize) -> PathBuf {
    // Determine the base cache directory using the `dirs` crate.
    // If not available, fallback to "~/.cache".
    let base_cache_dir = dirs::cache_dir().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".cache")
    });

    // Create a base directory for thumbnails.
    let base_dir = base_cache_dir.join("thumbnails");
    // Create the subdirectory based on the thumbnail size.
    let output_dir = base_dir.join(size.to_string());
    let output_file = format!("{}.png", hash);
    output_dir.join(output_file)
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