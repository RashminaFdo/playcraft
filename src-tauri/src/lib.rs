//! PlayCraft Native Rust Core Library
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

pub mod error;
pub mod input_bridge;
pub mod mobile_server;
pub mod models;
pub mod optimizer;
pub mod scanner;

use error::{PlayCraftError, PlayCraftResult};
use input_bridge::{
    get_connected_xinput_count, init_virtual_x360_controller, is_vigem_bus_available,
    send_virtual_controller_button, shutdown_virtual_x360_controller, spawn_xinput_poller,
};
use mobile_server::{
    detect_lan_ip, setup_adb_usb_bridge, start_mobile_server, MobileServerState,
};
use models::{
    ActivityCard, AppUpdateInfo, AuthorMetadata, DiskSpec, DisplayModeSpec, DrmBypassConfig,
    FullSystemSpecs, GameEntry, GameSessionEntry, GameSource, GameWorkspaceConfigUpdate, GpuSpec,
    InspectedFileResult, LauncherSettings, LocalMediaItem, MobilePairingInfo, MonitorSpec,
    NewGameRequest, OptimizerConfig, OptimizerReport, RestorableProcess, SaveBackupEntry,
    SystemTelemetry, WallpaperItem, APP_TITLE, AUTHOR_DISPLAY,
};
use optimizer::{restore_terminated_processes, run_deep_optimization};
use rand::Rng;
use scanner::{
    build_initial_library, calculate_directory_size_gb, clean_trademark_text,
    detect_likely_save_folder, inspect_executable_file, resolve_game_root_folder, scan_all_stores,
    scan_directory_for_games, spawn_game_process,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

static FORCE_EXIT_APPROVED: AtomicBool = AtomicBool::new(false);
const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/RashminaFdo/playcraft/releases/latest";
const GITHUB_RELEASES_PAGE: &str = "https://github.com/RashminaFdo/playcraft/releases/latest";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 86_400; // 24 hours

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolves a writable data directory for settings, game library, backups & session history.
/// Preserves existing portable bundles (`playcraft_library.json` beside exe/cwd) while safely
/// isolating Start Menu / Windows Startup launches into `%LOCALAPPDATA%\PlayCraft`.
pub fn resolve_playcraft_data_dir() -> PathBuf {
    let is_system_dir = |p: &std::path::Path| {
        let lower = p.to_string_lossy().to_lowercase();
        lower.contains("\\windows\\system32") || lower.contains("\\program files")
    };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            if !is_system_dir(exe_dir)
                && (exe_dir.join("playcraft_library.json").exists()
                    || exe_dir.join("playcraft_settings.json").exists())
            {
                return exe_dir.to_path_buf();
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        if !is_system_dir(&cwd)
            && (cwd.join("playcraft_library.json").exists()
                || cwd.join("playcraft_settings.json").exists())
        {
            return cwd;
        }
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let app_dir = PathBuf::from(local_app_data).join("PlayCraft");
        let _ = std::fs::create_dir_all(&app_dir);
        return app_dir;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn parse_semver_tuple(raw: &str) -> (u64, u64, u64, bool) {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let (core, is_prerelease) = match clean.split_once('-') {
        Some((c, _pre)) => (c, true),
        None => (clean, false),
    };
    let mut parts = core.split('.').filter_map(|s| s.parse::<u64>().ok());
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    (major, minor, patch, is_prerelease)
}

pub fn is_remote_version_newer(current: &str, remote: &str) -> bool {
    let (cmaj, cmin, cpat, cpre) = parse_semver_tuple(current);
    let (rmaj, rmin, rpat, rpre) = parse_semver_tuple(remote);
    if (rmaj, rmin, rpat) != (cmaj, cmin, cpat) {
        return (rmaj, rmin, rpat) > (cmaj, cmin, cpat);
    }
    cpre && !rpre
}
use sysinfo::{Disks, Pid, System};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tracing::info;

pub struct AppState {
    pub games: Arc<Mutex<Vec<GameEntry>>>,
    pub settings: Mutex<LauncherSettings>,
    pub optimizer_config: Mutex<OptimizerConfig>,
    pub restorable_processes: Arc<Mutex<Vec<RestorableProcess>>>,
    pub active_game: Arc<Mutex<Option<(String, u32)>>>,
    pub overlay_active: Mutex<bool>,
    pub mobile_server: Arc<MobileServerState>,
    pub storage_path: PathBuf,
    pub settings_path: PathBuf,
}

impl AppState {
    pub fn persist_library(&self) {
        if let Ok(games) = self.games.lock() {
            if let Ok(json) = serde_json::to_string_pretty(&*games) {
                let _ = std::fs::write(&self.storage_path, json);
            }
        }
    }

    pub fn persist_settings(&self) {
        if let Ok(settings) = self.settings.lock() {
            if let Ok(json) = serde_json::to_string_pretty(&*settings) {
                let _ = std::fs::write(&self.settings_path, json);
            }
        }
    }
}

#[tauri::command]
fn get_author_metadata() -> AuthorMetadata {
    AuthorMetadata::default()
}

#[tauri::command]
fn get_available_wallpapers(state: State<'_, AppState>) -> Vec<WallpaperItem> {
    let mut list = vec![
        WallpaperItem {
            id: "wall-1".to_string(),
            name: "Cosmic Singularity Vortex".to_string(),
            url: "assets/wallpapers/wall_1.jpeg".to_string(),
            subtitle: "Deep Space Cyan & Violet Nebula".to_string(),
        },
        WallpaperItem {
            id: "wall-2".to_string(),
            name: "Planetary Cyber Grid".to_string(),
            url: "assets/wallpapers/wall_2.jpeg".to_string(),
            subtitle: "Holographic Globe & Synthwave Horizon".to_string(),
        },
        WallpaperItem {
            id: "wall-3".to_string(),
            name: "Ancient Cyber Monolith".to_string(),
            url: "assets/wallpapers/wall_3.jpeg".to_string(),
            subtitle: "Moonlit Circuit Ruins & Bioluminescence".to_string(),
        },
        WallpaperItem {
            id: "wall-4".to_string(),
            name: "Neon Night Cityscape".to_string(),
            url: "assets/wallpapers/wall_4.jpeg".to_string(),
            subtitle: "Cyberpunk Skyline & Ocean Reflections".to_string(),
        },
        WallpaperItem {
            id: "wall-5".to_string(),
            name: "Mecha Cyber Foundry".to_string(),
            url: "assets/wallpapers/wall_5.jpeg".to_string(),
            subtitle: "Industrial Reactor & Cyan Plasma Sparks".to_string(),
        },
    ];

    if let Ok(settings) = state.settings.lock() {
        for custom in &settings.custom_wallpapers {
            list.push(custom.clone());
        }
    }
    list
}

#[tauri::command]
fn get_launcher_settings(state: State<'_, AppState>) -> PlayCraftResult<LauncherSettings> {
    let guard = state
        .settings
        .lock()
        .map_err(|_| PlayCraftError::ScannerError("Settings mutex poisoned".to_string()))?;
    Ok(guard.clone())
}

fn resolve_target_monitor(
    win: &tauri::WebviewWindow,
    target_monitor: &str,
) -> Option<tauri::Monitor> {
    let monitors = win.available_monitors().unwrap_or_default();
    let primary = win.primary_monitor().ok().flatten();
    let trimmed = target_monitor.trim();

    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("primary") {
        return primary.or_else(|| monitors.into_iter().next());
    }

    if let Some(found) = monitors.iter().find(|m| {
        m.name()
            .map(|n| n.eq_ignore_ascii_case(trimmed))
            .unwrap_or(false)
    }) {
        return Some(found.clone());
    }

    if let Ok(idx) = trimmed.parse::<usize>() {
        if let Some(m) = monitors.get(idx) {
            return Some(m.clone());
        }
    }

    primary.or_else(|| monitors.into_iter().next())
}

pub fn apply_window_mode_and_monitor(
    app_handle: &AppHandle,
    window_mode: &str,
    target_monitor: &str,
) {
    let Some(win) = app_handle.get_webview_window("main") else {
        return;
    };

    let monitor = resolve_target_monitor(&win, target_monitor);
    let mode = window_mode.trim().to_ascii_lowercase();

    match mode.as_str() {
        "borderless" | "windowed_fullscreen" => {
            let _ = win.set_fullscreen(false);
            let _ = win.set_decorations(false);
            if let Some(m) = monitor {
                let pos = *m.position();
                let size = *m.size();
                let _ = win.set_position(tauri::Position::Physical(pos));
                let _ = win.set_size(tauri::Size::Physical(size));
            }
        }
        "windowed" | "normal" => {
            let _ = win.set_fullscreen(false);
            let _ = win.set_decorations(true);
            let _ = win.set_resizable(true);
            if let Some(m) = monitor {
                let m_pos = *m.position();
                let m_size = *m.size();
                let target_w = ((m_size.width as f32) * 0.84).round().clamp(1120.0, 1680.0) as u32;
                let target_h = ((m_size.height as f32) * 0.84).round().clamp(660.0, 960.0) as u32;
                let cx = m_pos.x + ((m_size.width.saturating_sub(target_w)) / 2) as i32;
                let cy = m_pos.y + ((m_size.height.saturating_sub(target_h)) / 2) as i32;
                let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
                    target_w, target_h,
                )));
                let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                    cx, cy,
                )));
            }
        }
        _ => {
            // Default: exclusive Fullscreen positioned cleanly on the selected monitor (Primary by default)
            let _ = win.set_fullscreen(false);
            if let Some(m) = monitor {
                let pos = *m.position();
                let _ = win.set_position(tauri::Position::Physical(pos));
            }
            let _ = win.set_decorations(true);
            let _ = win.set_fullscreen(true);
        }
    }
}

