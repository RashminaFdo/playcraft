//! PlayCraft Domain Models, Launcher Settings & Full Hardware Specification Models
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

use serde::{Deserialize, Serialize};

pub const AUTHOR_NAME: &str = "Rashmina Fernando";
pub const AUTHOR_URL: &str = "https://www.rashmina.dev";
pub const AUTHOR_DISPLAY: &str = "Rashmina Fernando (www.rashmina.dev)";
pub const APP_TITLE: &str = "PlayCraft by Rashmina Fernando (www.rashmina.dev)";
pub const LICENSE_NOTICE: &str =
    "Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorMetadata {
    pub app_name: String,
    pub version: String,
    pub author: String,
    pub website: String,
    pub copyright: String,
    pub architecture: String,
}

impl Default for AuthorMetadata {
    fn default() -> Self {
        Self {
            app_name: APP_TITLE.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: AUTHOR_NAME.to_string(),
            website: AUTHOR_URL.to_string(),
            copyright: LICENSE_NOTICE.to_string(),
            architecture: "Tauri v2 Native Rust + Hardware-Accelerated WebView2 (60+ FPS)".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperItem {
    pub id: String,
    pub name: String,
    pub url: String,
    pub subtitle: String,
}

fn default_on_game_launch_action() -> String {
    "keep_open".to_string()
}

fn default_close_button_action() -> String {
    "exit_app".to_string()
}

fn default_window_mode() -> String {
    "fullscreen".to_string()
}

fn default_target_monitor() -> String {
    "primary".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorSpec {
    pub name: String,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherSettings {
    pub selected_wallpaper: String,
    pub wallpaper_mode: String, // "wallpaper_priority" | "game_hero_priority"
    pub start_fullscreen: bool,
    #[serde(default = "default_window_mode")]
    pub window_mode: String, // "fullscreen" | "borderless" | "windowed"
    #[serde(default = "default_target_monitor")]
    pub target_monitor: String, // "primary" | monitor name
    pub minimize_to_tray: bool,
    pub launch_on_windows_startup: bool,
    #[serde(default = "default_on_game_launch_action")]
    pub on_game_launch_action: String, // Always "keep_open"
    #[serde(default = "default_close_button_action")]
    pub close_button_action: String,   // "exit_app" | "minimize_to_tray"
    pub enable_ui_sounds: bool,
    pub ui_volume_percent: u8,
    pub enable_boot_video: bool,
    pub enable_particles: bool,
    pub show_fps_counter: bool,
    pub show_ram_in_topbar: bool,
    pub custom_wallpapers: Vec<WallpaperItem>,
    #[serde(default)]
    pub last_update_check_ts: u64,
    #[serde(default)]
    pub latest_known_version: String,
    #[serde(default)]
    pub latest_release_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_title: String,
    pub release_notes: String,
    pub release_url: String,
    pub setup_download_url: String,
    pub checked_at_ts: u64,
    pub offline_or_error: bool,
    pub status_message: String,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            selected_wallpaper: "assets/wallpapers/wall_1.jpeg".to_string(),
            wallpaper_mode: "game_hero_priority".to_string(),
            start_fullscreen: true,
            window_mode: "fullscreen".to_string(),
            target_monitor: "primary".to_string(),
            minimize_to_tray: false,
            launch_on_windows_startup: false,
            on_game_launch_action: "keep_open".to_string(),
            close_button_action: "exit_app".to_string(),
            enable_ui_sounds: true,
            ui_volume_percent: 80,
            enable_boot_video: true,
            enable_particles: true,
            show_fps_counter: true,
            show_ram_in_topbar: true,
            custom_wallpapers: Vec::new(),
            last_update_check_ts: 0,
            latest_known_version: String::new(),
            latest_release_url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectedFileResult {
    pub exe_path: String,
    pub working_dir: String,
    pub clean_title_query: String,
    pub size_on_disk_gb: f32,
    pub source_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSpec {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub disk_type: String,
    pub total_gb: f32,
    pub available_gb: f32,
    pub used_gb: f32,
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSpec {
    pub name: String,
    pub vendor: String,
    pub driver_version: String,
    pub vram_gb: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSystemSpecs {
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub hostname: String,
    pub architecture: String,
    pub uptime_formatted: String,
    pub motherboard: String,
    pub bios_version: String,
    pub cpu_brand: String,
    pub cpu_vendor: String,
    pub physical_cores: usize,
    pub logical_threads: usize,
    pub cpu_frequency_mhz: u64,
    pub cpu_usage_percent: f32,
    pub gpus: Vec<GpuSpec>,
    pub ram_total_gb: f32,
    pub ram_used_gb: f32,
    pub ram_available_gb: f32,
    pub ram_usage_percent: f32,
    pub swap_total_gb: f32,
    pub swap_used_gb: f32,
    pub primary_display_resolution: String,
    pub disks: Vec<DiskSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GameSource {
    Steam,
    Epic,
    Gog,
    Ubisoft,
    EaDesktop,
    Executable,
    BatchScript,
    Shortcut,
    PlayCraftHub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayModeSpec {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveBackupEntry {
    pub id: String,
    pub game_id: String,
    pub game_title: String,
    pub timestamp: String,
    pub backup_path: String,
    pub file_count: usize,
    pub size_mb: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSessionEntry {
    pub id: String,
    pub game_id: String,
    pub game_title: String,
    pub started_at: String,
    pub duration_minutes: f32,
    pub store_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalMediaItem {
    pub name: String,
    pub file_path: String,
    pub media_type: String,
    pub source_app: String,
    pub size_mb: f32,
    pub modified_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrmBypassConfig {
    pub enabled: bool,
    pub bypass_steam_stub: bool,
    pub bypass_epic_portal: bool,
    pub skip_intro_videos: bool,
    pub force_direct_shipping_exe: bool,
    pub offline_mode_env: bool,
    pub high_priority_process: bool,
    pub custom_args: String,
}

impl Default for DrmBypassConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bypass_steam_stub: false,
            bypass_epic_portal: false,
            skip_intro_videos: true,
            force_direct_shipping_exe: true,
            offline_mode_env: false,
            high_priority_process: true,
            custom_args: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityCard {
    pub title: String,
    pub subtitle: String,
    pub tag: String,
    pub image_url: String,
    pub progress_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub genre: String,
    pub source: GameSource,
    pub exe_path: String,
    pub working_dir: String,
    pub launch_args: String,
    pub steam_app_id: Option<u32>,
    pub cover_url: String,
    pub hero_url: String,
    pub accent_color: String,
    pub playtime_hours: f32,
    pub last_played: String,
    pub achievements_unlocked: u32,
    pub achievements_total: u32,
    pub size_on_disk_gb: f32,
    pub deep_optimize_before_launch: bool,
    pub drm_bypass: DrmBypassConfig,
    pub activities: Vec<ActivityCard>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub save_folder_path: String,
    #[serde(default)]
    pub auto_backup_saves: bool,
    #[serde(default)]
    pub mod_folder_path: String,
    #[serde(default = "default_launch_profile")]
    pub active_launch_profile: String,
    #[serde(default)]
    pub target_refresh_hz: Option<u32>,
    #[serde(default)]
    pub target_resolution: Option<String>,
    #[serde(default)]
    pub is_favorite: bool,
}

fn default_launch_profile() -> String {
    "Default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameWorkspaceConfigUpdate {
    pub game_id: String,
    pub notes: String,
    pub save_folder_path: String,
    pub auto_backup_saves: bool,
    pub mod_folder_path: String,
    pub active_launch_profile: String,
    pub target_refresh_hz: Option<u32>,
    pub target_resolution: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub hero_url: Option<String>,
    #[serde(default)]
    pub is_favorite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewGameRequest {
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub exe_path: String,
    pub launch_args: Option<String>,
    pub cover_url: Option<String>,
    pub hero_url: Option<String>,
    pub steam_app_id: Option<u32>,
    pub deep_optimize_before_launch: bool,
    pub drm_bypass: Option<DrmBypassConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorableProcess {
    pub name: String,
    pub pid: u32,
    pub exe_path: String,
    pub cmd_args: Vec<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    pub enable_windows_game_mode: bool,
    pub purge_standby_ram: bool,
    pub auto_kill_bloat_processes: bool,
    pub auto_restore_after_exit: bool,
    pub elevate_game_priority: bool,
    pub target_bloat_names: Vec<String>,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            enable_windows_game_mode: true,
            purge_standby_ram: true,
            auto_kill_bloat_processes: true,
            auto_restore_after_exit: true,
            elevate_game_priority: true,
            target_bloat_names: vec![
                "Cortana.exe".to_string(),
                "YourPhone.exe".to_string(),
                "PhoneExperienceHost.exe".to_string(),
                "Widgets.exe".to_string(),
                "WidgetService.exe".to_string(),
                "GameBarPresenceWriter.exe".to_string(),
                "SearchApp.exe".to_string(),
                "Spotify.exe".to_string(),
                "SpotifyWebHelper.exe".to_string(),
                "iTunesHelper.exe".to_string(),
                "AdobeARM.exe".to_string(),
                "CCXProcess.exe".to_string(),
                "CCLibrary.exe".to_string(),
                "AdobeIPCBroker.exe".to_string(),
                "AdobeNotificationClient.exe".to_string(),
                "CoreSync.exe".to_string(),
                "AdobeUpdateService.exe".to_string(),
                "Teams.exe".to_string(),
                "ms-teams.exe".to_string(),
                "Slack.exe".to_string(),
                "Zoom.exe".to_string(),
                "Dropbox.exe".to_string(),
                "GoogleDriveFS.exe".to_string(),
                "GoogleUpdate.exe".to_string(),
                "BraveUpdate.exe".to_string(),
                "Overwolf.exe".to_string(),
                "Wallpaper32.exe".to_string(),
                "Wallpaper64.exe".to_string(),
                "ArmouryCrate.UserSessionHelper.exe".to_string(),
                "RazerCentralService.exe".to_string(),
                "LGHUB_agent.exe".to_string(),
                "WinStore.App.exe".to_string(),
                "CalculatorApp.exe".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerReport {
    pub timestamp: String,
    pub game_mode_enabled: bool,
    pub ram_before_mb: u64,
    pub ram_after_mb: u64,
    pub ram_freed_mb: u64,
    pub processes_trimmed: u32,
    pub bloat_killed: Vec<String>,
    pub pending_restore_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedMobileClient {
    pub id: String,
    pub remote_addr: String,
    pub user_agent: String,
    pub connected_at: String,
    pub last_input: String,
    pub latency_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobilePairingInfo {
    pub ip_address: String,
    #[serde(default)]
    pub available_ips: Vec<String>,
    pub port: u16,
    #[serde(default)]
    pub discovery_port: u16,
    pub pin_code: String,
    pub pairing_url: String,
    pub ws_url: String,
    pub qr_svg: String,
    pub connected_clients: Vec<ConnectedMobileClient>,
    pub total_inputs_processed: u64,
    pub virtual_controller_backend: String,
    #[serde(default)]
    pub vigem_active: bool,
    #[serde(default)]
    pub bluetooth_spp_active: bool,
    #[serde(default)]
    pub bluetooth_pc_name: String,
    #[serde(default)]
    pub apk_path: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobileControllerPacket {
    pub packet_type: String,
    pub pin: Option<String>,
    pub control: Option<String>,
    pub custom_key: Option<String>,
    pub controller_style: Option<String>,
    #[serde(default)]
    pub input_mode: Option<String>,
    pub state: Option<String>,
    pub lx: Option<f32>,
    pub ly: Option<f32>,
    pub rx: Option<f32>,
    pub ry: Option<f32>,
    #[serde(default)]
    pub lt: Option<f32>,
    #[serde(default)]
    pub rt: Option<f32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub steer: Option<f32>,
    pub tilt_deg: Option<f32>,
    pub client_ts: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemTelemetry {
    pub cpu_usage_percent: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_usage_percent: f32,
    pub active_game_title: Option<String>,
    pub active_game_pid: Option<u32>,
    pub overlay_active: bool,
    pub xinput_controllers_connected: u32,
    pub vigem_bus_detected: bool,
    pub mobile_clients_connected: usize,
}
