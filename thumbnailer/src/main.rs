use clap::Parser;
use image::{DynamicImage, ImageError};
use std::process;
use thumbnailify::thumbnail::{generate_thumbnail, parse_file};

/// Thumbnail images
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Input image file
    #[arg(value_name = "INPUT FILE")]
    input: String,

    /// Output image file
    #[arg(value_name = "OUTPUT FILE")]
    output: String,

    /// Size of the thumbnail in pixels
    #[arg(short, long, default_value_t = 128)]
    size: u32,
}

fn run(args: Args) -> Result<(), ImageError> {
    // Open the input image.
    let img: DynamicImage = parse_file(&args.input)?;

    // Generate the thumbnail using the provided size.
    // We're calling the helper function from your library.
    let thumb = generate_thumbnail(&img, args.size);

    // Save the thumbnail to the specified output file.
    thumb.save(&args.output)?;

    Ok(())
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