#[tauri::command]
fn get_available_monitors(app_handle: AppHandle) -> PlayCraftResult<Vec<MonitorSpec>> {
    let Some(win) = app_handle.get_webview_window("main") else {
        return Ok(Vec::new());
    };
    let primary_pos = win.primary_monitor().ok().flatten().map(|m| *m.position());
    let primary_name = win
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());

    let mut out = Vec::new();
    for (idx, m) in win.available_monitors().unwrap_or_default().into_iter().enumerate() {
        let m_name = m
            .name()
            .cloned()
            .unwrap_or_else(|| format!("DISPLAY-{}", idx + 1));
        let pos = *m.position();
        let sz = *m.size();
        let is_primary = primary_name
            .as_ref()
            .map(|pn| pn == &m_name)
            .unwrap_or(false)
            || primary_pos.map(|pp| pp == pos).unwrap_or(idx == 0);
        let suffix = if is_primary { " [Primary]" } else { "" };
        out.push(MonitorSpec {
            name: m_name.clone(),
            label: format!("Display {} ({}x{}){}", idx + 1, sz.width, sz.height, suffix),
            width: sz.width,
            height: sz.height,
            x: pos.x,
            y: pos.y,
            is_primary,
        });
    }
    Ok(out)
}

#[tauri::command]
fn save_launcher_settings(
    app_handle: AppHandle,
    mut settings: LauncherSettings,
    state: State<'_, AppState>,
) -> PlayCraftResult<LauncherSettings> {
    if settings.window_mode.trim().is_empty() {
        settings.window_mode = if settings.start_fullscreen {
            "fullscreen".to_string()
        } else {
            "windowed".to_string()
        };
    }
    settings.start_fullscreen = settings.window_mode != "windowed";

    apply_window_mode_and_monitor(&app_handle, &settings.window_mode, &settings.target_monitor);

    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok((run_key, _)) =
            hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        {
            if settings.launch_on_windows_startup {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = run_key.set_value("PlayCraft", &exe.to_string_lossy().to_string());
                }
            } else {
                let _ = run_key.delete_value("PlayCraft");
            }
        }
    }

    if let Ok(mut guard) = state.settings.lock() {
        // Preserve backend updater state unless incoming explicitly has newer check timestamp
        if settings.last_update_check_ts < guard.last_update_check_ts {
            settings.last_update_check_ts = guard.last_update_check_ts;
            if settings.latest_known_version.is_empty() {
                settings.latest_known_version = guard.latest_known_version.clone();
            }
            if settings.latest_release_url.is_empty() {
                settings.latest_release_url = guard.latest_release_url.clone();
            }
        }
        *guard = settings.clone();
    }
    state.persist_settings();
    Ok(settings)
}

#[tauri::command]
fn open_external_url(url: String) -> PlayCraftResult<()> {
    let trimmed = url.trim().to_string();
    let allowed = trimmed.starts_with("https://github.com/RashminaFdo/playcraft")
        || trimmed.starts_with("https://www.rashmina.dev")
        || trimmed.starts_with("https://rashmina.dev")
        || trimmed.starts_with("https://github.com/nefarius/ViGEmBus");
    if !allowed {
        return Err(PlayCraftError::ScannerError(format!(
            "Blocked unsupported external URL: {}",
            trimmed
        )));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", &trimmed])
            .creation_flags(0x08000000)
            .spawn();
    }
    Ok(())
}

