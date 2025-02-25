use std::{fs::{self, File}, io::BufWriter, path::PathBuf};
use jxl_oxide::integration::JxlDecoder;
use url::Url;
use std::path::Path;
use image::{DynamicImage, ImageError, RgbaImage};
use png::Encoder;
use image::error::{DecodingError, ImageFormatHint};


use crate::ThumbnailSize;


/// Parses the input file and returns a DynamicImage.
pub fn parse_file(input: &str) -> Result<DynamicImage, ImageError> {
    let path = Path::new(input);

    // Check if file exists.
    if !path.exists() {
        return Err(ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File {} not found", input),
        )));
    }

    // Determine the file extension to decide how to parse.
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "jxl" => {
            let file = File::open(input)?;
            // Initialize the JxlDecoder.
            let decoder = JxlDecoder::new(file).map_err(|e| {
                ImageError::Decoding(DecodingError::new(
                    ImageFormatHint::PathExtension("jxl".into()),
                    e,
                ))
            })?;
            // Convert the decoded image into a DynamicImage.
            DynamicImage::from_decoder(decoder)
        },
        _ => {
            // Default to image-rs open
            image::open(input)
        }
    }
}

/// Gets the thumbnail output path usingg hash and size
/// The output path format is "{cache_dir}/thumbnails/{size}/{md5_hash}.png"
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

pub fn get_file_uri(input: &str) -> String {
    // Attempt to canonicalize the input to get the full file path.
    let canonical = std::fs::canonicalize(input).unwrap_or_else(|_| PathBuf::from(input));
    // Create a file URL from the canonical path.
    let url = Url::from_file_path(&canonical)
        .expect("Failed to convert file path to URL");
    
    url.to_string()
}

pub fn write_out_thumbnail(
    image_path: &str,
    img: DynamicImage,
    source_image_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(image_path)?;
    
    let rgba_image: RgbaImage = img.to_rgba8();
    let (width, height) = rgba_image.dimensions();
    let buffer = rgba_image.into_raw();

    let ref mut w = BufWriter::new(file);
    let mut encoder = Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    encoder.add_text_chunk("Software".to_string(), "Thumbnailify".to_string())?;
    
    let uri = get_file_uri(source_image_path);
    encoder.add_text_chunk("Thumb::URI".to_string(), uri)?;

    let metadata = fs::metadata(source_image_path)?;
    
    let size = metadata.len();
    encoder.add_text_chunk("Thumb::Size".to_string(), size.to_string())?;

    let mtime = metadata.modified()?.elapsed()?.as_secs();
    encoder.add_text_chunk("Thumb::MTime".to_string(), mtime.to_string())?;

    let mut writer = encoder.write_header()?;
    writer.write_image_data(&buffer)?;

    Ok(())
}