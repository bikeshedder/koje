use std::path::PathBuf;

/// Errors produced by `koje-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid {kind} name {value:?}: {reason}")]
    InvalidName {
        kind: &'static str,
        value: String,
        reason: &'static str,
    },

    #[error("invalid color {0:?}: expected #RRGGBB")]
    InvalidColor(String),

    #[error("invalid size {0:?}: expected a number with optional K, M, G or T suffix")]
    InvalidSize(String),

    #[error("state {0:?} is reserved and cannot be written")]
    ReservedState(String),

    #[error("memory soft limit ({soft}) must be below the hard limit ({hard})")]
    LimitOrder { soft: String, hard: String },

    #[error("could not determine the home directory")]
    NoHomeDir,

    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
