use thiserror::Error;

/// Errors from reading or writing a `.amalith` container.
#[derive(Debug, Error)]
pub enum IoError {
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip container error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("malformed JSON in container: {0}")]
    Json(#[from] serde_json::Error),
    #[error("container references invalid document structure: {0}")]
    Document(#[from] amalith_core::DocumentError),
}
