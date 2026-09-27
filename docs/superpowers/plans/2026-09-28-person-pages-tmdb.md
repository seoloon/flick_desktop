# Pages acteur avec TMDB — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Une page personne (acteur, réalisateur…) avec biographie TMDB, ses titres présents sur tous les serveurs du profil actif, et « Also known for » ; clé TMDB dans Réglages › Metadata.

**Architecture:** Nouveau crate `oneshot-tmdb` (client TMDB pur, testé sur serveur simulé, avec la règle d'identification et la fusion des données). Le contrat `MediaProvider` gagne `person` (fiche serveur) et `person_items` (titres d'une personne) ; Jellyfin et Plex les implémentent, le `Catalog` agrège. `app` câble la clé (trousseau), le cache, deux commandes et une route d'images TMDB restreinte. L'UI ajoute la route `/person/:ref`, rend la distribution cliquable et une section Réglages.

**Tech Stack:** Rust 2024 (reqwest via `oneshot-net`, serde, wiremock), React 19 + TS, TanStack Query, Motion, Norigin (`useTv`/`FocusGroup`), Vitest.

**Spec:** `docs/superpowers/specs/2026-09-28-person-pages-tmdb-design.md`

## Global Constraints

- Clé TMDB : trousseau de l'OS uniquement (entrée `tmdb-key`), jamais dans `settings.json`, jamais renvoyée à la WebView (l'UI ne reçoit qu'un booléen).
- Deux formats de clé : jeton v4 (JWT, contient deux `.`) → en-tête `Authorization: Bearer …` ; clé v3 (32 caractères hexadécimaux) → paramètre `api_key` (déjà masqué par `oneshot_net::redact`).
- Base API `https://api.themoviedb.org/3/`, images `https://image.tmdb.org/t/p/<taille>/<fichier>` avec taille ∈ {`w185`, `h632`, `w342`} et fichier `^[A-Za-z0-9_-]+\.(jpg|png)$` ; toute autre forme refusée.
- Langue : `general.language` sinon `navigator.language`, passée par l'UI ; biographie vide → repli `en-US` pour la biographie seulement.
- Cache : `MetadataCache`, préfixe serveur `"tmdb"`, TTL 7 jours (604 800 s).
- Correspondance des noms : même normalisation que les profils (casse, accents, espaces) — une seule fonction, `oneshot_core::text::normalize_name`.
- « Also known for » : trié par nombre de votes, 20 au plus, sans titres présents sur les serveurs (même identifiant TMDB), sans apparitions « Self/Himself/Herself/Themselves ».
- Attribution affichée dans Réglages › Metadata : « This product uses the TMDB API but is not endorsed or certified by TMDB. » avec le logo TMDB en blanc.
- Textes UI en anglais ; interface monochrome ; tout élément interactif via `useTv` / `Button` / `FocusGroup`.
- Un serveur qui ne sait pas chercher une personne (`Unsupported`) est ignoré sans erreur, comme pour les favoris.
- Commits signés GPG, jamais contournés ; la ligne `Co-Authored-By:` nomme le modèle qui a écrit le commit.
- Vérifs : `cargo test -p <crate>`, `cargo clippy --workspace --all-targets` sans nouvel avertissement, `pnpm --dir ui run typecheck`, `pnpm --dir ui run test`.

## Review Focus