#[tauri::command]
async fn check_for_app_update(
    force: bool,
    state: State<'_, AppState>,
) -> PlayCraftResult<AppUpdateInfo> {
    let current_ver = env!("CARGO_PKG_VERSION").to_string();
    let now_secs = now_unix_secs();

    let (last_ts, cached_ver, cached_url) = state
        .settings
        .lock()
        .map(|g| {
            (
                g.last_update_check_ts,
                g.latest_known_version.clone(),
                g.latest_release_url.clone(),
            )
        })
        .unwrap_or((0, String::new(), String::new()));

    // Honor daily 24-hour background throttle unless manually triggered via Settings
    if !force && last_ts > 0 && now_secs.saturating_sub(last_ts) < UPDATE_CHECK_INTERVAL_SECS {
        let has_cached_update = !cached_ver.is_empty()
            && is_remote_version_newer(&current_ver, &cached_ver);
        return Ok(AppUpdateInfo {
            current_version: current_ver.clone(),
            latest_version: if cached_ver.is_empty() {
                current_ver.clone()
            } else {
                cached_ver
            },
            has_update: has_cached_update,
            release_title: if has_cached_update {
                "New PlayCraft Update Available".to_string()
            } else {
                String::new()
            },
            release_notes: String::new(),
            release_url: if cached_url.is_empty() {
                GITHUB_RELEASES_PAGE.to_string()
            } else {
                cached_url
            },
            setup_download_url: String::new(),
            checked_at_ts: last_ts,
            offline_or_error: false,
            status_message: if has_cached_update {
                "Update available on GitHub Releases".to_string()
            } else {
                format!("Up to date (v{})", current_ver)
            },
        });
    }

    let current_ver_bg = current_ver.clone();
    let fetch_res = tokio::task::spawn_blocking(move || -> Result<AppUpdateInfo, String> {
        let mut cmd = std::process::Command::new("curl.exe");
        cmd.args([
            "-sS",
            "-L",
            "-w",
            "\n__HTTP_CODE:%{http_code}",
            "--connect-timeout",
            "6",
            "--max-time",
            "12",
            "-A",
            "PlayCraft-Updater/1.0",
            "-H",
            "Accept: application/vnd.github+json",
            GITHUB_RELEASES_API,
        ]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let output = cmd.output().map_err(|e| format!("Network unavailable: {}", e))?;
        if !output.status.success() && output.stdout.is_empty() {
            return Err("Offline or unable to reach GitHub Releases".to_string());
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let (body_part, code_part) = match raw.rsplit_once("\n__HTTP_CODE:") {
            Some((b, c)) => (b, c.trim()),
            None => (raw.as_str(), "0"),
        };

        if code_part == "404" {
            // No published release tag yet on GitHub — treat cleanly as up-to-date
            return Ok(AppUpdateInfo {
                current_version: current_ver_bg.clone(),
                latest_version: current_ver_bg.clone(),
                has_update: false,
                release_title: String::new(),
                release_notes: String::new(),
                release_url: GITHUB_RELEASES_PAGE.to_string(),
                setup_download_url: String::new(),
                checked_at_ts: now_secs,
                offline_or_error: false,
                status_message: format!("Up to date (v{})", current_ver_bg),
            });
        }

        if code_part != "200" {
            return Err(format!("GitHub API HTTP {}", code_part));
        }

        let val: serde_json::Value =
            serde_json::from_str(body_part).map_err(|e| format!("Invalid GitHub JSON: {}", e))?;
        let tag = val
            .get("tag_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if tag.is_empty() {
            return Err("Release tag missing in GitHub response".to_string());
        }

        let release_url = val
            .get("html_url")
            .and_then(|v| v.as_str())
            .filter(|s| s.starts_with("https://"))
            .unwrap_or(GITHUB_RELEASES_PAGE)
            .to_string();
        let release_title = val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&tag)
            .to_string();
        let release_notes = val
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(500)
            .collect::<String>();

        let mut setup_dl = String::new();
        if let Some(assets) = val.get("assets").and_then(|a| a.as_array()) {
            for asset in assets {
                let name = asset
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let dl = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if (name.contains("setup") || name.ends_with(".exe")) && dl.starts_with("https://")
                {
                    setup_dl = dl.to_string();
                    break;
                }
            }
        }

        let has_update = is_remote_version_newer(&current_ver_bg, &tag);
        let status_message = if has_update {
            format!("Update {} is ready on GitHub!", tag)
        } else {
            format!("Up to date (v{})", current_ver_bg)
        };

        Ok(AppUpdateInfo {
            current_version: current_ver_bg,
            latest_version: tag,
            has_update,
            release_title,
            release_notes,
            release_url,
            setup_download_url: setup_dl,
            checked_at_ts: now_secs,
            offline_or_error: false,
            status_message,
        })
    })
    .await;

    match fetch_res {
        Ok(Ok(info)) => {
            if let Ok(mut guard) = state.settings.lock() {
                guard.last_update_check_ts = info.checked_at_ts;
                guard.latest_known_version = info.latest_version.clone();
                guard.latest_release_url = info.release_url.clone();
            }
            state.persist_settings();
            Ok(info)
        }
        Ok(Err(err_msg)) => Ok(AppUpdateInfo {
            current_version: current_ver,
            latest_version: cached_ver,
            has_update: false,
            release_title: String::new(),
            release_notes: String::new(),
            release_url: if cached_url.is_empty() {
                GITHUB_RELEASES_PAGE.to_string()
            } else {
                cached_url
            },
            setup_download_url: String::new(),
            checked_at_ts: last_ts,
            offline_or_error: true,
            status_message: err_msg,
        }),
        Err(join_err) => Err(PlayCraftError::ScannerError(join_err.to_string())),
    }
}

#[tauri::command]
fn toggle_fullscreen_window(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> PlayCraftResult<String> {
    let (next_mode, target_mon) = {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Settings mutex poisoned".to_string()))?;
        let next = match guard.window_mode.as_str() {
            "fullscreen" => "borderless",
            "borderless" => "windowed",
            _ => "fullscreen",
        }
        .to_string();
        guard.window_mode = next.clone();
        guard.start_fullscreen = next != "windowed";
        (next, guard.target_monitor.clone())
    };

    apply_window_mode_and_monitor(&app_handle, &next_mode, &target_mon);
    state.persist_settings();
    Ok(next_mode)
}

fn clean_shutdown_app(app_handle: &AppHandle, state: &AppState) {
    FORCE_EXIT_APPROVED.store(true, Ordering::SeqCst);
    if let Some(win) = app_handle.get_webview_window("main") {
        let _ = win.hide();
    }
    let restore_arc = state.restorable_processes.clone();
    let app = app_handle.clone();
    std::thread::spawn(move || {
        shutdown_virtual_x360_controller();
        if let Ok(mut restore_guard) = restore_arc.lock() {
            let _ = restore_terminated_processes(&mut restore_guard);
        }
        app.exit(0);
        std::thread::sleep(std::time::Duration::from_millis(160));
        std::process::exit(0);
    });
}

#[tauri::command]
fn quit_application(app_handle: AppHandle, state: State<'_, AppState>) {
    clean_shutdown_app(&app_handle, &state);
}

#[tauri::command]
fn minimize_launcher_to_tray(app_handle: AppHandle) {
    if let Some(win) = app_handle.get_webview_window("main") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn setup_usb_mobile_bridge(state: State<'_, AppState>) -> PlayCraftResult<String> {
    setup_adb_usb_bridge(state.mobile_server.port).map_err(PlayCraftError::ScannerError)
}

fn collect_full_system_specs_sync() -> PlayCraftResult<FullSystemSpecs> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let os_name = System::name().unwrap_or_else(|| "Microsoft Windows".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "11 / 10".to_string());
    let kernel_version = System::kernel_version().unwrap_or_else(|| "NT".to_string());
    let hostname = System::host_name().unwrap_or_else(|| "WINDOWS-PC".to_string());
    let architecture = std::env::consts::ARCH.to_uppercase();

    let up_secs = System::uptime();
    let up_hours = up_secs / 3600;
    let up_mins = (up_secs % 3600) / 60;
    let uptime_formatted = format!("{}h {}m", up_hours, up_mins);

    let physical_cores = sys.physical_core_count().unwrap_or_else(|| sys.cpus().len());
    let logical_threads = sys.cpus().len();
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(read_cpu_brand_from_registry);
    let cpu_vendor = sys
        .cpus()
        .first()
        .map(|c| c.vendor_id().trim().to_string())
        .unwrap_or_else(|| "x86_64".to_string());
    let cpu_frequency_mhz = sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
    let cpu_usage_percent = (sys.global_cpu_info().cpu_usage() * 10.0).round() / 10.0;

    let total_bytes = sys.total_memory();
    let used_bytes = sys.used_memory();
    let avail_bytes = sys.available_memory();
    let ram_total_gb = ((total_bytes as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;
    let ram_used_gb = ((used_bytes as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;
    let ram_available_gb =
        ((avail_bytes as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;
    let ram_usage_percent = if total_bytes > 0 {
        ((used_bytes as f64 / total_bytes as f64) * 1000.0).round() as f32 / 10.0
    } else {
        0.0
    };

    let swap_total_gb =
        ((sys.total_swap() as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;
    let swap_used_gb =
        ((sys.used_swap() as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;

    let (motherboard, bios_version) = read_motherboard_and_bios();
    let gpus = read_installed_gpus();
    let primary_display_resolution = read_primary_display_resolution();

    let disks_info = Disks::new_with_refreshed_list();
    let mut disks = Vec::new();
    for d in disks_info.list() {
        let total = d.total_space();
        if total == 0 {
            continue;
        }
        let avail = d.available_space();
        let used = total.saturating_sub(avail);
        let total_gb = ((total as f64 / 1_073_741_824.0) * 10.0).round() as f32 / 10.0;
        let available_gb = ((avail as f64 / 1_073_741_824.0) * 10.0).round() as f32 / 10.0;
        let used_gb = ((used as f64 / 1_073_741_824.0) * 10.0).round() as f32 / 10.0;
        let usage_percent = ((used as f64 / total as f64) * 1000.0).round() as f32 / 10.0;

        disks.push(DiskSpec {
            name: d.name().to_string_lossy().to_string(),
            mount_point: d.mount_point().to_string_lossy().to_string(),
            file_system: d.file_system().to_string_lossy().to_string(),
            disk_type: format!("{:?}", d.kind()),
            total_gb,
            available_gb,
            used_gb,
            usage_percent,
        });
    }

    Ok(FullSystemSpecs {
        os_name,
        os_version,
        kernel_version,
        hostname,
        architecture,
        uptime_formatted,
        motherboard,
        bios_version,
        cpu_brand,
        cpu_vendor,
        physical_cores,
        logical_threads,
        cpu_frequency_mhz,
        cpu_usage_percent,
        gpus,
        ram_total_gb,
        ram_used_gb,
        ram_available_gb,
        ram_usage_percent,
        swap_total_gb,
        swap_used_gb,
        primary_display_resolution,
        disks,
    })
}

fn read_cpu_brand_from_registry() -> String {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(key) = hklm.open_subkey("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0") {
            if let Ok(name) = key.get_value::<String, _>("ProcessorNameString") {
                return name.trim().to_string();
            }
        }
    }
    "x86_64 Processor".to_string()
}

fn read_motherboard_and_bios() -> (String, String) {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(key) = hklm.open_subkey("HARDWARE\\DESCRIPTION\\System\\BIOS") {
            let mfg = key
                .get_value::<String, _>("BaseBoardManufacturer")
                .unwrap_or_default();
            let prod = key
                .get_value::<String, _>("BaseBoardProduct")
                .unwrap_or_default();
            let bios_vendor = key.get_value::<String, _>("BIOSVendor").unwrap_or_default();
            let bios_ver = key
                .get_value::<String, _>("BIOSVersion")
                .unwrap_or_default();
            let board = format!("{} {}", mfg.trim(), prod.trim()).trim().to_string();
            let bios = format!("{} {}", bios_vendor.trim(), bios_ver.trim())
                .trim()
                .to_string();
            return (
                if board.is_empty() {
                    "Standard PC Motherboard".to_string()
                } else {
                    board
                },
                if bios.is_empty() {
                    "UEFI BIOS".to_string()
                } else {
                    bios
                },
            );
        }
    }
    ("System Motherboard".to_string(), "UEFI".to_string())
}

fn read_installed_gpus() -> Vec<GpuSpec> {
    let mut gpus = Vec::new();
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let display_class =
            "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}";
        if let Ok(class_key) = hklm.open_subkey(display_class) {
            for sub in ["0000", "0001", "0002", "0003"] {
                if let Ok(adapter) = class_key.open_subkey(sub) {
                    let Ok(desc) = adapter.get_value::<String, _>("DriverDesc") else {
                        continue;
                    };
                    if desc.trim().is_empty() {
                        continue;
                    }
                    let vendor = adapter
                        .get_value::<String, _>("ProviderName")
                        .unwrap_or_else(|_| "Display Adapter".to_string());
                    let driver_version = adapter
                        .get_value::<String, _>("DriverVersion")
                        .unwrap_or_else(|_| "Installed".to_string());

                    let vram_bytes: u64 = adapter
                        .get_value::<u64, _>("HardwareInformation.qwMemorySize")
                        .or_else(|_| {
                            adapter
                                .get_value::<u32, _>("HardwareInformation.MemorySize")
                                .map(|v| v as u64)
                        })
                        .unwrap_or(0);
                    let vram_gb =
                        ((vram_bytes as f64 / 1_073_741_824.0) * 10.0).round() as f32 / 10.0;

                    if !gpus.iter().any(|g: &GpuSpec| g.name == desc) {
                        gpus.push(GpuSpec {
                            name: desc,
                            vendor,
                            driver_version,
                            vram_gb,
                        });
                    }
                }
            }
        }
    }
    if gpus.is_empty() {
        gpus.push(GpuSpec {
            name: "Windows DirectX 12 Hardware GPU".to_string(),
            vendor: "WDDM Display Adapter".to_string(),
            driver_version: "Active".to_string(),
            vram_gb: 0.0,
        });
    }
    gpus
}

fn read_primary_display_resolution() -> String {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
        };
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w > 0 && h > 0 {
            return format!("{} x {}", w, h);
        }
    }
    "1920 x 1080".to_string()
}

#[tauri::command]
async fn get_full_system_specs() -> PlayCraftResult<FullSystemSpecs> {
    tokio::task::spawn_blocking(collect_full_system_specs_sync)
        .await
        .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?
}

#[tauri::command]
async fn get_game_library(state: State<'_, AppState>) -> PlayCraftResult<Vec<GameEntry>> {
    let games_arc = state.games.clone();
    let storage_path = state.storage_path.clone();

    tokio::task::spawn_blocking(move || -> PlayCraftResult<Vec<GameEntry>> {
        let mut guard = games_arc
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
        let mut changed = false;
        for g in guard.iter_mut() {
            if !g.exe_path.is_empty() && !g.exe_path.starts_with("steam://") {
                let p = std::path::Path::new(&g.exe_path);
                if p.exists() {
                    let root = resolve_game_root_folder(p);
                    if root.exists() && root.is_dir() {
                        let real_sz = calculate_directory_size_gb(&root, Some(p));
                        if real_sz > g.size_on_disk_gb || (g.size_on_disk_gb - real_sz).abs() > 0.05 {
                            g.size_on_disk_gb = real_sz;
                            changed = true;
                        }
                    }
                }
            }
        }
        let result = guard.clone();
        drop(guard);
        if changed {
            if let Ok(json) = serde_json::to_string_pretty(&result) {
                let _ = std::fs::write(&storage_path, json);
            }
        }
        Ok(result)
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?
}

#[tauri::command]
async fn pick_game_executable() -> PlayCraftResult<Option<InspectedFileResult>> {
    let picked = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("PlayCraft — Select Game Executable, Script, or Shortcut (.exe, .bat, .lnk)")
            .add_filter("Game Executables & Shortcuts", &["exe", "bat", "cmd", "lnk"])
            .pick_file()
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?;

    let Some(path) = picked else {
        return Ok(None);
    };

    let inspected = inspect_executable_file(&path.to_string_lossy())?;
    Ok(Some(inspected))
}

#[tauri::command]
async fn pick_local_image_file() -> PlayCraftResult<Option<String>> {
    let picked = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("PlayCraft — Select Game Cover or Banner Image")
            .add_filter("Images (PNG, JPG, WEBP, AVIF)", &["png", "jpg", "jpeg", "webp", "avif"])
            .pick_file()
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?;

    let Some(path) = picked else {
        return Ok(None);
    };

    let bytes = std::fs::read(&path)
        .map_err(|e| PlayCraftError::ScannerError(format!("Failed to read image: {}", e)))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpeg")
        .to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => "image/jpeg",
    };

    let b64 = encode_base64(&bytes);
    Ok(Some(format!("data:{};base64,{}", mime, b64)))
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[tauri::command]
async fn pick_scan_directory() -> PlayCraftResult<Option<String>> {
    let picked = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("PlayCraft — Select Game Folder to Scan Recursively")
            .pick_folder()
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?;

    Ok(picked.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
fn inspect_game_file(file_path: String) -> PlayCraftResult<InspectedFileResult> {
    inspect_executable_file(&file_path)
}

#[tauri::command]
async fn search_online_game_db(query: String) -> PlayCraftResult<serde_json::Value> {
    let clean = query.trim().to_string();
    if clean.is_empty() {
        return Ok(serde_json::json!({ "items": [] }));
    }

    let res = tokio::task::spawn_blocking(move || -> PlayCraftResult<serde_json::Value> {
        let encoded: String = clean
            .bytes()
            .flat_map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    vec![b as char]
                }
                b' ' => vec!['+'],
                _ => format!("%{:02X}", b).chars().collect(),
            })
            .collect();

        let url = format!(
            "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
            encoded
        );

        let mut cmd = std::process::Command::new("curl.exe");
        cmd.args(["-s", "-L", "--max-time", "8", &url]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let output = cmd
            .output()
            .map_err(|e| PlayCraftError::ScannerError(format!("Online DB lookup failed: {}", e)))?;
        let body = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({ "items": [] }));
        Ok(parsed)
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))??;

    Ok(res)
}

#[tauri::command]
async fn fetch_steam_app_details(app_id: u32) -> PlayCraftResult<serde_json::Value> {
    let res = tokio::task::spawn_blocking(move || -> PlayCraftResult<serde_json::Value> {
        let url = format!(
            "https://store.steampowered.com/api/appdetails?appids={}&l=english",
            app_id
        );
        let mut cmd = std::process::Command::new("curl.exe");
        cmd.args(["-s", "-L", "--max-time", "8", &url]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let output = cmd
            .output()
            .map_err(|e| PlayCraftError::ScannerError(format!("App details lookup failed: {}", e)))?;
        let body = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({}));
        Ok(parsed)
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))??;

    Ok(res)
}

#[tauri::command]
fn toggle_favorite_game(
    game_id: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<Vec<GameEntry>> {
    let mut guard = state
        .games
        .lock()
        .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
    if let Some(g) = guard.iter_mut().find(|x| x.id == game_id) {
        g.is_favorite = !g.is_favorite;
    }
    let updated = guard.clone();
    drop(guard);
    state.persist_library();
    Ok(updated)
}

#[tauri::command]
fn remove_game_from_library(
    game_id: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<Vec<GameEntry>> {
    let mut guard = state
        .games
        .lock()
        .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
    guard.retain(|g| g.id != game_id);
    let updated = guard.clone();
    drop(guard);
    state.persist_library();
    Ok(updated)
}

#[tauri::command]
async fn scan_directory(dir_path: String, state: State<'_, AppState>) -> PlayCraftResult<Vec<GameEntry>> {
    let games_arc = state.games.clone();
    let storage_path = state.storage_path.clone();

    tokio::task::spawn_blocking(move || -> PlayCraftResult<Vec<GameEntry>> {
        let found = scan_directory_for_games(&dir_path, 4)?;
        let mut guard = games_arc
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;

        for item in &found {
            if !guard
                .iter()
                .any(|g| g.exe_path.eq_ignore_ascii_case(&item.exe_path))
            {
                guard.push(item.clone());
            }
        }
        let updated = guard.clone();
        drop(guard);
        if let Ok(json) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&storage_path, json);
        }
        Ok(updated)
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?
}

#[tauri::command]
async fn rescan_steam_library(state: State<'_, AppState>) -> PlayCraftResult<Vec<GameEntry>> {
    let games_arc = state.games.clone();
    let storage_path = state.storage_path.clone();

    tokio::task::spawn_blocking(move || -> PlayCraftResult<Vec<GameEntry>> {
        let store_found = scan_all_stores();
        let mut guard = games_arc
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;

        for item in store_found {
            if !guard
                .iter()
                .any(|g| g.id == item.id || g.exe_path.eq_ignore_ascii_case(&item.exe_path))
            {
                guard.push(item);
            }
        }
        let updated = guard.clone();
        drop(guard);
        if let Ok(json) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&storage_path, json);
        }
        Ok(updated)
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?
}

#[tauri::command]
fn launch_medal_app() -> PlayCraftResult<String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let medal_root = PathBuf::from(&local_app).join("Medal");
            let direct_exe = medal_root.join("Medal.exe");
            if direct_exe.exists() {
                let _ = Command::new(&direct_exe)
                    .creation_flags(0x08000000)
                    .spawn();
                return Ok(format!("Launched Medal ({})", direct_exe.display()));
            }
            if let Ok(rd) = std::fs::read_dir(&medal_root) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.is_dir()
                        && p.file_name()
                            .map(|n| n.to_string_lossy().starts_with("app-"))
                            .unwrap_or(false)
                    {
                        let app_exe = p.join("Medal.exe");
                        if app_exe.exists() {
                            let _ = Command::new(&app_exe)
                                .creation_flags(0x08000000)
                                .spawn();
                            return Ok(format!("Launched Medal ({})", app_exe.display()));
                        }
                    }
                }
            }
        }

        // Fallback to medal:// URI protocol handler
        let _ = Command::new("cmd.exe")
            .args(["/C", "start", "", "medal://"])
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| PlayCraftError::LaunchFailed(format!("Could not open Medal: {}", e)))?;
    }
    Ok("Triggered Medal.tv desktop application".to_string())
}

#[tauri::command]
fn open_path_in_explorer(target_path: String) -> PlayCraftResult<()> {
    let trimmed = target_path.trim();
    if trimmed.is_empty() {
        return Err(PlayCraftError::ScannerError("Path is empty".to_string()));
    }
    let p = PathBuf::from(trimmed);
    let open_target = if p.is_file() {
        p.parent().map(|parent| parent.to_path_buf()).unwrap_or(p)
    } else {
        p
    };
    if !open_target.exists() {
        return Err(PlayCraftError::ScannerError(format!(
            "Folder does not exist on disk: {}",
            open_target.display()
        )));
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(open_target.to_string_lossy().to_string())
            .spawn();
    }
    Ok(())
}

#[tauri::command]
fn open_windows_sound_settings() -> PlayCraftResult<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd.exe")
            .args(["/C", "start", "", "ms-settings:apps-volume"])
            .creation_flags(0x08000000)
            .spawn();
    }
    Ok(())
}

#[tauri::command]
async fn get_local_media_gallery() -> PlayCraftResult<Vec<LocalMediaItem>> {
    tokio::task::spawn_blocking(move || -> PlayCraftResult<Vec<LocalMediaItem>> {
        let mut items = Vec::new();
        let mut candidate_dirs: Vec<(PathBuf, &str)> = Vec::new();

        if let Ok(profile) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(&profile);
            candidate_dirs.push((p.join("Videos").join("Medal"), "Medal.tv Clips"));
            candidate_dirs.push((p.join("Videos").join("Captures"), "Windows Game Captures"));
            candidate_dirs.push((
                p.join("Pictures").join("Screenshots"),
                "Windows Screenshots",
            ));
            candidate_dirs.push((p.join("Videos"), "User Videos"));
        }
        for drive in ["C:\\Medal\\Clips", "D:\\Medal\\Clips", "E:\\Medal\\Clips"] {
            let p = PathBuf::from(drive);
            if p.exists() {
                candidate_dirs.push((p, "Medal.tv Clips"));
            }
        }

        let mut seen = std::collections::HashSet::new();
        for (dir, label) in candidate_dirs {
            if !dir.exists() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&dir)
                .max_depth(3)
                .into_iter()
                .flatten()
                .take(150)
            {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                    continue;
                };
                let ext_l = ext.to_lowercase();
                let media_type = match ext_l.as_str() {
                    "mp4" | "mkv" | "webm" | "mov" => "VIDEO CLIP",
                    "png" | "jpg" | "jpeg" | "webp" => "SCREENSHOT",
                    _ => continue,
                };

                let full_str = path.to_string_lossy().to_string();
                if !seen.insert(full_str.to_lowercase()) {
                    continue;
                }

                let meta = entry.metadata().ok();
                let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let size_mb =
                    ((size_bytes as f64 / (1024.0 * 1024.0)) * 100.0).round() as f32 / 100.0;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Capture".to_string());

                items.push(LocalMediaItem {
                    name,
                    file_path: full_str,
                    media_type: media_type.to_string(),
                    source_app: label.to_string(),
                    size_mb,
                    modified_date: label.to_string(),
                });

                if items.len() >= 36 {
                    break;
                }
            }
        }

        Ok(items)
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?
}

#[tauri::command]
fn get_available_display_modes() -> PlayCraftResult<Vec<DisplayModeSpec>> {
    let mut modes = Vec::new();
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW};
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        let mut idx = 0u32;
        let mut seen = std::collections::HashSet::new();

        while EnumDisplaySettingsW(std::ptr::null(), idx, &mut dm) != 0 {
            idx += 1;
            let w = dm.dmPelsWidth;
            let h = dm.dmPelsHeight;
            let hz = dm.dmDisplayFrequency;
            if w >= 1280 && h >= 720 && hz >= 59 {
                let key = (w, h, hz);
                if seen.insert(key) {
                    modes.push(DisplayModeSpec {
                        width: w,
                        height: h,
                        refresh_hz: hz,
                        label: format!("{}x{} @ {}Hz", w, h, hz),
                    });
                }
            }
        }
    }
    modes.sort_by(|a, b| {
        b.width
            .cmp(&a.width)
            .then_with(|| b.height.cmp(&a.height))
            .then_with(|| b.refresh_hz.cmp(&a.refresh_hz))
    });
    Ok(modes)
}

