use std::fs::File;
use std::path::Path;
use std::time::UNIX_EPOCH;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, GenericImageView};
use png::Decoder;


use crate::hash::compute_hash;
use crate::sizes::ThumbnailSize;
use crate::file::{get_file_uri, get_thumbnail_hash_output, parse_file, write_out_thumbnail};
use crate::ThumbnailError;


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

/// Creates multiple thumbnails for the given image file in one go.
/// This function decodes the image once and then generates each thumbnail using
/// `generate_thumbnail`, saving them in the universal cache directory:
/// "{cache_dir}/thumbnails/{size}/{md5_hash}.png".
///  
/// If the thumbnails already exist, it returns immediately without
/// overwriting them.
///
/// # Arguments
///
/// * `input` - Path to the input image file.
/// * `sizes` - A slice of thumbnail sizes to generate.
///
/// # Errors
///
/// Returns an error if reading or writing any of the images fails.
pub fn create_thumbnails(input: &str, sizes: &[ThumbnailSize]) -> Result<(), ThumbnailError> {
    let uri = get_file_uri(input)?;
    let hash = compute_hash(uri);

    let img = parse_file(input)?;

    for &size in sizes {
        let output_path = get_thumbnail_hash_output(&hash, size);

        if output_path.exists() && is_thumbnail_up_to_date(&output_path, input) {
            eprintln!("{:?} is up-to-date, skipping.", output_path);
            continue;
        }

        let max_dimension = size.to_dimension();
        let thumb = generate_thumbnail(&img, max_dimension)?;

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;  // => ThumbnailError::Io
        }

        let path_str = output_path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in path"))?;

        write_out_thumbnail(path_str, thumb, input)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn test_generate_thumbnail_for_all_images() -> Result<(), ThumbnailError> {
        let max_dimension = 128;
        let images_dir = "../tests/images";

        if !Path::new(images_dir).exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Test images directory '{images_dir}' not found"),
            )
            .into());
        }

        for entry in fs::read_dir(images_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let input = path
                    .to_str()
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid path string"))?;

                let img = parse_file(input)?; // => ThumbnailError
                let thumb = generate_thumbnail(&img, max_dimension)?; // => ThumbnailError

                let (thumb_width, thumb_height) = thumb.dimensions();
                let min_dimension = thumb_width.min(thumb_height);

                assert!(
                    min_dimension <= max_dimension,
                    "Thumbnail for {input} has min dimension {min_dimension} > {max_dimension}"
                );

                eprintln!(
                    "Processed {input}: original ({}x{}), thumbnail ({}x{})",
                    img.width(),
                    img.height(),
                    thumb_width,
                    thumb_height
                );
            }
        }

        Ok(())
    }

    #[test]
    fn test_create_thumbnails() -> Result<(), ThumbnailError> {
        let image = "../tests/images/nasa-4019x4019.png";
        let sizes = [ThumbnailSize::Small, ThumbnailSize::Normal];

        if !Path::new(image).exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Test image '{image}' not found"),
            )
            .into());
        }

        create_thumbnails(image, &sizes)?;
        eprintln!("Thumbnails for '{image}' created successfully for sizes: {sizes:?}");

        Ok(())
    }
}
