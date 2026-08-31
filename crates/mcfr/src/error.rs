use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Hdf5(rust_hdf5::Hdf5Error),
    Json(serde_json::Error),
    Arrow(arrow_schema::ArrowError),
    Parquet(parquet::errors::ParquetError),
    Zip(zip::result::ZipError),
    Invalid(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Hdf5(error) => write!(formatter, "HDF5 error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Arrow(error) => write!(formatter, "Arrow error: {error}"),
            Self::Parquet(error) => write!(formatter, "Parquet error: {error}"),
            Self::Zip(error) => write!(formatter, "ZIP error: {error}"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rust_hdf5::Hdf5Error> for Error {
    fn from(error: rust_hdf5::Hdf5Error) -> Self {
        Self::Hdf5(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<parquet::errors::ParquetError> for Error {
    fn from(error: parquet::errors::ParquetError) -> Self {
        Self::Parquet(error)
    }
}

impl From<arrow_schema::ArrowError> for Error {
    fn from(error: arrow_schema::ArrowError) -> Self {
        Self::Arrow(error)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
