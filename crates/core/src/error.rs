use serde::Serialize;

use crate::codes::{self, Code};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors crossing crate boundaries. Serialized to the UI as `{ kind, code, message }`.
///
/// The text of a variant is written for the person who reads it. A site that
/// knows exactly what went wrong attaches a code with [`Code::tag`]
/// (`Error::Network(codes::NET_TIMEOUT.tag("…"))`); the tag travels inside the
/// text, so existing `match`es on the variants keep working. Without one the
/// variant's fallback code applies (see [`codes::fallback`]).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", plain("Network problem", .0))]
    Network(String),
    #[error("The server did not accept the saved credentials.")]
    Unauthorized,
    #[error("{}", plain("The server does not allow this", .0))]
    Forbidden(String),
    #[error("{}", plain("Not found", .0))]
    NotFound(String),
    #[error("{}", unsupported(.0))]
    Unsupported(String),
    #[error("{}", plain("Unexpected answer from the server", .0))]
    Protocol(String),
    #[error("{}", plain("Invalid input", .0))]
    Invalid(String),
    #[error("{}", plain("Could not read or write a file", .0))]
    Storage(String),
    #[error("{}", plain("Playback problem", .0))]
    Playback(String),
    #[error("Wrong PIN.")]
    WrongPin,
    #[error("Too many attempts. Try again in {0} s.")]
    PinLocked(u64),
    #[error("{}", plain("Something went wrong", .0))]
    Other(String),
}

/// The text of a tagged message as is; an untagged (technical) one under `lead`.
fn plain(lead: &str, text: &str) -> String {
    match codes::split_tag(text) {
        (Some(_), message) => message.to_owned(),
        (None, raw) if raw.trim().is_empty() => format!("{lead}."),
        (None, raw) => format!("{lead}: {raw}"),
    }
}

fn unsupported(text: &str) -> String {
    match codes::split_tag(text) {
        (Some(_), message) => message.to_owned(),
        (None, feature) => format!("This server does not support {feature}."),
    }
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
            Self::WrongPin => "wrongPin",
            Self::PinLocked(_) => "pinLocked",
            Self::Other(_) => "other",
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Self::Network(m) | Self::Forbidden(m) | Self::NotFound(m) | Self::Unsupported(m) | Self::Protocol(m) | Self::Invalid(m) | Self::Storage(m) | Self::Playback(m) | Self::Other(m) => Some(m),
            Self::Unauthorized | Self::WrongPin | Self::PinLocked(_) => None,
        }
    }

    /// The code shown with the message: the one the site attached, else its kind's.
    pub fn code(&self) -> &'static str {
        let tagged = self.text().and_then(|t| codes::split_tag(t).0);
        tagged.and_then(|id| codes::ALL.iter().find(|c| c.id == id)).map_or_else(|| codes::fallback(self.kind()).id, |c| c.id)
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Error", 3)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

/// Attaches a code to an error that was built without one.
pub trait WithCode<T> {
    fn coded(self, code: Code) -> Result<T>;
}

impl<T> WithCode<T> for Result<T> {
    fn coded(self, code: Code) -> Result<T> {
        self.map_err(|e| e.with_code(code))
    }
}

impl Error {
    /// The same error with `code` attached, unless it already has one.
    pub fn with_code(self, code: Code) -> Self {
        let tag = |m: String| if codes::split_tag(&m).0.is_some() { m } else { code.tag(m) };
        match self {
            Self::Network(m) => Self::Network(tag(m)),
            Self::Forbidden(m) => Self::Forbidden(tag(m)),
            Self::NotFound(m) => Self::NotFound(tag(m)),
            Self::Unsupported(m) => Self::Unsupported(tag(m)),
            Self::Protocol(m) => Self::Protocol(tag(m)),
            Self::Invalid(m) => Self::Invalid(tag(m)),
            Self::Storage(m) => Self::Storage(tag(m)),
            Self::Playback(m) => Self::Playback(tag(m)),
            Self::Other(m) => Self::Other(tag(m)),
            same @ (Self::Unauthorized | Self::WrongPin | Self::PinLocked(_)) => same,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tagged_error_shows_its_message_and_reports_its_code() {
        let e = Error::Network(codes::NET_TIMEOUT.tag("The server took too long to answer."));
        assert_eq!(e.to_string(), "The server took too long to answer.");
        assert_eq!(e.code(), "FLK-NET-002");
    }

    #[test]
    fn an_untagged_error_falls_back_to_its_kind() {
        let e = Error::Storage("disk full".into());
        assert_eq!(e.code(), "FLK-STO-000");
        assert_eq!(e.to_string(), "Could not read or write a file: disk full");
        assert_eq!(Error::Unauthorized.code(), "FLK-AUTH-001");
        assert_eq!(Error::PinLocked(5).code(), "FLK-PROF-002");
    }

    #[test]
    fn with_code_keeps_a_code_that_is_already_there() {
        let e = Error::Invalid(codes::PROF_NAME.tag("A profile needs a name.")).with_code(codes::GEN_INVALID);
        assert_eq!(e.code(), "FLK-PROF-006");
        assert_eq!(Error::Invalid("x".into()).with_code(codes::GEN_INVALID).code(), "FLK-GEN-001");
    }

    #[test]
    fn every_code_in_the_table_is_reachable_by_id() {
        let e = Error::Other(codes::UI_CRASH.tag("x"));
        assert_eq!(e.code(), "FLK-UI-001");
    }
}
