use serde::Serialize;

/// Errors crossing the Rust -> JS boundary.
///
/// Tauri commands must return something `Serialize`, so this flattens to a
/// tagged object the frontend can switch on:
///   `{ "kind": "Network", "message": "..." }`
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("malformed json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("configuration has not been loaded")]
    ConfigNotLoaded,

    #[error("unable to load distribution from remote server or local disk")]
    NoDistribution,

    #[error("no server with id {0}")]
    UnknownServer(String),

    /// A distribution-supplied path that would write outside the directory it
    /// belongs in. Separate from `Other` so the frontend and the logs can tell
    /// a hostile index from an ordinary failure.
    #[error("unsafe path in distribution: {0}")]
    UnsafePath(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "Io",
            Error::Json(_) => "Json",
            Error::Network(_) => "Network",
            Error::ConfigNotLoaded => "ConfigNotLoaded",
            Error::NoDistribution => "NoDistribution",
            Error::UnknownServer(_) => "UnknownServer",
            Error::UnsafePath(_) => "UnsafePath",
            Error::Other(_) => "Other",
        }
    }
}

impl Serialize for Error {
    // NOTE: fully-qualified std::result::Result here — the `Result` alias
    // below shadows it within this module.
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Error", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type Result<T> = std::result::Result<T, Error>;
