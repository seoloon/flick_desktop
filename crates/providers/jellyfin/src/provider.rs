//! `MediaProvider` implementation (catalogue part). Playback and admin live
//! in sibling modules operating on the same [`JellyfinProvider`].

use std::time::Instant;

use async_trait::async_trait;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ImageKind, ImageRef, ImageSize, ItemKind, Marker, MediaItem};
use oneshot_core::playback::{ClientProfile, PlaybackInfo, PlaybackReport, StreamRequest, StreamTarget};
use oneshot_core::provider::{AdminProvider, Adjacent, MediaProvider};
use oneshot_core::query::{HomeRow, HomeRowKind, ItemQuery, Page, SortBy, SortOrder};
use oneshot_core::server::{Library, LibraryKind, ProviderKind, ServerDescriptor, ServerStatus};
use oneshot_core::person::PersonInfo;
use oneshot_core::text::normalize_name;
use oneshot_core::{Error, Result};
use oneshot_net::reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use url::Url;

use crate::auth::ClientIdentity;
use crate::dto::{BaseItemDto, MediaSegmentDto, QueryResult};
use crate::map;

/// Fields requested for list views: enough for cards and the hero, without
/// the heavy `MediaSources`/`People` payloads.
pub(crate) const LIST_FIELDS: &str =
    "Overview,Genres,ProviderIds,DateCreated,Taglines,ChildCount,RecursiveItemCount,PrimaryImageAspectRatio,Studios";
const IMAGE_TYPES: &str = "Primary,Backdrop,Thumb,Logo";

#[derive(Debug)]
pub struct JellyfinProvider {
    pub(crate) descriptor: ServerDescriptor,
    pub(crate) http: Client,
    pub(crate) identity: ClientIdentity,
    pub(crate) token: String,
}

impl JellyfinProvider {
    pub fn new(descriptor: ServerDescriptor, http: Client, identity: ClientIdentity, token: String) -> Self {
        Self { descriptor, http, identity, token }
    }

    pub(crate) fn user_id(&self) -> &str {
        &self.descriptor.user.id
    }

    pub(crate) fn server(&self) -> oneshot_core::ServerId {
        self.descriptor.id
    }

    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        oneshot_net::join(&self.descriptor.base_url, path)
    }

    pub(crate) fn request(&self, method: Method, path: &str, query: &[(&str, String)]) -> Result<RequestBuilder> {
        let mut url = self.url(path)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        Ok(self.http.request(method, url).header("Authorization", self.identity.header(Some(&self.token))))
    }

    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let started = Instant::now();
        let resp = self.request(Method::GET, path, query)?.send().await.map_err(oneshot_net::map_err)?;
        tracing::debug!(target: "provider", server = %self.descriptor.name, path, status = resp.status().as_u16(),
            ms = started.elapsed().as_millis() as u64, "jellyfin GET");
        oneshot_net::json(resp).await
    }

    pub(crate) async fn send_empty(&self, method: Method, path: &str, query: &[(&str, String)]) -> Result<()> {
        let resp = self.request(method, path, query)?.send().await.map_err(oneshot_net::map_err)?;
        oneshot_net::ensure_ok(resp).await.map(drop)
    }

    fn uid(&self) -> (&'static str, String) {
        ("userId", self.user_id().to_owned())
    }

    fn list_query(&self) -> Vec<(&'static str, String)> {
        vec![
            self.uid(),
            ("Fields", LIST_FIELDS.into()),
            ("EnableImageTypes", IMAGE_TYPES.into()),
            ("ImageTypeLimit", "1".into()),
        ]
    }

    fn items(&self, dtos: &[BaseItemDto]) -> Vec<MediaItem> {
        dtos.iter().map(|d| map::item(self.server(), d)).collect()
    }

    fn check_server(&self, id: &ItemRef) -> Result<()> {
        if id.server == self.server() {
            Ok(())
        } else {
            Err(Error::Invalid(format!("item {id} does not belong to {}", self.descriptor.name)))
        }
    }

    /// The user's libraries, without their title counts.
    async fn views(&self) -> Result<Vec<Library>> {
        let r: QueryResult<BaseItemDto> = self.get("UserViews", &[self.uid()]).await?;
        Ok(r.items.iter().map(|d| map::library(self.server(), d)).collect())
    }

    /// Movies or series in a library, as its grid lists them. `None` for
    /// other kinds of library, or when the server does not answer.
    async fn title_count(&self, library: &Library) -> Option<u32> {
        let kind = match library.kind {
            LibraryKind::Movies => "Movie",
            LibraryKind::Shows => "Series",
            _ => return None,
        };
        let q = [
            self.uid(),
            ("ParentId", library.id.key.clone()),
            ("IncludeItemTypes", kind.into()),
            ("Recursive", "true".into()),
            ("Limit", "0".into()),
            ("EnableTotalRecordCount", "true".into()),
        ];
        match self.get::<QueryResult<BaseItemDto>>("Items", &q).await {
            Ok(r) => r.total_record_count,
            Err(e) => {
                tracing::warn!(target: "provider", library = %library.name, "title count failed: {e}");
                None
            }
        }
    }

    async fn latest(&self, library: &Library) -> Result<Vec<MediaItem>> {
        let mut q = self.list_query();
        q.push(("ParentId", library.id.key.clone()));
        q.push(("Limit", "20".into()));
        let dtos: Vec<BaseItemDto> = self.get("Items/Latest", &q).await?;
        Ok(self.items(&dtos))
    }
}

