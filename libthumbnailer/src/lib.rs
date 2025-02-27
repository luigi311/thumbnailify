pub mod thumbnail;
pub mod file;
pub mod error;

// Re-export key items for easier access.
pub use thumbnail::generate_thumbnail;
pub use file::{parse_file, write_out_thumbnail};
pub use error::ThumbnailError;