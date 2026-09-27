use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Locally-assigned identifier of a configured server connection.
///
/// It is *not* the server's own ID: the same physical server may be added
/// twice with different accounts, and each connection must stay distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
#[serde(transparent)]
pub struct ServerId(pub Uuid);

impl ServerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ServerId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Globally unique reference to an item: which server connection + the
/// provider's own key (Jellyfin GUID, Plex ratingKey).
///
/// Serialized as `"<server-uuid>:<provider-key>"` so the UI can treat it as an
/// opaque string key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "string"))]
pub struct ItemRef {
    pub server: ServerId,
    pub key: String,
}

impl ItemRef {
    pub fn new(server: ServerId, key: impl Into<String>) -> Self {
        Self { server, key: key.into() }
    }
}

impl fmt::Display for ItemRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.server, self.key)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid item reference `{0}`")]
pub struct ParseItemRefError(String);

impl std::str::FromStr for ItemRef {
    type Err = ParseItemRefError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (server, key) = s.split_once(':').ok_or_else(|| ParseItemRefError(s.to_owned()))?;
        let server = Uuid::parse_str(server).map_err(|_| ParseItemRefError(s.to_owned()))?;
        if key.is_empty() {
            return Err(ParseItemRefError(s.to_owned()));
        }
        Ok(Self { server: ServerId(server), key: key.to_owned() })
    }
}

impl Serialize for ItemRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ItemRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ref_roundtrip_keeps_colons_in_key() {
        let r = ItemRef::new(ServerId::new(), "abc:def");
        let parsed: ItemRef = r.to_string().parse().unwrap();
        assert_eq!(parsed, r);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<ItemRef>(&json).unwrap(), r);
    }

    #[test]
    fn item_ref_rejects_garbage() {
        assert!("nope".parse::<ItemRef>().is_err());
        assert!("not-a-uuid:1".parse::<ItemRef>().is_err());
    }
}
