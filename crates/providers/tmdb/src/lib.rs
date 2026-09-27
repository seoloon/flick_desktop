//! TMDB client for person pages: biographies, photos, filmographies, and
//! the cast of a title (to tell namesakes apart). Pure: no cache, no key
//! storage — `app` owns both.

mod dto;

use chrono::NaiveDate;
use oneshot_core::media::ItemKind;
use oneshot_core::person::{KnownFor, PersonDetails, PersonInfo, PersonPhoto, TmdbUse};
use oneshot_core::text::normalize_name;
use oneshot_core::{Error, Result};
use oneshot_net::reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

const API: &str = "https://api.themoviedb.org/3/";
const IMAGES: &str = "https://image.tmdb.org/t/p/";
const IMAGE_SIZES: [&str; 3] = ["w185", "h632", "w342"];
const FALLBACK_LANGUAGE: &str = "en-US";
/// Talk shows and documentaries list people as themselves.
const SELF_ROLES: [&str; 4] = ["self", "himself", "herself", "themselves"];

#[derive(Clone)]
enum Key {
    /// v3: `api_key` query parameter (masked by `oneshot_net::redact`).
    V3(String),
    /// v4 read access token: `Authorization: Bearer`.
    V4(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleKind {
    Movie,
    Tv,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersonRef {
    pub id: u64,
    pub name: String,
    pub popularity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TmdbPerson {
    pub id: u64,
    pub name: String,
    pub biography: Option<String>,
    pub birthday: Option<NaiveDate>,
    pub deathday: Option<NaiveDate>,
    pub place_of_birth: Option<String>,
    pub department: Option<String>,
    pub profile_path: Option<String>,
    pub known_for: Vec<KnownFor>,
}

#[derive(Clone)]
pub struct Tmdb {
    http: Client,
    key: Key,
    base: Url,
}

/// Never prints the key: one `{:?}` in a log line must not leak it.
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::V3(_) => "V3(***)",
            Self::V4(_) => "V4(***)",
        })
    }
}

impl std::fmt::Debug for Tmdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tmdb").field("key", &self.key).field("base", &self.base.as_str()).finish()
    }
}

fn parse_key(key: &str) -> Result<Key> {
    let key = key.trim();
    if key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(Key::V3(key.to_owned()))
    } else if key.split('.').count() == 3 && key.len() > 40 && !key.contains(char::is_whitespace) {
        Ok(Key::V4(key.to_owned()))
    } else {
        Err(Error::Invalid("not a TMDB API key (v3 key or v4 read access token)".into()))
    }
}

fn date(s: &Option<String>) -> Option<NaiveDate> {
    s.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

impl Tmdb {
    pub fn new(http: Client, key: &str) -> Result<Self> {
        Ok(Self { http, key: parse_key(key)?, base: Url::parse(API).expect("static url") })
    }

    /// For tests: point at a mock TMDB (must end with `/3/`).
    pub fn with_base(mut self, base: Url) -> Self {
        self.base = base;
        self
    }

    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<RequestBuilder> {
        let mut url = oneshot_net::join(&self.base, path)?;
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in query {
                q.append_pair(k, v);
            }
            if let Key::V3(k) = &self.key {
                q.append_pair("api_key", k);
            }
        }
        let rb = self.http.get(url).header("Accept", "application/json");
        Ok(match &self.key {
            Key::V4(t) => rb.bearer_auth(t),
            Key::V3(_) => rb,
        })
    }

    async fn json<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        oneshot_net::json(self.get(path, query)?.send().await.map_err(oneshot_net::map_err)?).await
    }

    /// Validates the key (Settings › Metadata › Test).
    pub async fn check(&self) -> Result<()> {
        let resp = self.get("authentication", &[])?.send().await.map_err(oneshot_net::map_err)?;
        oneshot_net::ensure_ok(resp).await.map(drop)
    }

    pub async fn person(&self, id: u64, language: &str) -> Result<TmdbPerson> {
        let p: dto::Person = self
            .json(&format!("person/{id}"), &[("language", language), ("append_to_response", "combined_credits")])
            .await?;
        let mut person = map_person(p);
        if person.biography.is_none() && language != FALLBACK_LANGUAGE {
            // Best-effort: the first answer stands on its own.
            match self.json::<dto::Person>(&format!("person/{id}"), &[("language", FALLBACK_LANGUAGE)]).await {
                Ok(en) => person.biography = non_empty(en.biography),
                Err(e) => tracing::debug!(target: "provider", "TMDB English biography of {id}: {e}"),
            }
        }
        Ok(person)
    }

    pub async fn cast_of(&self, kind: TitleKind, tmdb_id: &str) -> Result<Vec<PersonRef>> {
        let path = match kind {
            TitleKind::Movie => format!("movie/{tmdb_id}/credits"),
            TitleKind::Tv => format!("tv/{tmdb_id}/aggregate_credits"),
        };
        let c: dto::Cast = self.json(&path, &[]).await?;
        Ok(c.cast.into_iter().chain(c.crew).map(hit).collect())
    }

    pub async fn search_person(&self, name: &str) -> Result<Vec<PersonRef>> {
        let s: dto::Search = self.json("search/person", &[("query", name)]).await?;
        Ok(s.results.into_iter().map(hit).collect())
    }

    /// The TMDB id of the person named `name`: found in the cast of the
    /// title the person was opened from (tells namesakes apart, even when
    /// the server keeps one record per name); else a TMDB id the server
    /// knows; else the most popular namesake. A cast that cannot be read
    /// falls through to the next step.
    pub async fn identify(&self, name: &str, from: Option<(TitleKind, &str)>, known: Option<&str>) -> Result<Option<u64>> {
        if let Some((kind, id)) = from {
            match self.cast_of(kind, id).await {
                Ok(cast) => {
                    if let Some(found) = pick_by_name(&cast, name) {
                        return Ok(Some(found));
                    }
                }
                Err(e) => tracing::debug!(target: "provider", "TMDB cast of {id}: {e}"),
            }
        }
        if let Some(id) = known.and_then(|k| k.parse().ok()) {
            return Ok(Some(id));
        }
        Ok(pick_by_name(&self.search_person(name).await?, name))
    }
}

