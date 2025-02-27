use std::fs::File;
use std::path::Path;
use std::time::UNIX_EPOCH;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, GenericImageView};
use png::Decoder;

use crate::error::ThumbnailError;


/// Checks whether the thumbnail file at `thumb_path` is up to date with respect
/// to the source image at `source_path`. It verifies two metadata fields in the PNG:
/// 
/// - "Thumb::MTime": the source file's modification time (in seconds since UNIX_EPOCH)
/// - "Thumb::Size": the source file's size in bytes (only checked if present)
///
/// Returns true if "Thumb::MTime" is present and matches the source file's modification time,
/// and if "Thumb::Size" is present it must match the source file's size.
pub fn is_thumbnail_up_to_date(thumb_path: &Path, source_path: &str) -> bool {
    let file = match File::open(thumb_path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let decoder = Decoder::new(file);
    let reader = match decoder.read_info() {
        Ok(r) => r,
        Err(_) => return false,
    };

    let texts = &reader.info().uncompressed_latin1_text;

    let thumb_mtime_str = match texts.iter().find(|c| c.keyword == "Thumb::MTime") {
        Some(c) => &c.text,
        None => return false,
    };
    let thumb_mtime = thumb_mtime_str.parse::<u64>().unwrap_or(0);

    let source_metadata = match std::fs::metadata(source_path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let source_modified_time = match source_metadata.modified() {
        Ok(mt) => mt.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        Err(_) => return false,
    };

    if thumb_mtime != source_modified_time {
        return false;
    }

    if let Some(chunk) = texts.iter().find(|c| c.keyword == "Thumb::Size") {
        let thumb_size = chunk.text.parse::<u64>().unwrap_or(0);
        let source_file_size = source_metadata.len();
        if thumb_size != source_file_size {
            return false;
        }
    }

    true
}

/// Resizes the given image using the provided max pixel size for its smallest dimension,
/// and returns the scaled-down image. Uses a fast filter (Triangle) for downsizing.
pub fn generate_thumbnail(
    img: &DynamicImage,
    max_dimension: u32,
) -> Result<DynamicImage, ThumbnailError> {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return Err(ThumbnailError::Image(image::ImageError::Parameter(
            image::error::ParameterError::from_kind(
                image::error::ParameterErrorKind::Generic("Source image has no size.".into()),
            ),
        )));
    }

    let largest_side = width.max(height) as f32;
    let scale = max_dimension as f32 / largest_side;
    let dst_width = (width as f32 * scale).round() as u32;
    let dst_height = (height as f32 * scale).round() as u32;

    let mut dst_image = DynamicImage::new(dst_width, dst_height, img.color());

    let mut resizer = Resizer::new();

    resizer.resize(
        img,
        &mut dst_image,
        &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Box)),
    )?;

    Ok(dst_image)
}