1. **Homonymes** : deux « Chris Evans » ne doivent pas se confondre quand on vient d'un titre connu → test `identify_prefers_the_cast_of_the_title` (Tâche 2).
2. **Clé qui fuit** : la clé ne doit jamais sortir de Rust (réponse de commande, logs, URL d'image) → Tâche 6, `tmdb_status` ne renvoie qu'un booléen, test `v3_key_goes_in_the_query_v4_in_the_header` (Tâche 2).
3. **Proxy d'images détourné** en requête arbitraire → test `image_urls_only_for_tmdb_files` (Tâche 2).
4. **Page vide sans clé / TMDB en panne** : la page doit rester utile (titres des serveurs) → `merge_details` testé sans TMDB (Tâche 2) et UI (Tâche 8).
5. **Serveur Plex dont la recherche d'acteurs a une autre forme** → DTO tolérant + test live manuel (Tâche 4).

---

### Task 1: Core — normalisation partagée, types « personne », contrat provider

**Files:**
- Create: `crates/core/src/text.rs`, `crates/core/src/person.rs`
- Modify: `crates/core/Cargo.toml` (`unicode-normalization = "0.1"`), `crates/core/src/lib.rs`, `crates/core/src/provider.rs`, `crates/storage/src/profiles.rs`, `crates/storage/Cargo.toml` (garder la dépendance si encore utilisée, sinon la retirer)

**Interfaces:**
- Produces: `oneshot_core::text::normalize_name(&str) -> String` ; `oneshot_core::person::{PersonInfo, PersonPhoto, KnownFor, TmdbUse, PersonDetails}` ; `MediaProvider::person(&self, id: &ItemRef) -> Result<PersonInfo>` et `MediaProvider::person_items(&self, name: &str, hint: Option<&ItemRef>) -> Result<Vec<MediaItem>>` (défauts : `Unsupported`).

- [ ] **Step 1: Move `normalize_name` to core** — `crates/core/src/text.rs` :

```rust
//! Text rules shared across crates.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Matching key for a person's name: case, accents and spacing ignored.
pub fn normalize_name(name: &str) -> String {
    let folded: String = name.nfkd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_across_case_accents_and_spaces() {
        assert_eq!(normalize_name("  Antoine "), "antoine");
        assert_eq!(normalize_name("Antoïne"), "antoine");
        assert_eq!(normalize_name("Élodie   Martin"), "elodie martin");
        assert_ne!(normalize_name("Léa"), normalize_name("Leo"));
    }
}
```

In `crates/storage/src/profiles.rs`: delete the local `normalize_name` and its `unicode_normalization` imports, add `pub use oneshot_core::text::normalize_name;` (callers keep working), and move its test into core (above). Add `unicode-normalization = "0.1"` to `crates/core/Cargo.toml`; remove it from storage if nothing else uses it. Add `pub mod text;` and `pub mod person;` to `crates/core/src/lib.rs`.

- [ ] **Step 2: Person types** — `crates/core/src/person.rs` :

```rust
//! People (actors, directors…): what a server knows, and the page the UI
//! shows (server data completed by TMDB).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::ids::ItemRef;
use crate::media::{ExternalIds, ImageRef, ItemKind};

/// A person as a media server describes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonInfo {
    pub id: ItemRef,
    pub name: String,
    pub overview: Option<String>,
    pub birth: Option<NaiveDate>,
    pub death: Option<NaiveDate>,
    pub birthplace: Option<String>,
    pub image: Option<ImageRef>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum PersonPhoto {
    /// From the person's server (served by `oneshot-img`).
    Server { image: ImageRef },
    /// A TMDB file name (`/abc.jpg`), served by `oneshot-img` too.
    Tmdb { path: String },
}

/// A title of the person's TMDB filmography.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct KnownFor {
    /// `Movie` or `Series`.
    pub kind: ItemKind,
    pub title: String,
    pub year: Option<i32>,
    /// Character played, or job for crew.
    pub role: Option<String>,
    /// TMDB poster file (`/abc.jpg`).
    pub poster: Option<String>,
    pub tmdb_id: String,
    pub vote_count: u32,
}

/// Whether TMDB contributed to the page, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum TmdbUse {
    Used,
    NoKey,
    NotFound,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PersonDetails {
    pub name: String,
    pub department: Option<String>,
    pub biography: Option<String>,
    pub birth: Option<NaiveDate>,
    pub death: Option<NaiveDate>,
    pub birthplace: Option<String>,
    pub photo: Option<PersonPhoto>,
    /// TMDB filmography, most voted first (the UI removes titles on the servers).
    pub known_for: Vec<KnownFor>,
    pub tmdb: TmdbUse,
}
```

- [ ] **Step 3: Provider contract** — in `crates/core/src/provider.rs`, after `favorites`:

```rust
    /// What this server knows about a person of its own.
    async fn person(&self, _id: &ItemRef) -> Result<crate::person::PersonInfo> {
        Err(crate::Error::Unsupported("person details".into()))
    }

    /// Movies and series featuring a person, found by `hint` when it is a
    /// person of this server, else by name.
    async fn person_items(&self, _name: &str, _hint: Option<&ItemRef>) -> Result<Vec<MediaItem>> {
        Err(crate::Error::Unsupported("person search".into()))
    }
```

- [ ] **Step 4: Verify and export bindings**

Run: `cargo test -p oneshot-core -p oneshot-storage && cargo build --workspace && cargo test -p oneshot-core --features ts export_bindings`
Expected: PASS; new bindings `PersonPhoto.ts`, `KnownFor.ts`, `TmdbUse.ts`, `PersonDetails.ts` in `ui/src/ipc/bindings/`.

- [ ] **Step 5: Commit** — `Core: shared name matching, person types, provider person methods`.

---

### Task 2: Crate `oneshot-tmdb`

**Files:**
- Create: `crates/providers/tmdb/Cargo.toml`, `crates/providers/tmdb/src/lib.rs`, `crates/providers/tmdb/src/dto.rs`, `crates/providers/tmdb/tests/tmdb.rs`
- Modify: `Cargo.toml` (workspace member `crates/providers/tmdb`, dependency `oneshot-tmdb = { path = "crates/providers/tmdb" }`)

**Interfaces:**
- Consumes: `oneshot_core::{person::*, text::normalize_name, media::{ItemKind, ExternalIds}}`
- Produces:
  - `pub struct Tmdb` : `Tmdb::new(http: Client, key: &str) -> Result<Tmdb>` (refuses an unrecognised key format with `Error::Invalid`), `with_base(Url)`, `check() -> Result<()>`, `person(id: u64, language: &str) -> Result<TmdbPerson>`, `cast_of(kind: TitleKind, tmdb_id: &str) -> Result<Vec<PersonRef>>`, `search_person(name) -> Result<Vec<PersonRef>>`, `identify(name, from: Option<(TitleKind, &str)>, known: Option<&str>) -> Result<Option<u64>>`
  - `pub enum TitleKind { Movie, Tv }`
  - `pub struct PersonRef { id: u64, name: String, popularity: f32 }`
  - `pub struct TmdbPerson { id, name, biography: Option<String>, birthday: Option<NaiveDate>, deathday: Option<NaiveDate>, place_of_birth: Option<String>, department: Option<String>, profile_path: Option<String>, known_for: Vec<KnownFor> }` (Serialize + Deserialize, for the cache)
  - `pub fn pick_by_name(people: &[PersonRef], name: &str) -> Option<u64>`
  - `pub fn image_url(rest: &str) -> Option<Url>` (`rest` = `"w342/abc.jpg"`)
  - `pub fn merge_details(name: &str, server: Option<&PersonInfo>, tmdb: Option<&TmdbPerson>, used: TmdbUse) -> PersonDetails`

- [ ] **Step 1: Cargo manifest**

```toml
[package]
name = "oneshot-tmdb"
description = "TMDB client: people, filmographies, credits (biographies for person pages)"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
oneshot-core.workspace = true
oneshot-net.workspace = true
chrono.workspace = true
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
url.workspace = true

[dev-dependencies]
tokio.workspace = true
wiremock.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write the failing tests** — `crates/providers/tmdb/tests/tmdb.rs` :

```rust
use oneshot_core::Error;
use oneshot_core::media::ItemKind;
use oneshot_core::person::{PersonInfo, TmdbUse};
use oneshot_core::{ItemRef, ServerId};
use oneshot_tmdb::{PersonRef, TitleKind, Tmdb, image_url, merge_details, pick_by_name};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const V3: &str = "0123456789abcdef0123456789abcdef";
const V4: &str = "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJ4In0.c2lnbmF0dXJl";

fn tmdb(server: &MockServer, key: &str) -> Tmdb {
    Tmdb::new(oneshot_net::reqwest::Client::new(), key).unwrap().with_base(Url::parse(&format!("{}/3/", server.uri())).unwrap())
}

fn person_json(bio: &str) -> serde_json::Value {
    serde_json::json!({
        "id": 31, "name": "Tom Hanks", "biography": bio, "birthday": "1956-07-09", "deathday": null,
        "place_of_birth": "Concord, California, USA", "known_for_department": "Acting", "profile_path": "/tom.jpg",
        "combined_credits": {
            "cast": [
                { "id": 13, "media_type": "movie", "title": "Forrest Gump", "release_date": "1994-06-23", "character": "Forrest", "poster_path": "/fg.jpg", "vote_count": 27000 },
                { "id": 862, "media_type": "movie", "title": "Toy Story", "release_date": "1995-10-30", "character": "Woody (voice)", "poster_path": "/ts.jpg", "vote_count": 18000 },
                { "id": 1, "media_type": "tv", "name": "Late Show", "first_air_date": "2015-09-08", "character": "Self", "vote_count": 90000 },
                { "id": 13, "media_type": "movie", "title": "Forrest Gump", "release_date": "1994-06-23", "character": "Forrest (archive)", "vote_count": 27000 }
            ],
            "crew": [
                { "id": 4415, "media_type": "movie", "title": "That Thing You Do!", "release_date": "1996-10-04", "job": "Director", "vote_count": 600 }
            ]
        }
    })
}

#[test]
fn keys_are_recognised_by_format() {
    let http = oneshot_net::reqwest::Client::new();
    assert!(Tmdb::new(http.clone(), V3).is_ok());
    assert!(Tmdb::new(http.clone(), V4).is_ok());
    assert!(matches!(Tmdb::new(http, "not a key"), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn v3_key_goes_in_the_query_v4_in_the_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/authentication")).and(query_param("api_key", V3))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "success": true }))).expect(1).mount(&server).await;
    Mock::given(method("GET")).and(path("/3/authentication")).and(header("Authorization", format!("Bearer {V4}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "success": true }))).expect(1).mount(&server).await;
    tmdb(&server, V3).check().await.unwrap();
    tmdb(&server, V4).check().await.unwrap();
}

#[tokio::test]
async fn an_invalid_key_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/authentication")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    assert!(matches!(tmdb(&server, V3).check().await, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn person_maps_the_filmography_most_voted_first() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/person/31")).and(query_param("language", "fr-FR"))
        .respond_with(ResponseTemplate::new(200).set_body_json(person_json("Acteur américain."))).mount(&server).await;
    let p = tmdb(&server, V3).person(31, "fr-FR").await.unwrap();
    assert_eq!(p.biography.as_deref(), Some("Acteur américain."));
    assert_eq!(p.birthday.unwrap().to_string(), "1956-07-09");
    let titles: Vec<&str> = p.known_for.iter().map(|k| k.title.as_str()).collect();
    assert_eq!(titles, ["Forrest Gump", "Toy Story", "That Thing You Do!"], "no talk-show self, no duplicate");
    assert_eq!(p.known_for[0].kind, ItemKind::Movie);
    assert_eq!(p.known_for[0].year, Some(1994));
    assert_eq!(p.known_for[2].role.as_deref(), Some("Director"));
}

#[tokio::test]
async fn an_empty_biography_falls_back_to_english() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/person/31")).and(query_param("language", "fr-FR"))
        .respond_with(ResponseTemplate::new(200).set_body_json(person_json(""))).mount(&server).await;
    Mock::given(method("GET")).and(path("/3/person/31")).and(query_param("language", "en-US"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 31, "name": "Tom Hanks", "biography": "American actor." }))).mount(&server).await;
    let p = tmdb(&server, V3).person(31, "fr-FR").await.unwrap();
    assert_eq!(p.biography.as_deref(), Some("American actor."));
    assert_eq!(p.known_for.len(), 3, "the filmography stays from the first call");
}