/// "Self", "Self - Host", "Himself (archive footage)"… but not "Selma".
fn plays_oneself(role: &str) -> bool {
    let first = role.split(|c: char| !c.is_alphabetic()).next().unwrap_or_default().to_lowercase();
    SELF_ROLES.contains(&first.as_str())
}

fn hit(h: dto::PersonHit) -> PersonRef {
    PersonRef { id: h.id, name: h.name, popularity: h.popularity }
}

/// The most popular person whose normalised name equals `name`.
pub fn pick_by_name(people: &[PersonRef], name: &str) -> Option<u64> {
    let key = normalize_name(name);
    people
        .iter()
        .filter(|p| normalize_name(&p.name) == key)
        .max_by(|a, b| a.popularity.total_cmp(&b.popularity))
        .map(|p| p.id)
}

fn map_person(p: dto::Person) -> TmdbPerson {
    let credits = p.combined_credits.unwrap_or_default();
    let mut known: Vec<KnownFor> = Vec::new();
    for (c, crew) in credits.cast.into_iter().map(|c| (c, false)).chain(credits.crew.into_iter().map(|c| (c, true))) {
        let kind = match c.media_type.as_deref() {
            Some("movie") => ItemKind::Movie,
            Some("tv") => ItemKind::Series,
            _ => continue,
        };
        let role = if crew { non_empty(c.job) } else { non_empty(c.character) };
        if !crew && role.as_deref().is_some_and(plays_oneself) {
            continue;
        }
        let tmdb_id = c.id.to_string();
        if known.iter().any(|k| k.kind == kind && k.tmdb_id == tmdb_id) {
            continue;
        }
        let released = if kind == ItemKind::Movie { &c.release_date } else { &c.first_air_date };
        known.push(KnownFor {
            kind,
            title: c.title.or(c.name).unwrap_or_default(),
            year: date(released).map(|d| chrono::Datelike::year(&d)),
            role,
            poster: c.poster_path,
            tmdb_id,
            vote_count: c.vote_count,
        });
    }
    known.sort_by_key(|k| std::cmp::Reverse(k.vote_count));
    TmdbPerson {
        id: p.id,
        name: p.name,
        biography: non_empty(p.biography),
        birthday: date(&p.birthday),
        deathday: date(&p.deathday),
        place_of_birth: non_empty(p.place_of_birth),
        department: non_empty(p.known_for_department),
        profile_path: p.profile_path,
        known_for: known,
    }
}

/// `https://image.tmdb.org/t/p/<size>/<file>` for `"<size>/<file>"`, only
/// for the sizes Flick uses and a plain TMDB file name.
pub fn image_url(rest: &str) -> Option<Url> {
    let (size, file) = rest.split_once('/')?;
    let (stem, ext) = file.rsplit_once('.')?;
    let ok = IMAGE_SIZES.contains(&size)
        && matches!(ext, "jpg" | "png")
        && !stem.is_empty()
        && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then(|| Url::parse(&format!("{IMAGES}{size}/{file}")).ok()).flatten()
}

/// The person page: TMDB first, the server's own data where TMDB has none.
pub fn merge_details(name: &str, server: Option<&PersonInfo>, tmdb: Option<&TmdbPerson>, used: TmdbUse) -> PersonDetails {
    PersonDetails {
        name: tmdb.map(|t| t.name.clone()).or_else(|| server.map(|s| s.name.clone())).unwrap_or_else(|| name.to_owned()),
        department: tmdb.and_then(|t| t.department.clone()),
        biography: tmdb.and_then(|t| t.biography.clone()).or_else(|| server.and_then(|s| s.overview.clone())),
        birth: tmdb.and_then(|t| t.birthday).or_else(|| server.and_then(|s| s.birth)),
        death: tmdb.and_then(|t| t.deathday).or_else(|| server.and_then(|s| s.death)),
        birthplace: tmdb.and_then(|t| t.place_of_birth.clone()).or_else(|| server.and_then(|s| s.birthplace.clone())),
        photo: tmdb
            .and_then(|t| t.profile_path.clone())
            .map(|path| PersonPhoto::Tmdb { path })
            .or_else(|| server.and_then(|s| s.image.clone()).map(|image| PersonPhoto::Server { image })),
        known_for: tmdb.map(|t| t.known_for.clone()).unwrap_or_default(),
        tmdb: used,
    }
}
