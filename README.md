# Thumbnailify

Thumbnailify is a Rust library for generating and caching thumbnails for image files using the GNOME thumbnailer approach. It supports multiple MIME types, ensures thumbnails are up to date by comparing metadata, and leverages external commands (with optional sandboxing via Bubblewrap) for secure thumbnail generation.

## Features

- **Thumbnail Generation:** Uses external thumbnailers to generate thumbnails for images.
- **Caching:** Stores thumbnails in the XDG cache directory (with a fallback to `~/.cache`) and checks if the cached thumbnail is up to date.
- **Custom Sizes:** Provides predefined thumbnail sizes conforming to the XDG thumbnail standard.
- **Unified Error Handling:** Implements a unified error type with the `thiserror` crate to handle errors from various sources.
- **Secure Execution:** Optionally uses Bubblewrap for sandboxing external commands.
- **MD5 Hashing:** Generates unique identifiers for thumbnails based on the image file’s URI.

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
thumbnailify = "0.1"
```

## Usage Example

```rust
use thumbnailify::{generate_thumbnail, ThumbnailSize, ThumbnailError};
use std::path::Path;

fn main() -> Result<(), ThumbnailError> {
    // Specify the path to your source image.
    let image_path = Path::new("path/to/your/image.png");
    
    // Generate a thumbnail of "Normal" size.
    let thumbnail_path = generate_thumbnail(image_path, ThumbnailSize::Normal)?;
    
    println!("Thumbnail generated at: {:?}", thumbnail_path);
    
    Ok(())
}
```

## Library Structure

- **`error` Module:**  
  Defines a unified error type (`ThumbnailError`) that wraps errors from libraries such as `image`, `std::io`, `ini`, `tempfile`, `shell_words`, and `png`.

- **`file` Module:**  
  Contains helpers for determining cache directories, writing thumbnails (or failure markers), and converting file paths to URIs.

- **`hash` Module:**  
  Provides an MD5-based function to compute a hash from the image file's URI, ensuring a unique thumbnail name.

- **`sizes` Module:**  
  Offers predefined thumbnail sizes (Small, Normal, Large, XLarge, XXLarge) that correspond to maximum dimensions in pixels.

- **`thumbnailer` Module:**  
  Implements the main logic to generate thumbnails:
  - Reads MIME types and searches for an appropriate `.thumbnailer` file.
  - Replaces tokens in the Exec command with actual parameters.
  - Executes the external command (with Bubblewrap sandboxing if available).
  - Checks if the cached thumbnail is up to date using embedded PNG metadata.

## Running Tests

The library includes tests to verify core functionality. Run the tests using:

```bash
cargo test
```

## Contributing

Contributions are welcome! If you have suggestions, encounter any issues, or would like to add features, please open an issue or submit a pull request.
