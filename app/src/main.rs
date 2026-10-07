#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
#[cfg(debug_assertions)]
mod dev;
mod diagnostics;
mod downloads;
mod flicksync;
mod images;
mod offline;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use oneshot_capabilities::CapabilityManager;
use oneshot_catalog::Catalog;
use oneshot_player::presenter::HostWindow;
use oneshot_player::{Player, PlayerConfig};
use oneshot_storage::cache::MetadataCache;
use oneshot_storage::images::ImageCache;
use oneshot_storage::pin::PinGuard;
use oneshot_storage::{Paths, Store};
use parking_lot::{Mutex, RwLock};
use tauri::{Emitter, Manager};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

use crate::diagnostics::Diagnostics;
use crate::state::AppState;

/// Cached metadata older than this is dropped at startup. Well past every
/// TTL (TMDB answers: a week), so it only removes what nobody opens anymore.
const METADATA_MAX_AGE_SECS: i64 = 30 * 24 * 3600;

/// Log filter for a settings level; noisy webview crates stay at warn.
pub fn log_filter(level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(format!("{level},wry=warn,tao=warn,hyper=warn,reqwest=warn")))
}

fn init_tracing(diag: &Diagnostics) -> state::LogReload {
    let (filter, handle) = tracing_subscriber::reload::Layer::new(log_filter("info"));
    tracing_subscriber::registry().with(filter).with(fmt::layer().with_target(true)).with(diag.layer()).init();
    Arc::new(move |level: &str| {
        if let Err(e) = handle.reload(log_filter(level)) {
            tracing::warn!("log level change failed: {e}");
        }
    })
}

/// Where libmpv is looked for: next to the executable (packaged app), the
/// bundle resources, then the dev folder.
fn libmpv_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.to_path_buf());
    }
    if let Ok(res) = app.path().resource_dir() {
        dirs.push(res.join("libmpv"));
    }
    if cfg!(debug_assertions) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        dirs.push(root.join("third_party/mpv/windows-x64"));
        dirs.push(root.join("third_party/mpv/macos-arm64"));
        if cfg!(target_os = "macos") {
            // `brew install mpv` (arm64: Homebrew's default prefix; Intel: /usr/local).
            dirs.push(PathBuf::from("/opt/homebrew/lib"));
            dirs.push(PathBuf::from("/usr/local/lib"));
        }
    }
    dirs
}

/// The bundled subtitle fonts: the resource folder, or the sources in dev.
fn fonts_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|r| r.join("fonts"));
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/src/assets/fonts");
    bundled.into_iter().chain(cfg!(debug_assertions).then_some(dev)).find(|d| d.is_dir())
}

