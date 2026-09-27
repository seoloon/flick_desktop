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

#[tokio::test]
async fn the_titles_cast_beats_an_id_the_server_knows() {
    // Servers keep one record per name: a namesake's TMDB id may be on it.
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/movie/100/credits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "cast": [{ "id": 16828, "name": "Chris Evans" }], "crew": [] }))).mount(&server).await;
    assert_eq!(tmdb(&server, V3).identify("Chris Evans", Some((TitleKind::Movie, "100")), Some("42")).await.unwrap(), Some(16828));
}

#[tokio::test]
async fn a_failed_cast_lookup_falls_back_to_search() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/tv/555/aggregate_credits")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    Mock::given(method("GET")).and(path("/3/search/person"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "results": [{ "id": 999, "name": "Chris Evans", "popularity": 1.0 }] }))).mount(&server).await;
    assert_eq!(tmdb(&server, V3).identify("Chris Evans", Some((TitleKind::Tv, "555")), None).await.unwrap(), Some(999));
}

#[tokio::test]
async fn a_failed_english_fallback_keeps_the_first_answer() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/person/31")).and(query_param("language", "fr-FR"))
        .respond_with(ResponseTemplate::new(200).set_body_json(person_json(""))).mount(&server).await;
    Mock::given(method("GET")).and(path("/3/person/31")).and(query_param("language", "en-US")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    let p = tmdb(&server, V3).person(31, "fr-FR").await.unwrap();
    assert_eq!(p.biography, None);
    assert_eq!(p.known_for.len(), 3);
}

#[tokio::test]
async fn appearances_as_oneself_are_left_out_whatever_the_wording() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/3/person/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 7, "name": "X", "biography": "b", "combined_credits": { "cast": [
            { "id": 1, "media_type": "tv", "name": "Talk", "character": "Self - Host", "vote_count": 9 },
            { "id": 2, "media_type": "tv", "name": "Doc", "character": "Himself (archive footage)", "vote_count": 8 },
            { "id": 3, "media_type": "movie", "title": "Real", "character": "Selma", "vote_count": 7 }
        ] } }))).mount(&server).await;
    let p = tmdb(&server, V3).person(7, "en-US").await.unwrap();
    assert_eq!(p.known_for.iter().map(|k| k.title.as_str()).collect::<Vec<_>>(), ["Real"]);
}

#[test]
fn debug_output_never_shows_the_key() {
    let t = Tmdb::new(oneshot_net::reqwest::Client::new(), V3).unwrap();
    assert!(!format!("{t:?}").contains(V3));
    let t = Tmdb::new(oneshot_net::reqwest::Client::new(), V4).unwrap();
    assert!(!format!("{t:?}").contains(V4));
}
