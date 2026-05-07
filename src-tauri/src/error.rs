use serde::Serialize;
use thiserror::Error;

/// Top-level error type for codo. Crosses the Tauri command boundary.
#[derive(Debug, Error)]
pub enum CodoError {
    #[error("database error: {0}")]
    Db(#[from] libsql::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("turso platform api error: {status} — {body}")]
    TursoApi { status: u16, body: String },

    #[error("not onboarded — no Turso credentials in keyring")]
    NotOnboarded,

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("canonicalize: {0}")]
    Canon(#[from] crate::canonicalize::CanonError),

    #[error("validation: {0}")]
    Validation(#[from] crate::validation::ValidationError),

    #[error("internal: {0}")]
    Internal(String),
}

impl CodoError {
    pub fn invalid<S: Into<String>>(msg: S) -> Self {
        CodoError::Invalid(msg.into())
    }
    pub fn internal<S: Into<String>>(msg: S) -> Self {
        CodoError::Internal(msg.into())
    }
}

impl From<anyhow::Error> for CodoError {
    fn from(e: anyhow::Error) -> Self {
        CodoError::Internal(format!("{e:#}"))
    }
}

impl From<serde_json::Error> for CodoError {
    fn from(e: serde_json::Error) -> Self {
        CodoError::Internal(format!("json: {e}"))
    }
}

/// Tauri commands serialize errors as plain strings (with the variant name
/// preserved for grep-ability).
impl Serialize for CodoError {
    fn serialize<S: serde::Serializer>(
        &self,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CodoError>;
