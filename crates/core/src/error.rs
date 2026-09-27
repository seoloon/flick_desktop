use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors crossing crate boundaries. Serialized to the UI as `{ kind, message }`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("the server rejected the credentials")]
    Unauthorized,
    #[error("permission denied by the server: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0} is not supported by this server")]
    Unsupported(String),
    #[error("unexpected server response: {0}")]
    Protocol(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("playback error: {0}")]
    Playback(String),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Network(_) => "network",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "notFound",
            Self::Unsupported(_) => "unsupported",
            Self::Protocol(_) => "protocol",
            Self::Invalid(_) => "invalid",
            Self::Storage(_) => "storage",
            Self::Playback(_) => "playback",
            Self::Other(_) => "other",
        }
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Error", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}