fn sort_by(s: SortBy) -> &'static str {
    match s {
        SortBy::Title => "SortName",
        SortBy::DateAdded => "DateCreated,SortName",
        SortBy::ReleaseDate => "PremiereDate,ProductionYear,SortName",
        SortBy::Rating => "CommunityRating,SortName",
        SortBy::LastPlayed => "DatePlayed,SortName",
        SortBy::Random => "Random",
    }
}

fn image_type(tag: &str, kind: ImageKind) -> (&str, &str) {
    // Tags are stored as "<JellyfinType>/<tag>" by the mapper.
    match tag.split_once('/') {
        Some((t, rest)) => (t, rest),
        None => (
            match kind {
                ImageKind::Poster => "Primary",
                ImageKind::Backdrop => "Backdrop",
                ImageKind::Thumb => "Thumb",
                ImageKind::Logo => "Logo",
                ImageKind::Banner => "Banner",
            },
            tag,
        ),
    }
}

#[async_trait]
impl MediaProvider for JellyfinProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Jellyfin
    }

    fn descriptor(&self) -> &ServerDescriptor {
        &self.descriptor
    }

    async fn status(&self) -> ServerStatus {
        let started = Instant::now();
        let url = self.descriptor.base_url.clone();
        match self.get::<serde_json::Value>("System/Info", &[]).await {
            Ok(_) => ServerStatus::Online { latency_ms: started.elapsed().as_millis() as u32, url },
            Err(Error::Unauthorized) => ServerStatus::Unauthorized,
            Err(e) => ServerStatus::Unreachable { error: e.to_string() },
        }
    }

    async fn libraries(&self) -> Result<Vec<Library>> {
        let mut libraries = self.views().await?;
        let counts = futures::future::join_all(libraries.iter().map(|l| self.title_count(l))).await;
        for (library, count) in libraries.iter_mut().zip(counts) {
            library.item_count = count;
        }
        Ok(libraries)
    }

    async fn home(&self) -> Result<Vec<HomeRow>> {
        let mut resume_q = self.list_query();
        resume_q.extend([("Limit", "24".into()), ("MediaTypes", "Video".into())]);
        let mut next_q = self.list_query();
        next_q.extend([("Limit", "24".into()), ("EnableResumable", "false".into())]);
        let (resume, next_up, libraries) = futures::join!(
            self.get::<QueryResult<BaseItemDto>>("UserItems/Resume", &resume_q),
            self.get::<QueryResult<BaseItemDto>>("Shows/NextUp", &next_q),
            self.views(),
        );

        let mut rows = Vec::new();
        if let Ok(r) = resume
            && !r.items.is_empty()
        {
            rows.push(HomeRow { kind: HomeRowKind::ContinueWatching, title: "Continue Watching".into(), items: self.items(&r.items) });
        }
        if let Ok(r) = next_up
            && !r.items.is_empty()
        {
            rows.push(HomeRow { kind: HomeRowKind::NextUp, title: "Next Up".into(), items: self.items(&r.items) });
        }
        for lib in libraries?.iter().filter(|l| matches!(l.kind, LibraryKind::Movies | LibraryKind::Shows)) {
            match self.latest(lib).await {
                Ok(items) if !items.is_empty() => rows.push(HomeRow {
                    kind: HomeRowKind::RecentlyAdded { library: Some(lib.id.clone()) },
                    title: format!("Recently Added in {}", lib.name),
                    items,
                }),
                Ok(_) => {}
                Err(e) => tracing::warn!(target: "provider", library = %lib.name, "latest failed: {e}"),
            }
        }
        // Recommendations ("Because you watched …") are real server data.
        let mut rec_q = vec![self.uid(), ("categoryLimit", "1".into()), ("ItemLimit", "20".into()), ("Fields", LIST_FIELDS.into())];
        rec_q.push(("EnableImageTypes", IMAGE_TYPES.into()));
        if let Ok(recs) = self.get::<Vec<Recommendation>>("Movies/Recommendations", &rec_q).await
            && let Some(rec) = recs.into_iter().find(|r| !r.items.is_empty())
        {
            let title = rec.baseline_item_name.map_or_else(|| "Recommended".into(), |b| format!("Because you watched {b}"));
            rows.push(HomeRow { kind: HomeRowKind::Recommended, title, items: self.items(&rec.items) });
        }
        let mut col_q = self.list_query();
        col_q.extend([("IncludeItemTypes", "BoxSet".into()), ("Recursive", "true".into()), ("Limit", "20".into())]);
        if let Ok(c) = self.get::<QueryResult<BaseItemDto>>("Items", &col_q).await
            && !c.items.is_empty()
        {
            rows.push(HomeRow { kind: HomeRowKind::Collections, title: "Collections".into(), items: self.items(&c.items) });
        }
        Ok(rows)
    }

    async fn items(&self, query: &ItemQuery) -> Result<Page<MediaItem>> {
        let mut q = self.list_query();
        q.push(("Recursive", "true".into()));
        q.push(("StartIndex", query.start.to_string()));
        q.push(("Limit", query.limit.to_string()));
        q.push(("SortBy", sort_by(query.sort).into()));
        q.push(("SortOrder", if query.order == SortOrder::Descending { "Descending" } else { "Ascending" }.into()));
        q.push(("EnableTotalRecordCount", "true".into()));
        if let Some(parent) = &query.parent {
            self.check_server(parent)?;
            q.push(("ParentId", parent.key.clone()));
        }
        let types: Vec<&str> = query.kinds.iter().filter_map(|k| map::item_type_name(*k)).collect();
        if !types.is_empty() {
            q.push(("IncludeItemTypes", types.join(",")));
        }
        let f = &query.filter;
        if !f.genres.is_empty() {
            q.push(("Genres", f.genres.join("|")));
        }
        if !f.years.is_empty() {
            q.push(("Years", f.years.iter().map(i32::to_string).collect::<Vec<_>>().join(",")));
        }
        if let Some(p) = &f.person {
            q.push(("PersonIds", p.key.clone()));
        }
        if f.unplayed_only {
            q.push(("IsPlayed", "false".into()));
        }
        if f.favorites_only {
            q.push(("IsFavorite", "true".into()));
        }
        let r: QueryResult<BaseItemDto> = self.get("Items", &q).await?;
        Ok(Page { items: self.items(&r.items), start: r.start_index.unwrap_or(query.start), total: r.total_record_count })
    }

    async fn item(&self, id: &ItemRef) -> Result<MediaItem> {
        self.check_server(id)?;
        let dto: BaseItemDto = self.get(&format!("Items/{}", id.key), &[self.uid()]).await?;
        Ok(map::item(self.server(), &dto))
    }

    async fn children(&self, id: &ItemRef, kind: ItemKind) -> Result<Vec<MediaItem>> {
        self.check_server(id)?;
        let mut q = self.list_query();
        let path = match kind {
            // Playlists keep their curated order.
            ItemKind::Playlist => format!("Playlists/{}/Items", id.key),
            ItemKind::Series => {
                q.push(("SortBy", "ParentIndexNumber,IndexNumber,SortName".into()));
                format!("Shows/{}/Seasons", id.key)
            }
            _ => {
                q.push(("ParentId", id.key.clone()));
                q.push(("SortBy", if kind == ItemKind::Collection { "PremiereDate,SortName" } else { "ParentIndexNumber,IndexNumber,SortName" }.into()));
                "Items".into()
            }
        };
        let r: QueryResult<BaseItemDto> = self.get(&path, &q).await?;
        Ok(self.items(&r.items))
    }

    async fn search(&self, term: &str, limit: u32) -> Result<Vec<MediaItem>> {
        let mut q = self.list_query();
        q.extend([
            ("searchTerm", term.to_owned()),
            ("Recursive", "true".into()),
            ("IncludeItemTypes", "Movie,Series,Episode,BoxSet".into()),
            ("Limit", limit.to_string()),
        ]);
        let r: QueryResult<BaseItemDto> = self.get("Items", &q).await?;
        Ok(self.items(&r.items))
    }

    async fn similar(&self, id: &ItemRef, limit: u32) -> Result<Vec<MediaItem>> {
        self.check_server(id)?;
        let mut q = self.list_query();
        q.push(("Limit", limit.to_string()));
        let r: QueryResult<BaseItemDto> = self.get(&format!("Items/{}/Similar", id.key), &q).await?;
        Ok(self.items(&r.items))
    }

    async fn adjacent_episodes(&self, id: &ItemRef) -> Result<Adjacent> {
        let current = self.item(id).await?;
        let Some(series) = current.episode.as_ref().and_then(|e| e.series.clone()) else {
            return Ok(Adjacent::default());
        };
        let mut q = self.list_query();
        q.push(("AdjacentTo", id.key.clone()));
        let r: QueryResult<BaseItemDto> = self.get(&format!("Shows/{}/Episodes", series.key), &q).await?;
        let pos = r.items.iter().position(|d| d.id == id.key);
        let at = |i: Option<usize>| i.and_then(|i| r.items.get(i)).map(|d| map::item(self.server(), d));
        Ok(match pos {
            Some(p) => Adjacent { previous: at(p.checked_sub(1)), next: at(Some(p + 1)) },
            None => Adjacent::default(),
        })
    }

    async fn markers(&self, id: &ItemRef) -> Result<Vec<Marker>> {
        self.check_server(id)?;
        match self.get::<QueryResult<MediaSegmentDto>>(&format!("MediaSegments/{}", id.key), &[]).await {
            Ok(r) => Ok(r.items.iter().filter_map(map::marker).collect()),
            // Servers < 10.10 have no segments API: no markers, not an error.
            Err(Error::NotFound(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    async fn set_played(&self, id: &ItemRef, played: bool) -> Result<()> {
        self.check_server(id)?;
        let method = if played { Method::POST } else { Method::DELETE };
        self.send_empty(method, &format!("UserPlayedItems/{}", id.key), &[self.uid()]).await
    }

    async fn set_favorite(&self, id: &ItemRef, favorite: bool) -> Result<()> {
        self.check_server(id)?;
        let method = if favorite { Method::POST } else { Method::DELETE };
        self.send_empty(method, &format!("UserFavoriteItems/{}", id.key), &[self.uid()]).await
    }

    async fn playback_info(&self, id: &ItemRef, profile: &ClientProfile) -> Result<PlaybackInfo> {
        self.check_server(id)?;
        crate::playback::playback_info(self, id, profile).await
    }

    async fn stream(&self, request: &StreamRequest) -> Result<StreamTarget> {
        self.check_server(&request.item)?;
        crate::playback::stream(self, request).await
    }

    async fn report(&self, report: &PlaybackReport) -> Result<()> {
        crate::playback::report(self, report).await
    }

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
                let key = normalize_name(name);
                match found.items.into_iter().find(|p| p.name.as_deref().is_some_and(|n| normalize_name(n) == key)) {
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

    fn image_url(&self, image: &ImageRef, size: ImageSize) -> Result<Url> {
        let (jf_type, tag) = image_type(&image.tag, image.kind);
        let mut url = self.url(&format!("Items/{}/Images/{jf_type}", image.item.key))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("tag", tag).append_pair("quality", "90");
            if let Some(w) = size.max_width() {
                q.append_pair("maxWidth", &w.to_string());
            }
        }
        Ok(url)
    }

    fn auth_headers(&self) -> Vec<(String, String)> {
        vec![("Authorization".into(), self.identity.header(Some(&self.token)))]
    }

    fn admin(&self) -> Option<&dyn AdminProvider> {
        self.descriptor.user.is_admin.then_some(self as &dyn AdminProvider)
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Recommendation {
    #[serde(default)]
    items: Vec<BaseItemDto>,
    baseline_item_name: Option<String>,
}
