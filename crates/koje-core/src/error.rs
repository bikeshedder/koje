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

    #[error("invalid mount {value:?}: {reason}")]
    InvalidMount { value: String, reason: &'static str },

    #[error("refused mount {value:?}: {reason} (mark it `unsafe = true` to allow)")]
    RefusedMount { value: String, reason: String },

    #[error("mounts {first:?} and {second:?} both name {path}")]
    DuplicateMount {
        path: PathBuf,
        first: String,
        second: String,
    },

    #[error("context {0} does not exist")]
    UnknownContext(crate::name::ContextName),

    #[error("context {0} already exists")]
    ContextExists(crate::name::ContextName),

    #[error("{path} names context {found:?}, but its directory is {expected:?}")]
    ContextNameMismatch {
        path: PathBuf,
        expected: crate::name::ContextName,
        found: crate::name::ContextName,
    },

    #[error("context {context} has no state {state}")]
    UnknownState {
        context: crate::name::ContextName,
        state: crate::name::StateName,
    },

    #[error("context {context} already has a state {state} (remove it first with `koje state rm`)")]
    StateExists {
        context: crate::name::ContextName,
        state: crate::name::StateName,
    },

    #[error("could not determine the home directory")]
    NoHomeDir,

    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to serialize {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },

    #[error("inconsistent state {path}: {issue}")]
    InvalidState {
        path: PathBuf,
        issue: crate::state::StateIssue,
    },

    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
