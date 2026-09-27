//! Subset of the Jellyfin OpenAPI schema we consume. Unknown fields are
//! ignored; every field is optional unless the server always sends it, so a
//! newer/older server never breaks deserialization.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PublicSystemInfo {
    pub server_name: Option<String>,
    pub version: Option<String>,
    pub id: String,
    pub product_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthenticationResult {
    pub user: UserDto,
    pub access_token: String,
    pub server_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub policy: Option<UserPolicy>,
    pub last_activity_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserPolicy {
    #[serde(default)]
    pub is_administrator: bool,
    #[serde(default)]
    pub is_disabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QueryResult<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    pub total_record_count: Option<u32>,
    pub start_index: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserItemData {
    #[serde(default)]
    pub played: bool,
    #[serde(default)]
    pub play_count: u32,
    #[serde(default)]
    pub playback_position_ticks: i64,
    #[serde(default)]
    pub is_favorite: bool,
    pub last_played_date: Option<String>,
    pub unplayed_item_count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PersonDto {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub r#type: Option<String>,
    pub primary_image_tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NameIdPair {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BaseItemDto {
    pub id: String,
    pub name: Option<String>,
    pub original_title: Option<String>,
    pub sort_name: Option<String>,
    pub r#type: String,
    pub collection_type: Option<String>,
    pub overview: Option<String>,
    #[serde(default)]
    pub taglines: Vec<String>,
    pub production_year: Option<i32>,
    pub premiere_date: Option<String>,
    pub date_created: Option<String>,
    pub run_time_ticks: Option<i64>,
    pub official_rating: Option<String>,
    pub community_rating: Option<f32>,
    pub critic_rating: Option<f32>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub studios: Vec<NameIdPair>,
    #[serde(default)]
    pub people: Vec<PersonDto>,
    #[serde(default)]
    pub provider_ids: HashMap<String, String>,
    pub user_data: Option<UserItemData>,
    pub child_count: Option<u32>,
    pub recursive_item_count: Option<u32>,

    // Series/season/episode placement
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub parent_index_number: Option<u32>,
    pub index_number: Option<u32>,
    pub index_number_end: Option<u32>,

    // Images
    #[serde(default)]
    pub image_tags: HashMap<String, String>,
    #[serde(default)]
    pub backdrop_image_tags: Vec<String>,
    pub series_primary_image_tag: Option<String>,
    pub parent_backdrop_item_id: Option<String>,
    #[serde(default)]
    pub parent_backdrop_image_tags: Vec<String>,
    pub parent_thumb_item_id: Option<String>,
    pub parent_thumb_image_tag: Option<String>,
    pub parent_logo_item_id: Option<String>,
    pub parent_logo_image_tag: Option<String>,
    #[serde(default)]
    pub image_blur_hashes: HashMap<String, HashMap<String, String>>,

    #[serde(default)]
    pub media_sources: Vec<MediaSourceInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSourceInfo {
    pub id: String,
    pub name: Option<String>,
    pub container: Option<String>,
    pub size: Option<i64>,
    pub bitrate: Option<i64>,
    pub run_time_ticks: Option<i64>,
    #[serde(default)]
    pub media_streams: Vec<MediaStream>,
    #[serde(default)]
    pub supports_direct_play: bool,
    #[serde(default)]
    pub supports_direct_stream: bool,
    #[serde(default)]
    pub supports_transcoding: bool,
    pub transcoding_url: Option<String>,
    #[serde(default)]
    pub transcode_reasons: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaStream {
    pub index: u32,
    pub r#type: String,
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub level: Option<f32>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_depth: Option<u8>,
    pub bit_rate: Option<i64>,
    pub real_frame_rate: Option<f32>,
    pub average_frame_rate: Option<f32>,
    pub channels: Option<u8>,
    pub channel_layout: Option<String>,
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_forced: bool,
    #[serde(default)]
    pub is_hearing_impaired: bool,
    #[serde(default)]
    pub is_external: bool,
    #[serde(default)]
    pub is_interlaced: bool,
    pub video_range_type: Option<String>,
    pub dv_profile: Option<u8>,
    pub dv_bl_signal_compatibility_id: Option<u8>,
    pub el_present_flag: Option<u8>,
    pub delivery_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackInfoResponse {
    #[serde(default)]
    pub media_sources: Vec<MediaSourceInfo>,
    pub play_session_id: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackInfoRequest {
    pub user_id: String,
    pub max_streaming_bitrate: Option<u64>,
    pub start_time_ticks: Option<i64>,
    pub audio_stream_index: Option<u32>,
    pub subtitle_stream_index: Option<i64>,
    pub media_source_id: Option<String>,
    pub device_profile: serde_json::Value,
    pub enable_direct_play: bool,
    pub enable_direct_stream: bool,
    pub enable_transcoding: bool,
    pub allow_video_stream_copy: bool,
    pub allow_audio_stream_copy: bool,
    pub auto_open_live_stream: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackProgressInfo {
    pub item_id: String,
    pub media_source_id: String,
    pub play_session_id: Option<String>,
    pub position_ticks: i64,
    pub is_paused: bool,
    pub is_muted: bool,
    pub volume_level: u8,
    pub play_method: &'static str,
    pub audio_stream_index: Option<u32>,
    pub subtitle_stream_index: Option<i64>,
    pub can_seek: bool,
    pub event_name: Option<&'static str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSegmentDto {
    pub r#type: String,
    pub start_ticks: i64,
    pub end_ticks: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuickConnectResult {
    pub secret: String,
    pub code: String,
    #[serde(default)]
    pub authenticated: bool,
}

// ------------------------------------------------------------------ admin

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SystemInfo {
    pub server_name: Option<String>,
    pub version: Option<String>,
    pub operating_system_display_name: Option<String>,
    pub has_update_available: Option<bool>,
    pub has_pending_restart: Option<bool>,
    pub local_address: Option<String>,
    pub encoder_location: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SessionInfo {
    pub id: String,
    pub user_name: Option<String>,
    pub client: Option<String>,
    pub device_name: Option<String>,
    pub now_playing_item: Option<BaseItemRef>,
    pub play_state: Option<PlayState>,
    pub transcoding_info: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BaseItemRef {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayState {
    #[serde(default)]
    pub is_paused: bool,
    pub play_method: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TaskInfo {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub state: String,
    pub current_progress_percentage: Option<f32>,
    pub last_execution_result: Option<TaskResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TaskResult {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LogFile {
    pub name: String,
}
