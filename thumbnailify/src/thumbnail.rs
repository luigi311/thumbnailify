use fast_image_resize::{ResizeAlg, ResizeOptions, Resizer};
use image::DynamicImage;
use image::{GenericImageView, ImageError};


use crate::hash::compute_hash;
use crate::sizes::ThumbnailSize;
use crate::file::{get_file_uri, get_thumbnail_hash_output, parse_file, write_out_thumbnail};

/// Resizes the given image using the provided max pixel size for its smallest dimension,
/// and returns the scaled-down image. Uses a fast filter (Triangle) for downsizing.
pub fn generate_thumbnail(img: &DynamicImage, max_dimension: u32) -> DynamicImage {
    // Convert the input image to RGBA8.
    let src_image = img;
    let (width, height) = src_image.dimensions();

    // Calculate new dimensions while maintaining the aspect ratio.
    let scale = max_dimension as f32 / width.max(height) as f32;
    let dst_width = (width as f32 * scale).round() as u32;
    let dst_height = (height as f32 * scale).round() as u32;

    // Create a destination image container with the new dimensions.
    let mut dst_image: DynamicImage= DynamicImage::new(dst_width, dst_height, img.color());

    // Create a Resizer instance with default settings.
    let mut resizer = Resizer::new();

    // Resize the source image (converted into an image view) into the destination image.
    resizer.resize(src_image, &mut dst_image, &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(fast_image_resize::FilterType::Box)))
        .expect("Resizing failed");

    dst_image
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
) -> Result<(), ImageError> {
    let uri = get_file_uri(input);
    // Compute the hash once for the input file.
    let hash = compute_hash(uri);

    // Load the image from the input file only once.
    let img: DynamicImage = parse_file(input)?;
    
    for &size in sizes {
        let output_path = get_thumbnail_hash_output(&hash, size);
        // If the output file already exists, skip this size.
        if output_path.exists() {
            println!("{:?} already exists", output_path);
            continue;
        }

        // Get the maximum dimension for the current size.
        let max_dimension = size.to_dimension();
        // Generate the resized image using our helper function.
        let thumb = generate_thumbnail(&img, max_dimension);

        // Ensure the output directory exists.
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(ImageError::IoError)?;
        }

        // Save the thumbnail.
        write_out_thumbnail(output_path.to_str().unwrap(), thumb, input).unwrap();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_generate_thumbnail_for_all_images() {
        // Set the maximum thumbnail dimension.
        let max_dimension = 128;

        // Directory containing test images.
        let images_dir = "../tests/images";

        // Iterate over each entry in the tests/images directory.
        let entries = fs::read_dir(images_dir)
            .expect("Unable to read tests/images directory");

        for entry in entries {
            let entry = entry.expect("Error reading directory entry");
            let path = entry.path();

            // Only process files (skip directories).
            if path.is_file() {
                let input = path.to_str().expect("Invalid path string");

                // Open the image using the image crate.
                let img = parse_file(input).unwrap();

                // Generate a thumbnail.
                let thumb = generate_thumbnail(&img, max_dimension);

                // Retrieve the thumbnail dimensions.
                let (thumb_width, thumb_height) = thumb.dimensions();
                let min_dimension = thumb_width.min(thumb_height);

                // Assert that the smallest dimension is less than or equal to max_dimension.
                assert!(
                    min_dimension <= max_dimension,
                    "Thumbnail for {} has min dimension {} greater than {}",
                    input,
                    min_dimension,
                    max_dimension
                );

                // Optionally, print dimensions for debugging.
                println!(
                    "Processed {}: original ({}x{}), thumbnail ({}x{})",
                    input,
                    img.width(),
                    img.height(),
                    thumb_width,
                    thumb_height
                );
            }
        }
    }
}