pub fn apply_custom_display_mode(resolution: Option<&str>, refresh_hz: Option<u32>) -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Graphics::Gdi::{
            ChangeDisplaySettingsW, EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS,
        };
        if resolution.is_none() && refresh_hz.is_none() {
            return false;
        }
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut dm) == 0 {
            return false;
        }
        if let Some(res_str) = resolution {
            let parts: Vec<&str> = res_str.split('x').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].trim().parse::<u32>(), parts[1].trim().parse::<u32>()) {
                    dm.dmPelsWidth = w;
                    dm.dmPelsHeight = h;
                }
            }
        }
        if let Some(hz) = refresh_hz {
            if hz >= 50 {
                dm.dmDisplayFrequency = hz;
            }
        }
        ChangeDisplaySettingsW(&dm, 0) == 0
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn restore_desktop_display_mode() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Graphics::Gdi::ChangeDisplaySettingsW;
        let _ = ChangeDisplaySettingsW(std::ptr::null(), 0);
    }
}

fn copy_dir_recursive(src: & std::path::Path, dst: &std::path::Path) -> anyhow::Result<(usize, u64)> {
    std::fs::create_dir_all(dst)?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            let (f, b) = copy_dir_recursive(&entry.path(), &dest_path)?;
            files += f;
            bytes += b;
        } else if ty.is_file() {
            let len = std::fs::copy(entry.path(), &dest_path)?;
            files += 1;
            bytes += len;
        }
    }
    Ok((files, bytes))
}

