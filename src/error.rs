use thiserror::Error;

#[derive(Error, Debug)]
pub enum BifrostError {
    #[error("Upstream API error: {0}")]
    Upstream(String),

    #[error("Parsing error: {0}")]
    Parsing(String),

    #[error("Database error: {0}")]
    Database(#[from] mongodb::error::Error),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl From<reqwest::Error> for BifrostError {
    fn from(err: reqwest::Error) -> Self {
        BifrostError::Upstream(err.to_string())
    }
}

impl From<serde_json::Error> for BifrostError {
    fn from(err: serde_json::Error) -> Self {
        BifrostError::Parsing(err.to_string())
    }
}

impl From<std::io::Error> for BifrostError {
    fn from(err: std::io::Error) -> Self {
        BifrostError::Cache(err.to_string())
    }
}
