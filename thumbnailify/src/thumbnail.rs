use std::fs::File;
use std::path::Path;
use std::time::UNIX_EPOCH;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, GenericImageView};
use png::Decoder;


use crate::hash::compute_hash;
use crate::sizes::ThumbnailSize;
use crate::file::{get_failed_thumbnail_output, get_file_uri, get_thumbnail_hash_output, parse_file, write_failed_thumbnail_with_dynamic_image, write_out_thumbnail};
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
pub fn create_thumbnails(
    input: &str,
    sizes: &[ThumbnailSize],
) -> Result<(), ThumbnailError> {
    let uri = get_file_uri(input)?;
    let hash = compute_hash(uri);
    
    // If the fail marker exists and is up to date then return early
    let fail_path = get_failed_thumbnail_output(&hash);
    if fail_path.exists() && is_thumbnail_up_to_date(&fail_path, input) {
        eprintln!("All thumbnails (normal or failure) are up-to-date.");
        return Ok(());
    }


    // Determine which thumbnail sizes need updating.
    // Here, for each size, we check both the normal thumbnail and the fail thumbnail.
    let sizes_to_update: Vec<ThumbnailSize> = sizes
        .iter()
        .cloned()
        .filter(|&size| {
            let output_path = get_thumbnail_hash_output(&hash, size);
            // If the thumbnail doesn't exist or is out-of-date, we need to update.
            !output_path.exists() || !is_thumbnail_up_to_date(&output_path, input)
        })
        .collect();

    // If all thumbnails are up-to-date, we can return early.
    if sizes_to_update.is_empty() {
        eprintln!("All thumbnails are up-to-date.");
        return Ok(());
    }

     // Attempt to parse the image.
     let img = match parse_file(input) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to parse image {}: {:?}. Writing failure marker.", input, e);
            // Ensure the failure marker directory exists.
            if let Some(parent) = fail_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let fail_path_str = fail_path.to_str().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in fail path")
            })?;
            write_failed_thumbnail_with_dynamic_image(fail_path_str, input)?;
            return Err(e);
        }
    };

    // For each thumbnail size that needs updating, try to generate and write it.
    for size in sizes_to_update {
        let output_path = get_thumbnail_hash_output(&hash, size);
        let max_dimension = size.to_dimension();
        match generate_thumbnail(&img, max_dimension) {
            Ok(thumb) => {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let path_str = output_path.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in path")
                })?;
                write_out_thumbnail(path_str, thumb, input)?;
            },
            Err(e) => {
                eprintln!("Thumbnail generation failed for size {:?}: {:?}. Writing failure marker.", size, e);
                if let Some(parent) = fail_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let fail_path_str = fail_path.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in fail path")
                })?;
                write_failed_thumbnail_with_dynamic_image(fail_path_str, input)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    // Utility: remove a file if it exists.
    fn cleanup_file<P: AsRef<Path>>(path: P) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_create_thumbnails_broken() -> Result<(), ThumbnailError> {
        let broken_image = "../tests/images/broken.png";
        let sizes = [ThumbnailSize::Normal];

        // Compute hash and failure marker path.
        let uri = get_file_uri(broken_image)?;
        let hash = compute_hash(uri);
        let fail_path = get_failed_thumbnail_output(&hash);

        // Ensure previous failure marker is removed.
        cleanup_file(&fail_path);

        // Call create_thumbnails on the broken image.
        let _ = create_thumbnails(broken_image, &sizes);

        // Verify that the failure marker now exists.
        assert!(
            fail_path.exists(),
            "Failure marker was not created for broken image {}",
            broken_image
        );

        // Record the modification time of the failure marker.
        let mod_time1 = fs::metadata(&fail_path)?.modified()?;

        // Call create_thumbnails again on the broken image.
        let _ = create_thumbnails(broken_image, &sizes);
        let mod_time2 = fs::metadata(&fail_path)?.modified()?;

        // The modification time should not change since the failure marker is up-to-date.
        assert_eq!(
            mod_time1, mod_time2,
            "Failure marker was updated even though it should be up-to-date."
        );

        // Clean up the failure marker.
        cleanup_file(&fail_path);

        Ok(())
    }

    #[test]
    fn test_create_thumbnails_valid() -> Result<(), ThumbnailError> {
        // List of valid image files to test.
        let valid_images = vec![
            "../tests/images/nasa-4019x4019.png",
            "../tests/images/nasa-4019x4019.jxl",
        ];
        let sizes = [ThumbnailSize::Small, ThumbnailSize::Normal];

        for image in valid_images {
            // Check if the image exists; if not, skip it.
            if !Path::new(image).exists() {
                eprintln!("Test image '{}' not found, skipping.", image);
                continue;
            }

            // Compute hash based on the file URI.
            let uri = get_file_uri(image)?;
            let hash = compute_hash(uri);

            // Clean up any preexisting normal thumbnails and failure markers.
            for &size in &sizes {
                cleanup_file(get_thumbnail_hash_output(&hash, size));
            }
            cleanup_file(get_failed_thumbnail_output(&hash));

            // Generate thumbnails for the image.
            create_thumbnails(image, &sizes)?;

            // Verify that each expected thumbnail exists.
            for &size in &sizes {
                let thumb_path = get_thumbnail_hash_output(&hash, size);
                assert!(
                    thumb_path.exists(),
                    "Thumbnail for size {:?} does not exist for image {}",
                    size,
                    image
                );
            }

            // Record modification times of the generated thumbnails.
            let mut mod_times = Vec::new();
            for &size in &sizes {
                let thumb_path = get_thumbnail_hash_output(&hash, size);
                mod_times.push(fs::metadata(&thumb_path)?.modified()?);
            }

            // Call create_thumbnails again, which should skip updating up-to-date thumbnails.
            create_thumbnails(image, &sizes)?;
            for (&size, &old_mod_time) in sizes.iter().zip(mod_times.iter()) {
                let thumb_path = get_thumbnail_hash_output(&hash, size);
                let new_mod_time = fs::metadata(&thumb_path)?.modified()?;
                assert_eq!(
                    old_mod_time, new_mod_time,
                    "Thumbnail for size {:?} of image {} was recreated unexpectedly.",
                    size, image
                );
            }

            // Cleanup: remove the generated thumbnails and any failure marker.
            for &size in &sizes {
                cleanup_file(get_thumbnail_hash_output(&hash, size));
            }
            cleanup_file(get_failed_thumbnail_output(&hash));
        }

        Ok(())
    }
}