#[tauri::command]
fn create_save_backup(
    game_id: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<SaveBackupEntry> {
    let (title, save_dir) = {
        let guard = state
            .games
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
        let Some(g) = guard.iter().find(|x| x.id == game_id) else {
            return Err(PlayCraftError::GameNotFound(game_id));
        };
        (g.title.clone(), g.save_folder_path.clone())
    };

    if save_dir.trim().is_empty() || !std::path::Path::new(&save_dir).exists() {
        return Err(PlayCraftError::ScannerError(
            "Save folder is not set or does not exist on disk. Pick a Save Folder first.".to_string(),
        ));
    }

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let snap_id = format!("snap-{}", now_secs);
    let backups_root = resolve_playcraft_data_dir()
        .join("playcraft_backups")
        .join(&game_id)
        .join(&snap_id);

    let (file_count, total_bytes) =
        copy_dir_recursive(std::path::Path::new(&save_dir), &backups_root)
            .map_err(|e| PlayCraftError::ScannerError(format!("Backup copy failed: {}", e)))?;

    let size_mb = ((total_bytes as f64 / (1024.0 * 1024.0)) * 100.0).round() as f32 / 100.0;
    let entry = SaveBackupEntry {
        id: snap_id.clone(),
        game_id,
        game_title: title,
        timestamp: format!("Snapshot #{}", now_secs % 100000),
        backup_path: backups_root.to_string_lossy().to_string(),
        file_count,
        size_mb,
    };

    if let Ok(meta_json) = serde_json::to_string_pretty(&entry) {
        let _ = std::fs::write(backups_root.join("playcraft_snapshot.json"), meta_json);
    }

    Ok(entry)
}

#[tauri::command]
fn list_save_backups(game_id: String) -> PlayCraftResult<Vec<SaveBackupEntry>> {
    let mut list = Vec::new();
    let game_backups_dir = resolve_playcraft_data_dir()
        .join("playcraft_backups")
        .join(&game_id);
    if !game_backups_dir.exists() {
        return Ok(list);
    }
    if let Ok(rd) = std::fs::read_dir(&game_backups_dir) {
        for item in rd.flatten() {
            let meta_file = item.path().join("playcraft_snapshot.json");
            if let Ok(txt) = std::fs::read_to_string(&meta_file) {
                if let Ok(parsed) = serde_json::from_str::<SaveBackupEntry>(&txt) {
                    list.push(parsed);
                }
            }
        }
    }
    list.reverse();
    Ok(list)
}

#[tauri::command]
fn restore_save_backup(
    game_id: String,
    backup_id: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<String> {
    let save_dir = {
        let guard = state
            .games
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
        let Some(g) = guard.iter().find(|x| x.id == game_id) else {
            return Err(PlayCraftError::GameNotFound(game_id));
        };
        g.save_folder_path.clone()
    };

    if save_dir.trim().is_empty() {
        return Err(PlayCraftError::ScannerError(
            "Target Save Folder is not configured for this game.".to_string(),
        ));
    }

    let snap_dir = resolve_playcraft_data_dir()
        .join("playcraft_backups")
        .join(&game_id)
        .join(&backup_id);

    if !snap_dir.exists() {
        return Err(PlayCraftError::ScannerError(
            "Snapshot folder not found.".to_string(),
        ));
    }

    let (files, _) = copy_dir_recursive(&snap_dir, std::path::Path::new(&save_dir))
        .map_err(|e| PlayCraftError::ScannerError(format!("Restore failed: {}", e)))?;

    Ok(format!(
        "Restored {} files into {}",
        files.saturating_sub(1),
        save_dir
    ))
}

#[tauri::command]
fn update_game_workspace_config(
    update: GameWorkspaceConfigUpdate,
    state: State<'_, AppState>,
) -> PlayCraftResult<GameEntry> {
    let mut guard = state
        .games
        .lock()
        .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
    let Some(game) = guard.iter_mut().find(|g| g.id == update.game_id) else {
        return Err(PlayCraftError::GameNotFound(update.game_id));
    };

    game.notes = update.notes;
    game.save_folder_path = update.save_folder_path;
    game.auto_backup_saves = update.auto_backup_saves;
    game.mod_folder_path = update.mod_folder_path;
    game.active_launch_profile = update.active_launch_profile;
    game.target_refresh_hz = update.target_refresh_hz;
    game.target_resolution = update.target_resolution;
    if let Some(cover) = update.cover_url.filter(|s| !s.trim().is_empty()) {
        game.cover_url = cover.clone();
        if let Some(first_act) = game.activities.first_mut() {
            first_act.image_url = cover;
        }
    }
    if let Some(hero) = update.hero_url.filter(|s| !s.trim().is_empty()) {
        game.hero_url = hero;
    }
    if let Some(fav) = update.is_favorite {
        game.is_favorite = fav;
    }

    let cloned = game.clone();
    drop(guard);
    state.persist_library();
    Ok(cloned)
}

#[tauri::command]
fn get_session_history() -> PlayCraftResult<Vec<GameSessionEntry>> {
    let sessions_path = resolve_playcraft_data_dir().join("playcraft_sessions.json");
    if !sessions_path.exists() {
        return Ok(Vec::new());
    }
    let list: Vec<GameSessionEntry> = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(list)
}

#[tauri::command]
fn add_custom_game(
    req: NewGameRequest,
    state: State<'_, AppState>,
) -> PlayCraftResult<Vec<GameEntry>> {
    let inspected = inspect_executable_file(&req.exe_path).ok();
    let path_obj = PathBuf::from(&req.exe_path);
    let ext = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("exe")
        .to_lowercase();

    let source = match ext.as_str() {
        "bat" | "cmd" => GameSource::BatchScript,
        "lnk" => GameSource::Shortcut,
        _ => GameSource::Executable,
    };

    let working_dir = inspected
        .as_ref()
        .map(|i| i.working_dir.clone())
        .or_else(|| path_obj.parent().map(|p| p.to_string_lossy().to_string()))
        .unwrap_or_else(|| ".".to_string());

    let real_size_gb = inspected.as_ref().map(|i| i.size_on_disk_gb).unwrap_or(0.1);

    let cover = req
        .cover_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "assets/playcraft-logo.jpg".to_string());
    let hero = req
        .hero_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| cover.clone());

    let clean_title = clean_trademark_text(&req.title);
    let description = clean_trademark_text(
        &req.description
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Installed at {}. Ready to launch via PlayCraft.", req.exe_path)),
    );

    let new_entry = GameEntry {
        id: format!("game-{}", rand::thread_rng().gen_range(10000..99999)),
        title: clean_title.clone(),
        subtitle: req
            .subtitle
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| working_dir.clone()),
        description,
        genre: req
            .genre
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "PC Game".to_string()),
        source,
        exe_path: req.exe_path,
        working_dir: working_dir.clone(),
        launch_args: req.launch_args.unwrap_or_default(),
        steam_app_id: req.steam_app_id,
        cover_url: cover.clone(),
        hero_url: hero,
        accent_color: "#00f0ff".to_string(),
        playtime_hours: 0.0,
        last_played: "Never Played".to_string(),
        achievements_unlocked: 0,
        achievements_total: 0,
        size_on_disk_gb: real_size_gb,
        deep_optimize_before_launch: req.deep_optimize_before_launch,
        drm_bypass: req.drm_bypass.unwrap_or_default(),
        activities: vec![ActivityCard {
            title: format!("{} — Local Installation", clean_title),
            subtitle: format!("Total Game Folder Size: {:.2} GB • {}", real_size_gb, working_dir),
            tag: "VERIFIED TARGET".to_string(),
            image_url: cover,
            progress_percent: None,
        }],
        notes: String::new(),
        save_folder_path: detect_likely_save_folder(&clean_title),
        auto_backup_saves: false,
        mod_folder_path: String::new(),
        active_launch_profile: "Default".to_string(),
        target_refresh_hz: None,
        target_resolution: None,
        is_favorite: false,
    };

    let mut guard = state
        .games
        .lock()
        .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
    guard.insert(0, new_entry);
    let updated = guard.clone();
    drop(guard);
    state.persist_library();
    Ok(updated)
}

