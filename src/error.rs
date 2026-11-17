use std::path::PathBuf;

/// Everything that can go wrong in the Kit.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// JSON could not be read or written.
    #[error("{context}: {source}")]
    Json {
        /// What was being read.
        context: String,
        /// The underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// The profile is missing something or is inconsistent.
    #[error("profile: {0}")]
    Profile(String),
    /// The model command failed or answered badly.
    #[error("model: {0}")]
    Model(String),
    /// The run spent more than its cap.
    #[error("spend ${spent:.2} passed the cap ${cap:.2}")]
    CostCap {
        /// What the run spent, at list price.
        spent: f64,
        /// The cap from the profile.
        cap: f64,
    },
    /// A deterministic check failed and the retries are used up.
    #[error("{0}")]
    Gate(String),
    /// The command was used wrongly, or its precondition does not hold.
    #[error("{0}")]
    Usage(String),
    /// A git command failed.
    #[error("git: {0}")]
    Git(String),
    /// Publishing was refused or failed.
    #[error("publish: {0}")]
    Publish(String),
    /// A proposal branch is already open on the remote.
    #[error("a proposal branch is already open on the remote; merge or delete it first")]
    ProposalOpen,
}

/// The Kit's result type.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// The process exit code for this error.
    ///
    /// ```
    /// use post_kit::Error;
    /// assert_eq!(Error::Usage("x".into()).exit_code(), 64);
    /// assert_eq!(Error::Gate("x".into()).exit_code(), 73);
    /// assert_eq!(Error::ProposalOpen.exit_code(), 65);
    /// ```
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) | Error::Profile(_) => 64,
            Error::Gate(_) | Error::CostCap { .. } => 73,
            Error::ProposalOpen => 65,
            _ => 1,
        }
    }
}

pub(crate) fn io_err(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}

pub(crate) fn json_err(context: impl Into<String>) -> impl FnOnce(serde_json::Error) -> Error {
    let context = context.into();
    move |source| Error::Json { context, source }
}
