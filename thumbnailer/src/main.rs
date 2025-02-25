use clap::Parser;
use image::DynamicImage;
use std::process;
use thumbnailify::{generate_thumbnail, parse_file, write_out_thumbnail, ThumbnailError};

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

fn run(args: Args) -> Result<(), ThumbnailError> {
    // Open the input image.
    let img: DynamicImage = parse_file(&args.input)?;

    // Generate the thumbnail using the provided size.
    // We're calling the helper function from your library.
    let thumb: DynamicImage = generate_thumbnail(&img, args.size)?;

    // Save the thumbnail to the specified output file.
    write_out_thumbnail(&args.output, thumb, &args.input).expect("Failed to write thumbnail");

    Ok(())
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