#[tauri::command]
fn update_game_drm_config(
    game_id: String,
    drm_bypass: DrmBypassConfig,
    deep_optimize: bool,
    launch_args: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<GameEntry> {
    let mut guard = state
        .games
        .lock()
        .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;

    let Some(game) = guard.iter_mut().find(|g| g.id == game_id) else {
        return Err(PlayCraftError::GameNotFound(game_id));
    };

    game.drm_bypass = drm_bypass;
    game.deep_optimize_before_launch = deep_optimize;
    game.launch_args = launch_args;
    let updated = game.clone();
    drop(guard);
    state.persist_library();
    Ok(updated)
}

#[tauri::command]
async fn run_deep_optimizations(state: State<'_, AppState>) -> PlayCraftResult<OptimizerReport> {
    let cfg = state
        .optimizer_config
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default();
    let restore_arc = state.restorable_processes.clone();

    tokio::task::spawn_blocking(move || -> PlayCraftResult<OptimizerReport> {
        let mut restore_guard = restore_arc
            .lock()
            .map_err(|_| PlayCraftError::OptimizerError("Restore mutex poisoned".to_string()))?;
        run_deep_optimization(&cfg, &mut restore_guard)
    })
    .await
    .map_err(|e| PlayCraftError::OptimizerError(e.to_string()))?
}

#[tauri::command]
fn restore_background_processes(state: State<'_, AppState>) -> PlayCraftResult<Vec<String>> {
    let mut restore_guard = state
        .restorable_processes
        .lock()
        .map_err(|_| PlayCraftError::OptimizerError("Restore mutex poisoned".to_string()))?;
    Ok(restore_terminated_processes(&mut restore_guard))
}

#[tauri::command]
fn get_optimizer_config(state: State<'_, AppState>) -> PlayCraftResult<OptimizerConfig> {
    let cfg = state
        .optimizer_config
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default();
    Ok(cfg)
}

#[tauri::command]
fn save_optimizer_config(
    config: OptimizerConfig,
    state: State<'_, AppState>,
) -> PlayCraftResult<OptimizerConfig> {
    if let Ok(mut guard) = state.optimizer_config.lock() {
        *guard = config.clone();
    }
    Ok(config)
}

#[derive(Clone)]
pub struct SafeLogWriter;

impl std::io::Write for SafeLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stdout().write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stdout().flush();
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SafeLogWriter {
    type Writer = SafeLogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tauri::command]
fn launch_game(
    app_handle: AppHandle,
    game_id: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<serde_json::Value> {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        launch_game_internal(&app_handle, &game_id, &state)
    }));
    match res {
        Ok(inner_res) => inner_res,
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown internal panic".to_string()
            };
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("playcraft_crash.log")
                .and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "Panic trapped in launch_game: {}", msg)
                });
            Err(PlayCraftError::LaunchFailed(format!("Game launched in system, PlayCraft remained active: {}", msg)))
        }
    }
}