#[tokio::test]
async fn identify_prefers_the_cast_of_the_title() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/movie/100/credits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "cast": [{ "id": 16828, "name": "Chris Evans", "popularity": 30.0 }], "crew": [] }))).mount(&server).await;
    Mock::given(method("GET")).and(path("/3/search/person"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "results": [
            { "id": 999, "name": "Chris Evans", "popularity": 90.0 }] }))).mount(&server).await;
    let t = tmdb(&server, V3);
    assert_eq!(t.identify("chris  evans", Some((TitleKind::Movie, "100")), None).await.unwrap(), Some(16828), "the title's cast wins over popularity");
    assert_eq!(t.identify("Chris Evans", None, None).await.unwrap(), Some(999), "else the most popular namesake");
    assert_eq!(t.identify("Chris Evans", None, Some("42")).await.unwrap(), Some(42), "a TMDB id the server knows is used as is");
}

#[test]
fn pick_by_name_takes_the_most_popular_exact_match() {
    let people = [
        PersonRef { id: 1, name: "Léa Seydoux".into(), popularity: 5.0 },
        PersonRef { id: 2, name: "Lea Seydoux".into(), popularity: 9.0 },
        PersonRef { id: 3, name: "Léa".into(), popularity: 50.0 },
    ];
    assert_eq!(pick_by_name(&people, "LÉA SEYDOUX"), Some(2));
    assert_eq!(pick_by_name(&people, "Nobody"), None);
}

#[test]
fn image_urls_only_for_tmdb_files() {
    assert_eq!(image_url("w342/abc_D-1.jpg").unwrap().as_str(), "https://image.tmdb.org/t/p/w342/abc_D-1.jpg");
    assert!(image_url("h632/x.png").is_some());
    for bad in ["original/abc.jpg", "w342/../x.jpg", "w342/abc.gif", "w342/a/b.jpg", "w342/", "https://evil/x.jpg", "w342/abc.jpg?x=1"] {
        assert!(image_url(bad).is_none(), "{bad}");
    }
}