fn setup(app: &mut tauri::App, diag: Diagnostics, log_reload: state::LogReload) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let paths = Paths { config: app.path().app_config_dir()?, cache: app.path().app_cache_dir()? };
    let store = Store::open(paths.clone())?;
    let settings = store.settings();
    let profiles = store.profiles();
    log_reload(&settings.advanced.log_level);
    let identity = store.identity()?;
    let http = oneshot_net::client(&settings.network)?;
    let cache = Arc::new(MetadataCache::open(&paths.cache.join("cache.sqlite"))?);
    let images = ImageCache::new(paths.cache.join("images"), u64::from(settings.cache.image_cache_mib) * 1024 * 1024)?;

    let window = app.get_webview_window("main").ok_or("main window missing")?;
    #[cfg(target_os = "macos")]
    hide_traffic_lights(&window);
    let host = host_window(&window);
    let dispatch_handle = handle.clone();
    let emit_handle = handle.clone();
    let player = Player::new(
        PlayerConfig {
            libmpv_path: std::env::var_os("ONESHOT_LIBMPV").map(PathBuf::from),
            search_dirs: libmpv_dirs(&handle),
            fonts_dir: fonts_dir(&handle),
            host,
            dispatch: Arc::new(move |f| {
                if let Err(e) = dispatch_handle.run_on_main_thread(f) {
                    tracing::error!(target: "player", "UI dispatch failed: {e}");
                }
            }),
            runtime: match tauri::async_runtime::handle() {
                tauri::async_runtime::RuntimeHandle::Tokio(h) => h,
            },
        },
        Arc::new(move |event| {
            let _ = emit_handle.emit("player", event);
        }),
    );

    let state = Arc::new(AppState {
        servers: RwLock::new(store.servers()?),
        catalog: Catalog::new(Arc::clone(&cache), settings.cache.metadata_ttl_secs),
        metadata: cache,
        tmdb: RwLock::new(commands::people::tmdb_from_keychain(&http)),
        caps: CapabilityManager::new(),
        store,
        identity,
        settings: RwLock::new(settings),
        http: RwLock::new(http),
        images,
        player,
        diagnostics: diag,
        log_reload,
        plex_account: Mutex::new(None),
        pip_restore: Mutex::new(None),
        profiles: RwLock::new(profiles),
        active_profile: RwLock::new(None),
        pin_guards: Mutex::new(Default::default()),
        config_guard: Mutex::new(PinGuard::default()),
        offline: RwLock::new(Default::default()),
        verified_plex: RwLock::new(Default::default()),
        switching: tokio::sync::Mutex::new(()),
        settings_io: Mutex::new(()),
        flicksync: flicksync::Hub::new(handle.clone()),
        downloads: downloads::Downloads::new(handle.clone(), paths.config.clone()),
        cast: oneshot_cast::Caster::new(),
    });
    // Multi-user: resume the last profile, or wait for the picker (nothing
    // is loaded until someone is chosen). Off: every connection, as before.
    if !state.resume_last_profile() {
        state.restore_servers();
    }
    app.manage(Arc::clone(&state));
    state.catalog.set_local(state.downloads.library());
    state.downloads.start();
    #[cfg(debug_assertions)]
    tauri::async_runtime::block_on(dev::bootstrap(Arc::clone(&state)));

    // Probe capabilities in the background so the first playback is instant,
    // and trim the caches (the metadata one otherwise grows forever: every
    // title and person ever opened stays in it).
    let bg = Arc::clone(&state);
    std::thread::spawn(move || {
        bg.caps.refresh();
        if let Err(e) = bg.images.enforce_limit() {
            tracing::warn!(target: "cache", "image cache trim failed: {e}");
        }
        match bg.metadata.purge_older_than(METADATA_MAX_AGE_SECS) {
            Ok(0) => {}
            Ok(n) => tracing::info!(target: "cache", purged = n, "old metadata dropped"),
            Err(e) => tracing::warn!(target: "cache", "metadata cache trim failed: {e}"),
        }
    });

    let st = Arc::clone(&state);
    window.on_window_event(move |event| match event {
        // Monitor changes can change HDR state and refresh rate.
        tauri::WindowEvent::ScaleFactorChanged { .. } => {
            let st = Arc::clone(&st);
            std::thread::spawn(move || st.caps.refresh());
        }
        tauri::WindowEvent::Destroyed => st.player.shutdown(),
        _ => {}
    });
    Ok(())
}

/// Hides the native traffic lights: the UI draws its own in the title island.
/// The window keeps its native shape (corners, shadow, resizing, fullscreen).
#[cfg(target_os = "macos")]
fn hide_traffic_lights(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    let Ok(ptr) = window.ns_window() else { return };
    // SAFETY: Tauri hands out the window's live NSWindow, and this runs on the main thread (setup).
    let ns_window = unsafe { &*(ptr as *const NSWindow) };
    for kind in [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
        if let Some(button) = ns_window.standardWindowButton(kind) {
            button.setHidden(true);
        }
    }
}

fn host_window(window: &tauri::WebviewWindow) -> HostWindow {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        return HostWindow::Win32 { hwnd: hwnd.0 as isize };
    }
    #[cfg(target_os = "macos")]
    if let Ok(ns_view) = window.ns_view() {
        return HostWindow::AppKit { ns_view };
    }
    let _ = window;
    HostWindow::Other
}

