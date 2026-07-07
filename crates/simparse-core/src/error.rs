use thiserror::Error;

pub type Result<T> = std::result::Result<T, SimparseError>;

#[derive(Debug, Error)]
pub enum SimparseError {
    #[error("unsupported file format for {0}")]
    UnsupportedFormat(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Parse(String),
    #[error("hdf5 error: {0}")]
    Hdf5(String),
}
