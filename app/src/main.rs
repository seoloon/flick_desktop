#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
#[cfg(debug_assertions)]
mod dev;
mod diagnostics;
mod images;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use oneshot_capabilities::CapabilityManager;
use oneshot_catalog::Catalog;
use oneshot_player::presenter::HostWindow;
use oneshot_player::{Player, PlayerConfig};
use oneshot_storage::cache::MetadataCache;
use oneshot_storage::images::ImageCache;
use oneshot_storage::{Paths, Store};
use parking_lot::{Mutex, RwLock};
use tauri::{Emitter, Manager};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

use crate::diagnostics::Diagnostics;
use crate::state::AppState;

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
    }
    dirs
}

fn setup(app: &mut tauri::App, diag: Diagnostics, log_reload: state::LogReload) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let paths = Paths { config: app.path().app_config_dir()?, cache: app.path().app_cache_dir()? };
    let store = Store::open(paths.clone())?;
    let settings = store.settings();
    log_reload(&settings.advanced.log_level);
    let identity = store.identity()?;
    let http = oneshot_net::client(&settings.network)?;
    let cache = Arc::new(MetadataCache::open(&paths.cache.join("cache.sqlite"))?);
    let images = ImageCache::new(paths.cache.join("images"), u64::from(settings.cache.image_cache_mib) * 1024 * 1024)?;

    let window = app.get_webview_window("main").ok_or("main window missing")?;
    let host = host_window(&window);
    let dispatch_handle = handle.clone();
    let emit_handle = handle.clone();
    let player = Player::new(
        PlayerConfig {
            libmpv_path: std::env::var_os("ONESHOT_LIBMPV").map(PathBuf::from),
            search_dirs: libmpv_dirs(&handle),
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
        catalog: Catalog::new(cache, settings.cache.metadata_ttl_secs),
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
    });
    state.restore_servers();
    app.manage(Arc::clone(&state));
    #[cfg(debug_assertions)]
    tauri::async_runtime::block_on(dev::bootstrap(Arc::clone(&state)));

    // Probe capabilities in the background so the first playback is instant,
    // and trim the image cache.
    let bg = Arc::clone(&state);
    std::thread::spawn(move || {
        bg.caps.refresh();
        if let Err(e) = bg.images.enforce_limit() {
            tracing::warn!(target: "cache", "image cache trim failed: {e}");
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

fn host_window(window: &tauri::WebviewWindow) -> HostWindow {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        return HostWindow::Win32 { hwnd: hwnd.0 as isize };
    }
    let _ = window;
    HostWindow::Other
}

fn main() {
    let diag = Diagnostics::default();
    let log_reload = init_tracing(&diag);
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("oneshot-img", |ctx, request, responder| {
            images::handle(ctx.app_handle(), request, responder);
        })
        .setup(move |app| setup(app, diag.clone(), Arc::clone(&log_reload)))
        .invoke_handler(tauri::generate_handler![
            commands::servers::servers_list,
            commands::servers::server_status,
            commands::servers::server_remove,
            commands::servers::jellyfin_probe,
            commands::servers::jellyfin_login,
            commands::servers::jellyfin_quick_connect_start,
            commands::servers::jellyfin_quick_connect_poll,
            commands::servers::plex_pin_start,
            commands::servers::plex_pin_poll,
            commands::servers::plex_add_servers,
            commands::catalog::home,
            commands::catalog::libraries,
            commands::catalog::items,
            commands::catalog::item,
            commands::catalog::item_cached,
            commands::catalog::children,
            commands::catalog::similar,
            commands::catalog::adjacent,
            commands::catalog::markers,
            commands::catalog::search,
            commands::catalog::set_played,
            commands::catalog::set_favorite,
            commands::playback::play,
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
            commands::system::palette,
            commands::system::cache_clear,
            commands::admin::admin_overview,
            commands::admin::admin_run_task,
            commands::admin::admin_scan_library,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Flick");
}
