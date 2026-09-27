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