fn main() {
    let diag = Diagnostics::default();
    let log_reload = init_tracing(&diag);
    // A panic on a worker thread is otherwise silent: put it in the logs (Settings › Debug).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "panic", "{info}");
        default_hook(info);
    }));
    tauri::Builder::default()
        // First plugin, as the docs require: a second launch brings the running window forward instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::updates::Updates::default())
        .register_asynchronous_uri_scheme_protocol("oneshot-img", |ctx, request, responder| {
            images::handle(ctx.app_handle(), request, responder);
        })
        .setup(move |app| setup(app, diag.clone(), Arc::clone(&log_reload)))
        .invoke_handler(tauri::generate_handler![
            commands::servers::servers_list,
            commands::servers::server_status,
            commands::servers::server_remove,
            commands::servers::server_set_enabled,
            commands::servers::jellyfin_probe,
            commands::servers::jellyfin_login,
            commands::servers::jellyfin_quick_connect_start,
            commands::servers::jellyfin_quick_connect_poll,
            commands::servers::plex_pin_start,
            commands::servers::plex_pin_poll,
            commands::servers::plex_add_servers,
            commands::profiles::profiles_state,
            commands::profiles::profiles_discover,
            commands::profiles::profiles_configure,
            commands::profiles::profile_check_pin,
            commands::profiles::profile_switch,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_set_pin,
            commands::profiles::profile_detach,
            commands::profiles::profile_merge,
            commands::profiles::profile_unmerge,
            commands::profiles::profile_delete,
            commands::catalog::home,
            commands::catalog::libraries,
            commands::catalog::items,
            commands::catalog::item,
            commands::catalog::item_cached,
            commands::catalog::children,
            commands::catalog::similar,
            commands::catalog::adjacent,
            commands::catalog::markers,
            commands::catalog::prerolls,
            commands::cast::cast_devices,
            commands::cast::cast_start,
            commands::cast::cast_command,
            commands::cast::cast_status,
            commands::cast::cast_stop,
            commands::catalog::search,
            commands::catalog::favorites,
            commands::people::tmdb_status,
            commands::people::tmdb_set_key,
            commands::people::tmdb_remove_key,
            commands::people::person_details,
            commands::people::person_server,
            commands::people::person_items,
            commands::catalog::set_played,
            commands::catalog::set_favorite,
            commands::catalog::genres,
            commands::catalog::by_genre,
            commands::catalog::recommendations,
            commands::flicksync::flicksync_status,
            commands::flicksync::flicksync_diagnose,
            commands::flicksync::flicksync_create,
            commands::flicksync::flicksync_join,
            commands::flicksync::flicksync_leave,
            commands::flicksync::flicksync_state,
            commands::flicksync::flicksync_select_media,
            commands::flicksync::flicksync_chat,
            commands::flicksync::flicksync_update_room,
            commands::flicksync::flicksync_close_room,
            commands::flicksync::flicksync_resync_media,
            commands::flicksync::flicksync_debug,
            commands::flicksync::flicksync_current_item,
            commands::flicksync::flicksync_invitation,
            commands::flicksync::flicksync_add_invitation,
            commands::flicksync::flicksync_clear_invitation,
            commands::downloads::downloads_status,
            commands::downloads::downloads_list,
            commands::downloads::downloads_enqueue,
            commands::downloads::downloads_pause,
            commands::downloads::downloads_resume,
            commands::downloads::downloads_remove,
            commands::downloads::downloads_clear,
            commands::downloads::downloads_open_folder,
            commands::offline::offline_check,
            commands::playback::play,
            commands::playback::player_reload,
            commands::playback::player_command,
            commands::playback::player_viewport,
            commands::playback::player_snapshot,
            commands::playback::player_stats,
            commands::system::settings_get,
            commands::system::settings_set,
            commands::system::capabilities,
            commands::system::about,
            commands::system::diagnostics,
            commands::system::set_fullscreen,
            commands::system::window_pip,
            commands::system::palette,
            commands::system::tmdb_palette,
            commands::system::cache_clear,
            commands::updates::update_check,
            commands::updates::update_install,
            commands::admin::admin_overview,
            commands::admin::admin_run_task,
            commands::admin::admin_scan_library,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Flick");
}