#[test]
fn merged_details_prefer_tmdb_and_keep_server_data_without_it() {
    let server = PersonInfo {
        id: ItemRef::new(ServerId::new(), "p1"),
        name: "Tom Hanks".into(),
        overview: Some("Server bio".into()),
        birth: None,
        death: None,
        birthplace: None,
        image: None,
        external_ids: Default::default(),
    };
    let alone = merge_details("Tom Hanks", Some(&server), None, TmdbUse::NoKey);
    assert_eq!(alone.biography.as_deref(), Some("Server bio"));
    assert_eq!(alone.tmdb, TmdbUse::NoKey);
    assert!(alone.known_for.is_empty());
    let nothing = merge_details("Someone", None, None, TmdbUse::Unavailable);
    assert_eq!(nothing.name, "Someone");
}
```

- [ ] **Step 3: Run to see it fail**

Run: `cargo test -p oneshot-tmdb`
Expected: FAIL — crate items not found.

- [ ] **Step 4: DTOs** — `crates/providers/tmdb/src/dto.rs` :

```rust
//! TMDB v3 JSON (only the fields Flick uses; serde ignores the rest).

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Person {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub biography: Option<String>,
    pub birthday: Option<String>,
    pub deathday: Option<String>,
    pub place_of_birth: Option<String>,
    pub known_for_department: Option<String>,
    pub profile_path: Option<String>,
    pub combined_credits: Option<Credits>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Credits {
    #[serde(default)]
    pub cast: Vec<Credit>,
    #[serde(default)]
    pub crew: Vec<Credit>,
}

#[derive(Debug, Deserialize)]
pub struct Credit {
    pub id: u64,
    pub media_type: Option<String>,
    pub title: Option<String>,
    pub name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub character: Option<String>,
    pub job: Option<String>,
    pub poster_path: Option<String>,
    #[serde(default)]
    pub vote_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct Cast {
    #[serde(default)]
    pub cast: Vec<PersonHit>,
    #[serde(default)]
    pub crew: Vec<PersonHit>,
}

#[derive(Debug, Deserialize)]
pub struct Search {
    #[serde(default)]
    pub results: Vec<PersonHit>,
}

#[derive(Debug, Deserialize)]
pub struct PersonHit {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub popularity: f32,
}
```

- [ ] **Step 5: Client** — `crates/providers/tmdb/src/lib.rs` :

```rust
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct Tmdb {
    http: Client,
    key: Key,
    base: Url,
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
            let en: dto::Person = self.json(&format!("person/{id}"), &[("language", FALLBACK_LANGUAGE)]).await?;
            person.biography = non_empty(en.biography);
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

    /// The TMDB id of the person named `name`: a TMDB id the server already
    /// knows; else found in the cast of the title the person was opened
    /// from (tells namesakes apart); else the most popular namesake.
    pub async fn identify(&self, name: &str, from: Option<(TitleKind, &str)>, known: Option<&str>) -> Result<Option<u64>> {
        if let Some(id) = known.and_then(|k| k.parse().ok()) {
            return Ok(Some(id));
        }
        if let Some((kind, id)) = from
            && let Some(found) = pick_by_name(&self.cast_of(kind, id).await?, name)
        {
            return Ok(Some(found));
        }
        Ok(pick_by_name(&self.search_person(name).await?, name))
    }
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
        if !crew && role.as_deref().is_some_and(|r| SELF_ROLES.contains(&r.to_lowercase().as_str())) {
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
    known.sort_by(|a, b| b.vote_count.cmp(&a.vote_count));
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
```

In the workspace `Cargo.toml`: add `"crates/providers/tmdb"` to `members` and `oneshot-tmdb = { path = "crates/providers/tmdb" }` to `[workspace.dependencies]`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p oneshot-tmdb && cargo clippy -p oneshot-tmdb --all-targets`
Expected: PASS (10 tests), no warning.

- [ ] **Step 7: Commit** — `TMDB client: people, filmographies, namesake-safe identification`.

---

### Task 3: Jellyfin — `person` et `person_items`

**Files:**
- Modify: `crates/providers/jellyfin/src/dto.rs` (`BaseItemDto`: `end_date: Option<String>`, `#[serde(default)] production_locations: Vec<String>`), `crates/providers/jellyfin/src/provider.rs`
- Create: `crates/providers/jellyfin/tests/people.rs`

**Interfaces:** implements `MediaProvider::person` and `person_items` (Task 1).

- [ ] **Step 1: Failing tests** — `crates/providers/jellyfin/tests/people.rs` (descriptor/identity helpers like `tests/public_users.rs`; provider built with `JellyfinProvider::new(descriptor(base), Client::new(), identity(), "tok".into())`, `descriptor.user.id = "u1"`):

```rust
// person(): GET /Items/p1?userId=u1 → { "Id":"p1","Name":"Tom Hanks","Type":"Person","Overview":"Bio",
//   "PremiereDate":"1956-07-09T00:00:00.0000000Z","ProductionLocations":["Concord"],"ProviderIds":{"Tmdb":"31"},
//   "ImageTags":{"Primary":"t1"} }
// assert: name, overview "Bio", birth 1956-07-09, birthplace "Concord", external_ids.tmdb "31",
//         image.tag == "Primary/t1" and image.item.key == "p1".
//
// person_items() with a hint of this server: GET /Items with query PersonIds=p1, IncludeItemTypes=Movie,Series,
//   Recursive=true → { "Items":[{ "Id":"m1","Name":"Forrest Gump","Type":"Movie" }], "TotalRecordCount":1 }
//   assert one item "m1".
// person_items() by name (hint None or from another server): GET /Persons?searchTerm=Tom%20Hanks →
//   { "Items":[{ "Id":"p9","Name":"Tom Hankson","Type":"Person" },{ "Id":"p1","Name":"tom hanks","Type":"Person" }] }
//   then /Items?PersonIds=p1 as above → assert the exact-name person was used.
// person_items() by name with no match → Ok(empty), no /Items call (mock with .expect(0)).
```

Write these four tests out in full, following the patterns of `tests/public_users.rs` (wiremock `Mock::given(method("GET")).and(path(...)).and(query_param(...))`).

- [ ] **Step 2: Run to see them fail** — `cargo test -p oneshot-jellyfin --test people` → FAIL (default `Unsupported`).

- [ ] **Step 3: Implement** — in `impl MediaProvider for JellyfinProvider`:

```rust
    async fn person(&self, id: &ItemRef) -> Result<PersonInfo> {
        self.check_server(id)?;
        let dto: BaseItemDto = self
            .get(&format!("Items/{}", id.key), &[self.uid(), ("Fields", "Overview,ProviderIds,ProductionLocations".into())])
            .await?;
        Ok(PersonInfo {
            id: id.clone(),
            name: dto.name.clone().unwrap_or_default(),
            overview: dto.overview.clone().filter(|o| !o.trim().is_empty()),
            birth: map::parse_date(&dto.premiere_date),
            death: map::parse_date(&dto.end_date),
            birthplace: dto.production_locations.first().cloned(),
            image: dto.image_tags.get("Primary").map(|t| ImageRef {
                item: id.clone(),
                kind: ImageKind::Poster,
                tag: format!("Primary/{t}"),
                blurhash: None,
            }),
            external_ids: map::external_ids(&dto),
        })
    }

    async fn person_items(&self, name: &str, hint: Option<&ItemRef>) -> Result<Vec<MediaItem>> {
        let person = match hint.filter(|h| h.server == self.server()) {
            Some(h) => h.key.clone(),
            None => {
                let found: QueryResult<BaseItemDto> =
                    self.get("Persons", &[self.uid(), ("searchTerm", name.to_owned()), ("Limit", "20".into())]).await?;
                let key = oneshot_core::text::normalize_name(name);
                match found.items.into_iter().find(|p| p.name.as_deref().is_some_and(|n| oneshot_core::text::normalize_name(n) == key)) {
                    Some(p) => p.id,
                    None => return Ok(Vec::new()),
                }
            }
        };
        let mut q = self.list_query();
        q.extend([
            ("PersonIds", person),
            ("IncludeItemTypes", "Movie,Series".into()),
            ("Recursive", "true".into()),
            ("SortBy", "ProductionYear,SortName".into()),
            ("SortOrder", "Descending".into()),
        ]);
        let r: QueryResult<BaseItemDto> = self.get("Items", &q).await?;
        Ok(self.items(&r.items))
    }
```

Make `map::parse_date` and `map::external_ids` `pub(crate)` if they are private (check names in `map.rs`: the date parser used for `premiere_date` and the `ExternalIds` builder at `map.rs:~172`; extract the latter into `pub(crate) fn external_ids(dto: &BaseItemDto) -> ExternalIds` if it is inline). Add the two DTO fields.

- [ ] **Step 4: Run** — `cargo test -p oneshot-jellyfin && cargo clippy -p oneshot-jellyfin --all-targets` → PASS.

- [ ] **Step 5: Commit** — `Jellyfin: person details and a person's titles`.

---

### Task 4: Plex — `person_items` (recherche d'acteur) + test live

**Files:**
- Modify: `crates/providers/plex/src/dto.rs` (`Hub`: `#[serde(rename = "Directory", default)] pub directories: Vec<HubTag>`; new `HubTag { tag: Option<String>, #[serde(default, deserialize_with = "opt_id_from_any")] id: Option<i64> }`), `crates/providers/plex/src/provider.rs`, `crates/providers/plex/tests/live.rs`
- Create: `crates/providers/plex/tests/people.rs`

**Interfaces:** implements `MediaProvider::person_items` (Task 1). `person` stays `Unsupported` (Plex has no person details worth showing).

- [ ] **Step 1: Failing tests** — `tests/people.rs` (provider on a `MockServer` base like `tests/watchlist.rs`):
  1. hint of this server with numeric key `"4242"`: `GET /library/sections` → two sections (`{"key":"1","type":"movie","title":"Films"}`, `{"key":"2","type":"show","title":"Séries"}`, plus a `"artist"` one ignored); `GET /library/sections/1/all?actor=4242` → one movie; `GET /library/sections/2/all?actor=4242` → one show → assert 2 items.
  2. by name: `GET /hubs/search?query=Tom Hanks` → `{"MediaContainer":{"Hub":[{"type":"actor","title":"People","Directory":[{"tag":"Tom Hankson","id":1},{"tag":"tom hanks","id":"4242"}]}]}}` → uses id 4242 (string or number accepted).
  3. by name, also accepted: the actor hub carrying `Metadata` entries (`{"ratingKey":"4242","type":"person","title":"Tom Hanks"}`) → id 4242.
  4. no actor match → `Ok(empty)`.

- [ ] **Step 2: Run to see them fail** — `cargo test -p oneshot-plex --test people` → FAIL.

- [ ] **Step 3: Implement** (`provider.rs`):

```rust
    /// The actor tag id of `name` on this server (its search hubs).
    async fn actor_id(&self, name: &str) -> Result<Option<i64>> {
        let c = self.container("hubs/search", &[("query", name.to_owned()), ("limit", "10".into())]).await?;
        let key = oneshot_core::text::normalize_name(name);
        let same = |n: &str| oneshot_core::text::normalize_name(n) == key;
        Ok(c.hubs.iter().filter(|h| h.r#type.as_deref() == Some("actor")).find_map(|h| {
            h.directories
                .iter()
                .find(|d| d.tag.as_deref().is_some_and(same))
                .and_then(|d| d.id)
                .or_else(|| h.metadata.iter().find(|m| same(&m.title)).and_then(|m| m.rating_key.parse().ok()))
        }))
    }
```

```rust
    async fn person_items(&self, name: &str, hint: Option<&ItemRef>) -> Result<Vec<MediaItem>> {
        let own = hint.filter(|h| h.server == self.server()).and_then(|h| h.key.parse::<i64>().ok());
        let Some(actor) = (match own {
            Some(id) => Some(id),
            None => self.actor_id(name).await?,
        }) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for lib in self.libraries().await?.into_iter().filter(|l| matches!(l.kind, LibraryKind::Movies | LibraryKind::Shows)) {
            let Some(section) = lib.id.key.strip_prefix("section:") else { continue };
            let c = self.container(&format!("library/sections/{section}/all"), &[("actor", actor.to_string())]).await?;
            out.extend(self.items(&c.metadata));
        }
        Ok(out)
    }
```

Add `opt_id_from_any` next to `id_from_any` in `dto.rs` (same untagged number/string enum, wrapped in `Option`). Import `LibraryKind`.

- [ ] **Step 4: Live check** — append to `tests/live.rs` an `#[ignore]` test `actor_search_live` that reads `ONESHOT_PLEX_URL`, `ONESHOT_PLEX_TOKEN` and `ONESHOT_PLEX_ACTOR` (skip if any is missing), builds the provider with the token, calls `person_items(actor, None)` and prints the titles. It confirms the hub shape on a real server; if it finds nothing while the actor exists, print the raw `hubs/search` body and adapt `HubTag`.

- [ ] **Step 5: Run** — `cargo test -p oneshot-plex && cargo clippy -p oneshot-plex --all-targets` → PASS. Live check: report "not run — needs human" if the variables are absent.

- [ ] **Step 6: Commit** — `Plex: a person's titles through actor search`.

---

### Task 5: Catalog — `person` et `person_items`

**Files:** Modify `crates/catalog/src/lib.rs`.

**Interfaces:**
- Produces: `Catalog::person(&self, id: &ItemRef) -> Option<PersonInfo>` (errors → `None`, logged at debug) ; `Catalog::person_items(&self, name: &str, hint: Option<&ItemRef>) -> Aggregated<Vec<MediaItem>>` (fan-out, `Unsupported` → empty, `dedupe`).

- [ ] **Step 1: Failing test** — in `mod tests`, extend the `Silent` double with `person_items` returning its favourites vector (reuse `self.1`: `Some(v)` → `Ok(v)`, `None` → `Unsupported`), then:

```rust
    #[tokio::test]
    async fn person_items_merge_servers_and_skip_those_without() {
        let catalog = Catalog::new(Arc::new(MetadataCache::in_memory().unwrap()), 3600);
        let jf = descriptor();
        let px = descriptor();
        let movie = MediaItem::new(ItemRef { server: jf.id, key: "1".into() }, ItemKind::Movie, "Forrest Gump");
        catalog.add(Arc::new(Silent(jf, Some(vec![movie]))));
        catalog.add(Arc::new(Silent(px, None)));
        let r = catalog.person_items("Tom Hanks", None).await;
        assert_eq!(r.data.len(), 1);
        assert!(r.issues.is_empty());
    }
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**:

```rust
    /// What the person's own server knows about them (none if it cannot say).
    pub async fn person(&self, id: &ItemRef) -> Option<PersonInfo> {
        let provider = self.provider(id.server).ok()?;
        match provider.person(id).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::debug!(target: "catalog", "person {id}: {e}");
                None
            }
        }
    }

    /// A person's movies and series on every server; the same title on
    /// several servers is merged. Servers that cannot search people are
    /// left out quietly.
    pub async fn person_items(&self, name: &str, hint: Option<&ItemRef>) -> Aggregated<Vec<MediaItem>> {
        let (results, issues) = self
            .fan_out(|p| {
                let name = name.to_owned();
                let hint = hint.cloned();
                async move {
                    match p.person_items(&name, hint.as_ref()).await {
                        Err(Error::Unsupported(_)) => Ok(Vec::new()),
                        other => other,
                    }
                }
            })
            .await;
        let all: Vec<MediaItem> = results.into_iter().flat_map(|(_, items)| items).collect();
        Aggregated { data: dedupe(all), issues }
    }
```

- [ ] **Step 4: Run** — `cargo test -p oneshot-catalog` → PASS. **Step 5: Commit** — `Catalog: person details and titles across servers`.

---

### Task 6: App — clé TMDB, commandes, cache, route d'images

**Files:**
- Create: `app/src/commands/people.rs`
- Modify: `app/Cargo.toml` (`oneshot-tmdb.workspace = true`), `app/src/state.rs` (fields), `app/src/main.rs` (init + handlers), `app/src/commands/mod.rs`, `app/src/images.rs`, `crates/storage/src/images.rs` (`tmdb_cache_key`), `ARCHITECTURE.md` (§10 tokens: `tmdb-key`)

**Interfaces:**
- AppState gains `pub tmdb: RwLock<Option<oneshot_tmdb::Tmdb>>` and `pub metadata: Arc<MetadataCache>` (the same `Arc` the catalogue uses; create it once in `main.rs` and clone).
- Commands (JS names camelCase):
  - `tmdb_status() -> bool`
  - `tmdb_set_key(key: String) -> Result<()>`: `Tmdb::new(state.http(), &key)?`, `check().await?`, store secret `tmdb-key`, set `state.tmdb`
  - `tmdb_remove_key() -> Result<()>`
  - `person_details(person: ItemRef, name: String, from: Option<ItemRef>, language: String) -> PersonDetails`
  - `person_items(person: ItemRef, name: String) -> Aggregated<Vec<MediaItem>>` → `state.catalog.person_items(&name, Some(&person)).await`
- Image route: `oneshot-img://…/tmdb/<size>/<file>`.

- [ ] **Step 1: State and startup** — in `main.rs` build `let metadata = Arc::new(MetadataCache::open(...)?);`, pass `Arc::clone(&metadata)` to `Catalog::new` and store it in `AppState.metadata`; `tmdb: RwLock::new(secrets::load_secret(TMDB_KEY).ok().flatten().and_then(|k| oneshot_tmdb::Tmdb::new(http.clone(), &k).ok()))`. Register the five commands. When network settings change (`settings_set`, where the HTTP client is rebuilt), rebuild the TMDB client from the keychain too (one line after `restore_servers`).

- [ ] **Step 2: `people.rs`**:

```rust
//! Person pages: server data completed by TMDB (key in the keychain, TMDB
//! answers cached for a week), and a person's titles on every server.

use std::sync::Arc;

use oneshot_catalog::Aggregated;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::person::{PersonDetails, TmdbUse};
use oneshot_core::{ItemRef, Result};
use oneshot_storage::secrets;
use oneshot_tmdb::{TitleKind, Tmdb, TmdbPerson, merge_details};
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub(crate) const TMDB_KEY: &str = "tmdb-key";
const WEEK: u32 = 7 * 24 * 3600;

#[tauri::command]
pub fn tmdb_status(state: St<'_>) -> bool {
    state.tmdb.read().is_some()
}

#[tauri::command]
pub async fn tmdb_set_key(state: St<'_>, key: String) -> Result<()> {
    let tmdb = Tmdb::new(state.http(), &key)?;
    tmdb.check().await?;
    secrets::store_secret(TMDB_KEY, key.trim())?;
    *state.tmdb.write() = Some(tmdb);
    Ok(())
}

#[tauri::command]
pub fn tmdb_remove_key(state: St<'_>) -> Result<()> {
    secrets::delete_secret(TMDB_KEY)?;
    *state.tmdb.write() = None;
    Ok(())
}

fn cached<T: serde::de::DeserializeOwned>(state: &AppState, key: &str) -> Option<T> {
    state.metadata.get::<T>(key).ok().flatten().filter(|c| c.fresh).map(|c| c.value)
}

fn remember<T: serde::Serialize>(state: &AppState, key: &str, value: &T) {
    if let Err(e) = state.metadata.put("tmdb", key, value, WEEK) {
        tracing::debug!(target: "cache", "tmdb cache write failed: {e}");
    }
}

async fn tmdb_person(state: &AppState, tmdb: &Tmdb, person: &ItemRef, name: &str, from: Option<&MediaItem>, known: Option<&str>, language: &str) -> Result<Option<TmdbPerson>> {
    let match_key = format!("tmdb:match:{person}");
    let id = match cached::<u64>(state, &match_key) {
        Some(id) => Some(id),
        None => {
            let title = from.and_then(|f| {
                let kind = match f.kind {
                    ItemKind::Movie => TitleKind::Movie,
                    ItemKind::Series | ItemKind::Season | ItemKind::Episode => TitleKind::Tv,
                    _ => return None,
                };
                f.external_ids.tmdb.as_deref().map(|id| (kind, id))
            });
            let id = tmdb.identify(name, title, known).await?;
            if let Some(id) = id {
                remember(state, &match_key, &id);
            }
            id
        }
    };
    let Some(id) = id else { return Ok(None) };
    let person_key = format!("tmdb:person:{id}:{language}");
    if let Some(p) = cached::<TmdbPerson>(state, &person_key) {
        return Ok(Some(p));
    }
    let p = tmdb.person(id, language).await?;
    remember(state, &person_key, &p);
    Ok(Some(p))
}

#[tauri::command]
pub async fn person_details(state: St<'_>, person: ItemRef, name: String, from: Option<ItemRef>, language: String) -> PersonDetails {
    let server = state.catalog.person(&person).await;
    let origin = match &from {
        Some(f) => state.catalog.item(f).await.ok(),
        None => None,
    };
    let tmdb = state.tmdb.read().clone();
    let (found, used) = match tmdb {
        None => (None, TmdbUse::NoKey),
        Some(t) => {
            let known = server.as_ref().and_then(|s| s.external_ids.tmdb.clone());
            match tmdb_person(&state, &t, &person, &name, origin.as_ref(), known.as_deref(), &language).await {
                Ok(Some(p)) => (Some(p), TmdbUse::Used),
                Ok(None) => (None, TmdbUse::NotFound),
                Err(e) => {
                    tracing::warn!(target: "provider", "TMDB unavailable for {name}: {e}");
                    (None, TmdbUse::Unavailable)
                }
            }
        }
    };
    merge_details(&name, server.as_ref(), found.as_ref(), used)
}

#[tauri::command]
pub async fn person_items(state: St<'_>, person: ItemRef, name: String) -> Aggregated<Vec<MediaItem>> {
    state.catalog.person_items(&name, Some(&person)).await
}
```

`Tmdb` derives `Clone`; `state.tmdb.read().clone()` releases the guard before any `.await` (keep it on its own statement).

- [ ] **Step 3: Image route** — in `app/src/images.rs` `handle`, add a branch before `avatar/`:

```rust
            Some(rest) if rest.starts_with("tmdb/") => Some(load_tmdb(&state, &rest["tmdb/".len()..]).await),
```

(restructure the existing `match path.trim_start_matches('/').strip_prefix("avatar/")` into a `let p = path.trim_start_matches('/');` followed by `if let Some(rest) = p.strip_prefix("tmdb/") { … } else if let Some(rest) = p.strip_prefix("avatar/") { … } else { … }`), and:

```rust
/// A TMDB photo or poster (`tmdb/<size>/<file>`), public; only TMDB files.
async fn load_tmdb(state: &AppState, rest: &str) -> Result<Vec<u8>> {
    let url = oneshot_tmdb::image_url(rest).ok_or_else(|| Error::Invalid("tmdb image path".into()))?;
    let key = oneshot_storage::images::tmdb_cache_key(&url);
    if let Some(bytes) = state.images.get(&key) {
        return Ok(bytes);
    }
    let resp = oneshot_net::ensure_ok(state.http().get(url).send().await.map_err(oneshot_net::map_err)?).await?;
    let bytes = resp.bytes().await.map_err(oneshot_net::map_err)?.to_vec();
    if let Err(e) = state.images.put(&key, &bytes) {
        tracing::warn!(target: "cache", "tmdb image cache write failed: {e}");
    }
    Ok(bytes)
}
```

`crates/storage/src/images.rs`: `pub fn tmdb_cache_key(url: &url::Url) -> String` — same as `avatar_cache_key` with the prefix `b"tmdb\0"` (factor both through a private `fn url_key(prefix: &[u8], url: &Url) -> String`; keep `avatar_cache_key`'s output unchanged).

- [ ] **Step 4: Docs** — ARCHITECTURE.md §10 tokens bullet: add « la clé TMDB (`tmdb-key`), saisie dans Réglages › Metadata ».

- [ ] **Step 5: Verify** — `cargo build -p oneshot-app && cargo test --workspace && cargo clippy --workspace --all-targets` → PASS, zero warnings.

- [ ] **Step 6: Commit** — `App: TMDB key, person commands, TMDB image proxy`.

---

### Task 7: UI — IPC, images, helpers

**Files:**
- Modify: `ui/src/ipc/api.ts`, `ui/src/ipc/images.ts`
- Create: `ui/src/lib/person.ts`, `ui/src/lib/person.test.ts`

**Interfaces:**
- `api.personDetails(person, name, from: ItemRef | null, language) -> PersonDetails`, `api.personItems(person, name) -> Aggregated<MediaItem[]>`, `api.tmdbStatus() -> boolean`, `api.tmdbSetKey(key) -> void`, `api.tmdbRemoveKey() -> void`
- `tmdbImageUrl(path: string, size: "w185" | "h632" | "w342"): string` → `${BASE}tmdb/${size}${path}` (TMDB paths start with `/`)
- `lib/person.ts`: `personPath(credit: { person: ItemRef; name: string }, from?: ItemRef): string` → `/person/<enc ref>?name=<enc>&from=<enc>` ; `age(birth: string, until: Date): number` ; `lifeLine(d: PersonDetails, today: Date): string | null` (« Born 9 July 1956 (age 70) · Concord, California, USA » / « 1932 – 2016 (aged 84) ») ; `alsoKnownFor(credits: KnownFor[], onServers: MediaItem[], limit = 20): KnownFor[]` (drops tmdbId present in `onServers[].externalIds.tmdb`, and same normalised title + year as an item on the servers) ; `personShelves(items: MediaItem[])` → `[{ title: "Movies", items }, { title: "TV Shows", items }]` without empty ones ; `uiLanguage(settingsLanguage: string | null, navigatorLanguage: string): string`.

- [ ] **Step 1: Failing tests** — `ui/src/lib/person.test.ts` (node env; mock `@/ipc/api` as in `favorites.test.ts`):

```ts
import { describe, expect, it, vi } from "vitest";
import type { KnownFor } from "@/ipc/bindings/KnownFor";
import type { MediaItem } from "@/ipc/bindings/MediaItem";

vi.mock("@/ipc/api", () => ({ api: {} }));

import { age, alsoKnownFor, lifeLine, personPath, personShelves, uiLanguage } from "./person";

const kf = (tmdbId: string, title: string, year: number, votes: number): KnownFor =>
  ({ kind: "movie", title, year, role: null, poster: null, tmdbId, voteCount: votes }) as KnownFor;
const item = (id: string, kind: MediaItem["kind"], tmdb: string | null, title = id, year: number | null = null) =>
  ({ id, kind, title, year, externalIds: { imdb: null, tmdb, tvdb: null } }) as unknown as MediaItem;

describe("person helpers", () => {
  it("builds the page path", () => {
    expect(personPath({ person: "s:p1", name: "Tom Hanks" }, "s:m1")).toBe("/person/s%3Ap1?name=Tom%20Hanks&from=s%3Am1");
    expect(personPath({ person: "s:p1", name: "Tom Hanks" })).toBe("/person/s%3Ap1?name=Tom%20Hanks");
  });

  it("computes ages", () => {
    expect(age("1956-07-09", new Date("2026-07-08"))).toBe(69);
    expect(age("1956-07-09", new Date("2026-07-09"))).toBe(70);
  });

  it("writes the life line", () => {
    const base = { name: "", department: null, biography: null, photo: null, knownFor: [], tmdb: "used" } as const;
    expect(lifeLine({ ...base, birth: "1956-07-09", death: null, birthplace: "Concord" }, new Date("2026-09-28"))).toBe("Born 9 July 1956 (age 70) · Concord");
    expect(lifeLine({ ...base, birth: "1932-04-10", death: "2016-12-28", birthplace: null }, new Date("2026-09-28"))).toBe("1932 – 2016 (aged 84)");
    expect(lifeLine({ ...base, birth: null, death: null, birthplace: null }, new Date())).toBeNull();
  });

  it("keeps known-for titles that are not on the servers, most voted first, 20 at most", () => {
    const credits = [kf("13", "Forrest Gump", 1994, 900), kf("862", "Toy Story", 1995, 800), kf("7", "Big", 1988, 100)];
    const onServers = [item("a", "movie", "13"), item("b", "movie", null, "Big", 1988)];
    expect(alsoKnownFor(credits, onServers).map((c) => c.title)).toEqual(["Toy Story"]);
    expect(alsoKnownFor(Array.from({ length: 30 }, (_, i) => kf(String(i), `T${i}`, 2000, 30 - i)), [])).toHaveLength(20);
  });

  it("splits the servers' titles into shelves", () => {
    const shelves = personShelves([item("s", "series", null), item("m", "movie", null)]);
    expect(shelves.map((s) => [s.title, s.items.map((i) => i.id)])).toEqual([["Movies", ["m"]], ["TV Shows", ["s"]]]);
  });

  it("picks the TMDB language", () => {
    expect(uiLanguage("fr", "en-US")).toBe("fr");
    expect(uiLanguage(null, "fr-FR")).toBe("fr-FR");
  });
});
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `ui/src/lib/person.ts` (English month names via `toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" })` on a `Date` built from the `YYYY-MM-DD` parts in UTC; title matching with a local `norm = (s) => s.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase().replace(/\s+/g, " ").trim()`), the `api` entries and `tmdbImageUrl`.

- [ ] **Step 4: Run** — `pnpm --dir ui run test && pnpm --dir ui run typecheck` → PASS. **Step 5: Commit** — `UI: person IPC and helpers`.

---

### Task 8: UI — page personne, distribution cliquable

**Files:**
- Create: `ui/src/features/person/PersonPage.tsx`, `ui/src/features/person/KnownForCard.tsx`
- Modify: `ui/src/App.tsx` (route `/person/:ref` in the Shell), `ui/src/features/detail/Detail.tsx` (the `Person` credit button navigates with `personPath(credit, item.id)`; `cursor-pointer`), `docs/DESIGN_SYSTEM.md` (rows `PersonPage`, `KnownForCard`)

**Interfaces:** consumes Task 7.

- [ ] **Step 1: `KnownForCard`** — a focusable (`useTv`) poster-shaped card, same size as `MediaCard` poster (`w-[var(--poster-w)]`, `aspect-[2/3]`, radius 8 px), TMDB poster via `tmdbImageUrl(poster, "w342")` or the title's initial on `bg-white/[0.06]`; title + year (+ role in `text-white/50`) below; `aria-label` "Not on your servers"; no click action; focus lift ×1.08 with `focusSpring`.

- [ ] **Step 2: `PersonPage`** — structure:
  - `const { ref } = useParams(); const [params] = useSearchParams(); const person = decodeURIComponent(ref); const name = params.get("name") ?? ""; const from = params.get("from");`
  - `language = uiLanguage(useSettings()?.general.language ?? null, navigator.language)`
  - `details = useQuery({ queryKey: ["person", person, language], queryFn: () => api.personDetails(person, name, from, language), staleTime: Infinity })`
  - `items = useQuery({ queryKey: ["person-items", person], queryFn: () => api.personItems(person, name) })`
  - Ambient: `useEffect` — when `details.data?.photo` is a server image, `ambientFor`-like call is not possible (no MediaItem); use `ambientColor("#7a7a7a")` only when nothing else — keep it simple: call `ambientReset()` on mount so the previous title's artwork does not linger.
  - Header (`px-[var(--gutter)] pt-[var(--page-top)]`): `BackButton`; round photo `size-44` (TMDB `h632` or `imageUrl(image, "large")`, else initials on `bg-white/[0.08]`); name `text-[2.75rem] font-bold tracking-tight`; department `text-white/60`; `lifeLine(...)`; biography `line-clamp-5` with a `Button size="sm" variant="ghost"` « More » / « Less » when longer than the clamp (toggle state).
  - No key → `Notice` « Add a TMDB key in Settings › Metadata for biographies. » with a `LinkRow`-style ghost button to `/settings?s=metadata`. `Unavailable` / `NotFound` → nothing.
  - Shelves: `personShelves(items.data?.data ?? [])` → `<Shelf … shape="poster" />`; then `alsoKnownFor(details.data?.knownFor ?? [], items.data?.data ?? [])` → a section titled « Also known for » with a horizontal `FocusGroup` (`fade="x"`, same paddings as `Shelf`) of `KnownForCard`s.
  - Loading: header shows the name from the query string at once; shelves use `CardPlaceholder` rows while `items` loads; `Screen ready={!!details.data || !!items.data}`.
  - Issues from `items.data.issues` → the same `Notice tone="warn"` as Favorites.

- [ ] **Step 3: Wire** — route in `App.tsx`: `<Route path="/person/:ref" element={<PersonPage />} />`; in `Detail.tsx` `Person`: `onClick={() => navigate(personPath(credit, itemId))}` (pass the title's id down), class `cursor-pointer`. `Shell` `TOP_LEVEL` unchanged (person pages are drill-downs, Back goes back).

- [ ] **Step 4: Verify** — `pnpm --dir ui run typecheck && pnpm --dir ui run test`; visual check in the app → "not run — needs human": open a movie, click an actor, check photo/bio/shelves/Also known for, Back returns to the movie; without a key the hint shows and server titles still list.

- [ ] **Step 5: Commit** — `UI: person pages from the cast`.

---

### Task 9: UI — Réglages › Metadata (clé TMDB)

**Files:**
- Create: `ui/src/features/settings/TmdbSettings.tsx`, `ui/src/components/tv/TmdbLogo.tsx`
- Modify: `ui/src/features/settings/Settings.tsx` (section `["metadata", "Metadata"]` after `["servers", "Servers"]`, body `<TmdbSettings />`)

- [ ] **Step 1: `TmdbLogo`** — the official TMDB wordmark as inline SVG path(s) in `currentColor` (white), height 14 px; source: TMDB's logo page (use the "short" blue logo's geometry, recoloured). If the official path data is not available offline, render the text « TMDB » in `font-heading font-black tracking-tight` as a placeholder and say so in the report.

- [ ] **Step 2: `TmdbSettings`**:
  - `status = useQuery({ queryKey: ["tmdb-status"], queryFn: api.tmdbStatus })`
  - `SettingsGroup` with note (attribution + logo): « This product uses the TMDB API but is not endorsed or certified by TMDB. »
  - Connected: `InfoRow label="TMDB" > "Connected"` and a `Button variant="danger" size="sm"` « Remove Key » (`api.tmdbRemoveKey()`, invalidate `["tmdb-status"]` and `["person"]`).
  - Not connected: `TextField label="API key or read access token" type="password"` + `Button variant="primary"` « Test & Save » → `api.tmdbSetKey(key)`; success → toast « TMDB connected », clear the field, invalidate; error → `Notice tone="error"` with `asError(e).message` (401 → « The key was refused by TMDB. »).
  - Hint text: « Free key at themoviedb.org › Settings › API. Used for biographies, photos and filmographies on person pages. »

- [ ] **Step 3: Verify** — typecheck, tests; visual → needs human (save a real key, Remove, invalid key message).

- [ ] **Step 4: Commit** — `UI: Settings › Metadata for the TMDB key`.

---

### Task 10: Vérification finale

- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets` (0 warnings), `pnpm --dir ui run typecheck`, `pnpm --dir ui run test`, `pnpm --dir ui exec vite build`.
- [ ] Live, manual (report as needs-human): `TMDB_KEY=… cargo test -p oneshot-tmdb --test live -- --ignored` if a live test was added; `ONESHOT_PLEX_URL=… ONESHOT_PLEX_TOKEN=… ONESHOT_PLEX_ACTOR="Tom Hanks" cargo test -p oneshot-plex --test live actor_search_live -- --ignored --nocapture`.
- [ ] Commit any doc touch-ups — `Docs: person pages`.
