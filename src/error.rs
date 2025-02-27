use thiserror::Error;

/// A unified error type for the thumbnail library.
#[derive(Error, Debug)]
pub enum ThumbnailError {
    /// Wraps errors originating from the `image` crate.
    #[error("Image crate error: {0}")]
    Image(#[from] image::ImageError),

    /// Wraps standard I/O errors.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),    

    #[error("INI config error: {0}")]
    Ini(#[from] ini::Error),

    #[error("File persistence error: {0}")]
    Persist(#[from] tempfile::PersistError),

    #[error("Shell parse error: {0}")]
    Parse(#[from] shell_words::ParseError),
}