fn launch_game_internal(
    app_handle: &AppHandle,
    game_id: &str,
    state: &State<'_, AppState>,
) -> PlayCraftResult<serde_json::Value> {
    let game = {
        let mut guard = state
            .games
            .lock()
            .map_err(|_| PlayCraftError::ScannerError("Library mutex poisoned".to_string()))?;
        let Some(g) = guard.iter_mut().find(|item| item.id == game_id) else {
            return Err(PlayCraftError::GameNotFound(game_id.to_string()));
        };
        g.last_played = "Playing Now".to_string();
        g.clone()
    };
    state.persist_library();

    let opt_report = if game.deep_optimize_before_launch {
        let cfg = state
            .optimizer_config
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default();
        let mut restore_guard = state
            .restorable_processes
            .lock()
            .map_err(|_| PlayCraftError::OptimizerError("Restore mutex poisoned".to_string()))?;
        Some(run_deep_optimization(&cfg, &mut restore_guard)?)
    } else {
        None
    };

    // Automatic Save-Game Snapshot before launch if enabled
    if game.auto_backup_saves && !game.save_folder_path.trim().is_empty() {
        let save_p = std::path::Path::new(&game.save_folder_path);
        if save_p.exists() {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let snap_id = format!("auto-{}", now_secs);
            let backups_root = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("playcraft_backups")
                .join(&game.id)
                .join(&snap_id);
            if let Ok((fc, bytes)) = copy_dir_recursive(save_p, &backups_root) {
                let entry = SaveBackupEntry {
                    id: snap_id,
                    game_id: game.id.clone(),
                    game_title: game.title.clone(),
                    timestamp: format!("Pre-Launch Snapshot #{}", now_secs % 100000),
                    backup_path: backups_root.to_string_lossy().to_string(),
                    file_count: fc,
                    size_mb: ((bytes as f64 / (1024.0 * 1024.0)) * 100.0).round() as f32 / 100.0,
                };
                if let Ok(json) = serde_json::to_string_pretty(&entry) {
                    let _ = std::fs::write(backups_root.join("playcraft_snapshot.json"), json);
                }
            }
        }
    }

    // Apply per-game Monitor Refresh Rate (Hz) & Resolution if configured
    let display_changed = apply_custom_display_mode(
        game.target_resolution.as_deref(),
        game.target_refresh_hz,
    );

    let pid = spawn_game_process(&game)?;
    let session_start = std::time::Instant::now();

    // Unconditionally keep PlayCraft open and running simultaneously behind the launched game!
    // The mobile gamepad WebSocket server, hotkeys, and system services must stay 100% active.
    // We intentionally NEVER touch, hide, or minimize the window on game launch.
    info!("Game launched: PlayCraft launcher remains active and running simultaneously in background.");

    if let Ok(mut active) = state.active_game.lock() {
        *active = Some((game.title.clone(), pid));
    }

    let active_arc = state.active_game.clone();
    let restore_arc = state.restorable_processes.clone();
    let games_arc = state.games.clone();
    let storage_path = state.storage_path.clone();
    let game_id_clone = game.id.clone();
    let title_clone = game.title.clone();
    let source_label = format!("{:?}", game.source);
    let app_clone = app_handle.clone();
    let working_dir_lower = game.working_dir.to_lowercase();
    let exe_name_lower = std::path::Path::new(&game.exe_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    tauri::async_runtime::spawn(async move {
        // Monitor game session while initial PID or child processes in game folder are active
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let mut sys = System::new();
            sys.refresh_processes();

            if sys.process(Pid::from_u32(pid)).is_some() {
                continue;
            }

            // If initial PID exited (common for multi-process launchers, stubs & repacks),
            // check if any process belonging to the game or its folder is still running.
            let mut game_still_running = false;
            if !exe_name_lower.is_empty() || !working_dir_lower.is_empty() {
                for proc in sys.processes().values() {
                    let proc_name = proc.name().to_string().to_lowercase();
                    if !exe_name_lower.is_empty() && (proc_name == exe_name_lower || proc_name.contains(&exe_name_lower)) {
                        game_still_running = true;
                        break;
                    }
                    if !working_dir_lower.is_empty() {
                        if let Some(exe_p) = proc.exe() {
                            let p_str = exe_p.to_string_lossy().to_lowercase();
                            if p_str.starts_with(&working_dir_lower) {
                                game_still_running = true;
                                break;
                            }
                        }
                    }
                }
            }

            if !game_still_running {
                break;
            }
        }

        if display_changed {
            restore_desktop_display_mode();
        }

        // Record 100% REAL elapsed session playtime & append to playcraft_sessions.json
        let elapsed_secs = session_start.elapsed().as_secs_f32();
        let elapsed_hours = elapsed_secs / 3600.0;
        let elapsed_mins = ((elapsed_secs / 60.0) * 10.0).round() / 10.0;

        if let Ok(mut guard) = games_arc.lock() {
            if let Some(g) = guard.iter_mut().find(|item| item.id == game_id_clone) {
                g.playtime_hours = ((g.playtime_hours + elapsed_hours) * 100.0).round() / 100.0;
                g.last_played = "Played Today".to_string();
            }
            if let Ok(json) = serde_json::to_string_pretty(&*guard) {
                let _ = std::fs::write(&storage_path, json);
            }
        }

        let sessions_path = resolve_playcraft_data_dir().join("playcraft_sessions.json");
        let mut sessions: Vec<GameSessionEntry> = std::fs::read_to_string(&sessions_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let epoch_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        sessions.insert(
            0,
            GameSessionEntry {
                id: format!("sess-{}", epoch_now),
                game_id: game_id_clone.clone(),
                game_title: title_clone.clone(),
                started_at: format!("Session #{}", epoch_now % 100000),
                duration_minutes: elapsed_mins.max(0.1),
                store_source: source_label,
            },
        );
        if let Ok(s_json) = serde_json::to_string_pretty(&sessions) {
            let _ = std::fs::write(&sessions_path, s_json);
        }

        if let Ok(mut active) = active_arc.lock() {
            *active = None;
        }

        let restored = if let Ok(mut queue) = restore_arc.lock() {
            restore_terminated_processes(&mut queue)
        } else {
            Vec::new()
        };

        info!(
            "Game '{}' (PID {}) exited after {:.2} hrs. Auto-restored {} processes.",
            title_clone,
            pid,
            elapsed_hours,
            restored.len()
        );

        let _ = app_clone.emit(
            "game-session-ended",
            serde_json::json!({
                "title": title_clone,
                "pid": pid,
                "elapsed_hours": elapsed_hours,
                "restored_processes": restored
            }),
        );
    });

    Ok(serde_json::json!({
        "status": "launched",
        "title": game.title,
        "pid": pid,
        "optimization_report": opt_report,
        "author": AUTHOR_DISPLAY
    }))
}

#[tauri::command]
async fn get_mobile_pairing_info(state: State<'_, AppState>) -> PlayCraftResult<MobilePairingInfo> {
    let mobile_arc = state.mobile_server.clone();
    tokio::task::spawn_blocking(move || mobile_arc.build_pairing_info())
        .await
        .map_err(|e| PlayCraftError::ScannerError(e.to_string()))
}

#[tauri::command]
fn regenerate_mobile_pin(state: State<'_, AppState>) -> PlayCraftResult<MobilePairingInfo> {
    Ok(state.mobile_server.regenerate_pin())
}

#[tauri::command]
fn simulate_mobile_input(
    app_handle: AppHandle,
    control: String,
    state: State<'_, AppState>,
) -> PlayCraftResult<()> {
    state
        .mobile_server
        .total_inputs
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if control.eq_ignore_ascii_case("PS_HOME") {
        let _ = app_handle.emit("playcraft-overlay-toggle", "simulated-ps-button");
        return Ok(());
    }

    send_virtual_controller_button(&control, true);
    send_virtual_controller_button(&control, false);

    let _ = app_handle.emit(
        "mobile-controller-input",
        serde_json::json!({
            "control": control.to_uppercase(),
            "state": "down",
            "source": "PlayCraft Virtual Gamepad Tester"
        }),
    );
    Ok(())
}

#[tauri::command]
async fn get_system_telemetry(state: State<'_, AppState>) -> PlayCraftResult<SystemTelemetry> {
    let active_game_pair = state
        .active_game
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .map(|(t, p)| (Some(t), Some(p)))
        .unwrap_or((None, None));
    let overlay_active = state.overlay_active.lock().map(|b| *b).unwrap_or(false);
    let mobile_clients_connected = state
        .mobile_server
        .clients
        .lock()
        .map(|c| c.len())
        .unwrap_or(0);

    tokio::task::spawn_blocking(move || -> PlayCraftResult<SystemTelemetry> {
        let mut sys = System::new();
        sys.refresh_cpu();
        sys.refresh_memory();

        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let ram_used_mb = sys.used_memory() / (1024 * 1024);
        let ram_total_mb = (sys.total_memory() / (1024 * 1024)).max(1);
        let ram_usage_percent =
            ((ram_used_mb as f32 / ram_total_mb as f32) * 1000.0).round() / 10.0;

        Ok(SystemTelemetry {
            cpu_usage_percent: (cpu_usage * 10.0).round() / 10.0,
            ram_used_mb,
            ram_total_mb,
            ram_usage_percent,
            active_game_title: active_game_pair.0,
            active_game_pid: active_game_pair.1,
            overlay_active,
            xinput_controllers_connected: get_connected_xinput_count(),
            vigem_bus_detected: is_vigem_bus_available(),
            mobile_clients_connected,
        })
    })
    .await
    .map_err(|e| PlayCraftError::ScannerError(e.to_string()))?
}

#[tauri::command]
fn toggle_global_overlay(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> PlayCraftResult<bool> {
    let mut guard = state
        .overlay_active
        .lock()
        .map_err(|_| PlayCraftError::OptimizerError("Overlay mutex poisoned".to_string()))?;
    *guard = !*guard;
    let is_open = *guard;

    if let Some(win) = app_handle.get_webview_window("main") {
        if is_open {
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_always_on_top(true);
            let _ = win.set_focus();
        } else {
            let _ = win.set_always_on_top(false);
        }
    }

    Ok(is_open)
}

fn setup_system_tray(app: &tauri::App) -> anyhow::Result<()> {
    let show_item = MenuItem::with_id(
        app,
        "tray_show",
        "Open PlayCraft (Fullscreen)",
        true,
        None::<&str>,
    )?;
    let toggle_fs_item = MenuItem::with_id(
        app,
        "tray_fullscreen",
        "Toggle Fullscreen / Windowed",
        true,
        None::<&str>,
    )?;
    let optimize_item = MenuItem::with_id(
        app,
        "tray_optimize",
        "Purge Standby RAM Now",
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, "tray_quit", "Quit PlayCraft", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&show_item, &toggle_fs_item, &optimize_item, &quit_item],
    )?;

    let mut tray_builder = TrayIconBuilder::new()
        .tooltip(APP_TITLE)
        .menu(&menu)
        .on_menu_event(|app_handle, event| match event.id.as_ref() {
            "tray_show" => {
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_fullscreen(true);
                    let _ = win.set_focus();
                }
            }
            "tray_fullscreen" => {
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.show();
                    let is_fs = win.is_fullscreen().unwrap_or(false);
                    let _ = win.set_fullscreen(!is_fs);
                    let _ = win.set_focus();
                }
            }
            "tray_optimize" => {
                let state = app_handle.state::<AppState>();
                let cfg = state
                    .optimizer_config
                    .lock()
                    .map(|c| c.clone())
                    .unwrap_or_default();
                if let Ok(mut q) = state.restorable_processes.lock() {
                    let _ = run_deep_optimization(&cfg, &mut q);
                };
            }
            "tray_quit" => {
                let state = app_handle.state::<AppState>();
                clean_shutdown_app(app_handle, &state);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app_handle = tray.app_handle();
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let _tray = tray_builder.build(app)?;
    Ok(())
}

#[cfg(target_os = "windows")]
struct SingleInstanceGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn acquire_single_instance_or_focus() -> Option<SingleInstanceGuard> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let mutex_name: Vec<u16> = "Local\\PlayCraft_By_Rashmina_Fernando_SingleInstance"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if !handle.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
            let title_w: Vec<u16> = APP_TITLE
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let existing = FindWindowW(std::ptr::null(), title_w.as_ptr());
            if !existing.is_null() {
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
            }
            CloseHandle(handle);
            return None;
        }
        Some(SingleInstanceGuard(handle))
    }
}

pub fn run() {
    #[cfg(target_os = "windows")]
    let _single_instance_guard = match acquire_single_instance_or_focus() {
        Some(guard) => guard,
        None => return,
    };

    // Install panic hook so any unexpected panic is safely written to a log file instead of silent exit
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = format!("=== PlayCraft Panic at {:?} ===\n{}\n", std::time::SystemTime::now(), panic_info);
        let crash_path = resolve_playcraft_data_dir().join("playcraft_crash.log");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crash_path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(msg.as_bytes())
            });
    }));

    tracing_subscriber::fmt()
        .with_writer(SafeLogWriter)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Initializing {} ...", APP_TITLE);

    let cwd = resolve_playcraft_data_dir();
    let storage_path = cwd.join("playcraft_library.json");
    let settings_path = cwd.join("playcraft_settings.json");

    let initial_settings: LauncherSettings = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        LauncherSettings::default()
    };

    // Load persisted library, strip any legacy fake/showcase entries, and recalculate full root folder size
    let initial_games: Vec<GameEntry> = if storage_path.exists() {
        let loaded: Vec<GameEntry> = std::fs::read_to_string(&storage_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| build_initial_library(&cwd));
        let upgraded: Vec<GameEntry> = loaded
            .into_iter()
            .filter(|g| !g.id.starts_with("showcase-") && g.id != "playcraft-explore")
            .map(|mut g| {
                g.title = clean_trademark_text(&g.title);
                g.description = clean_trademark_text(&g.description);
                let exe_p = std::path::Path::new(&g.exe_path);
                if exe_p.exists() {
                    let root_dir = resolve_game_root_folder(exe_p);
                    let full_gb = calculate_directory_size_gb(&root_dir, Some(exe_p));
                    if full_gb > 0.0 {
                        g.size_on_disk_gb = full_gb;
                    }
                    g.working_dir = root_dir.to_string_lossy().to_string();
                    if !g.subtitle.contains("•") {
                        g.subtitle = g.working_dir.clone();
                    }
                }
                for act in &mut g.activities {
                    act.title = clean_trademark_text(&act.title);
                    act.subtitle = format!(
                        "Total Game Folder Size: {:.2} GB • {}",
                        g.size_on_disk_gb, g.working_dir
                    );
                }
                g
            })
            .collect();
        if let Ok(json) = serde_json::to_string_pretty(&upgraded) {
            let _ = std::fs::write(&storage_path, json);
        }
        upgraded
    } else {
        let scanned = build_initial_library(&cwd);
        if let Ok(json) = serde_json::to_string_pretty(&scanned) {
            let _ = std::fs::write(&storage_path, json);
        }
        scanned
    };

    let lan_ip = detect_lan_ip();
    let pin = format!("{:04}", rand::thread_rng().gen_range(1000..=9999));
    let mobile_state = Arc::new(MobileServerState::new(lan_ip, 19550, pin));

    let app_state = AppState {
        games: Arc::new(Mutex::new(initial_games)),
        settings: Mutex::new(initial_settings.clone()),
        optimizer_config: Mutex::new(OptimizerConfig::default()),
        restorable_processes: Arc::new(Mutex::new(Vec::new())),
        active_game: Arc::new(Mutex::new(None)),
        overlay_active: Mutex::new(false),
        mobile_server: mobile_state.clone(),
        storage_path,
        settings_path,
    };

    tauri::Builder::default()
        .manage(app_state)
        .setup(move |app| {
            // Initialize native Windows System Tray Icon & Context Menu
            if let Err(e) = setup_system_tray(app) {
                tracing::warn!("System tray initialization warning: {}", e);
            }

            // Always start on the configured target monitor (Primary Monitor by default) and window mode
            let effective_mode = if initial_settings.window_mode.trim().is_empty() {
                if initial_settings.start_fullscreen {
                    "fullscreen"
                } else {
                    "windowed"
                }
            } else {
                initial_settings.window_mode.as_str()
            };
            apply_window_mode_and_monitor(
                app.handle(),
                effective_mode,
                &initial_settings.target_monitor,
            );

            let handle = app.handle().clone();
            let mobile_clone = mobile_state.clone();

            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_mobile_server(handle, mobile_clone).await {
                    tracing::error!("Mobile controller server failed: {}", e);
                }
            });

            #[cfg(target_os = "windows")]
            {
                let xinput_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = init_virtual_x360_controller();
                    spawn_xinput_poller(xinput_handle);
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if !FORCE_EXIT_APPROVED.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                    let _ = window.emit(
                        "playcraft-request-exit",
                        serde_json::json!({ "source": "window_close_or_alt_f4" }),
                    );
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_author_metadata,
            get_available_wallpapers,
            get_launcher_settings,
            save_launcher_settings,
            get_available_monitors,
            toggle_fullscreen_window,
            check_for_app_update,
            open_external_url,
            quit_application,
            minimize_launcher_to_tray,
            setup_usb_mobile_bridge,
            get_full_system_specs,
            get_game_library,
            pick_game_executable,
            pick_local_image_file,
            pick_scan_directory,
            inspect_game_file,
            search_online_game_db,
            fetch_steam_app_details,
            toggle_favorite_game,
            remove_game_from_library,
            scan_directory,
            rescan_steam_library,
            launch_medal_app,
            open_path_in_explorer,
            open_windows_sound_settings,
            get_local_media_gallery,
            get_available_display_modes,
            create_save_backup,
            list_save_backups,
            restore_save_backup,
            update_game_workspace_config,
            get_session_history,
            add_custom_game,
            update_game_drm_config,
            run_deep_optimizations,
            restore_background_processes,
            get_optimizer_config,
            save_optimizer_config,
            launch_game,
            get_mobile_pairing_info,
            regenerate_mobile_pin,
            simulate_mobile_input,
            get_system_telemetry,
            toggle_global_overlay,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PlayCraft by Rashmina Fernando (www.rashmina.dev)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile_server::generate_qr_svg;
    use crate::models::{AUTHOR_DISPLAY, AUTHOR_URL};

    #[test]
    fn test_authorship_metadata_hardcoded() {
        let meta = AuthorMetadata::default();
        assert!(meta.app_name.contains("Rashmina Fernando"));
        assert_eq!(meta.website, AUTHOR_URL);
        assert!(meta.copyright.contains(AUTHOR_DISPLAY));

        let settings = LauncherSettings::default();
        assert_eq!(settings.window_mode, "fullscreen");
        assert_eq!(settings.target_monitor, "primary");
        assert_eq!(settings.close_button_action, "exit_app");
        assert_eq!(settings.last_update_check_ts, 0);
    }

    #[test]
    fn test_semver_update_comparison() {
        assert!(is_remote_version_newer("1.0.0", "v1.0.1"));
        assert!(is_remote_version_newer("1.0.0", "v1.1.0"));
        assert!(is_remote_version_newer("1.9.9", "2.0.0"));
        assert!(!is_remote_version_newer("1.0.0", "v1.0.0"));
        assert!(!is_remote_version_newer("1.2.0", "v1.1.9"));
        assert!(is_remote_version_newer("1.0.0-beta.1", "v1.0.0"));
    }

    #[test]
    fn test_real_system_specs_collection() {
        let specs = collect_full_system_specs_sync().expect("system specs should succeed");
        assert!(specs.ram_total_gb > 0.0);
        assert!(specs.logical_threads > 0);
        assert!(!specs.cpu_brand.is_empty());
        assert!(!specs.gpus.is_empty());
    }

    #[test]
    fn test_qr_svg_generation_and_pairing_pin() {
        let state = MobileServerState::new("192.168.1.50".to_string(), 19550, "4829".to_string());
        let info = state.build_pairing_info();
        assert_eq!(info.pin_code, "4829");
        assert!(info.pairing_url.starts_with("playcraft://pair?"));
        assert!(info.pairing_url.contains("pin=4829"));
        assert!(info.pairing_url.contains("192.168.1.50"));
        assert!(info.qr_svg.contains("<svg"));
        let updated = state.regenerate_pin();
        assert_eq!(updated.pin_code.len(), 4);
        assert!(generate_qr_svg("http://127.0.0.1:19550").starts_with("<svg"));
    }

    #[test]
    fn test_universal_scanner_and_silent_batch_launch() {
        let temp_dir = std::env::temp_dir().join("playcraft_test_scan");
        let _ = std::fs::create_dir_all(&temp_dir);
        let bat = scanner::ensure_demo_batch_script(&temp_dir);
        assert!(bat.exists());

        let games = scanner::scan_directory_for_games(&temp_dir.to_string_lossy(), 3)
            .expect("scan should succeed");
        assert!(!games.is_empty());
        assert_eq!(games[0].playtime_hours, 0.0);

        let pid = scanner::spawn_game_process(&games[0]).expect("silent .bat launch should succeed");
        assert!(pid > 0);
    }

    #[test]
    fn test_deep_optimization_win32_ram_purge() {
        let cfg = OptimizerConfig::default();
        let mut queue = Vec::new();
        let report =
            optimizer::run_deep_optimization(&cfg, &mut queue).expect("optimization should succeed");
        assert!(report.processes_trimmed > 0);
        assert!(report.summary.contains("Game Mode: ON"));
    }
}
