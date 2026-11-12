//! PlayCraft Universal Game Scanner & Steam Library Integration
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

use crate::error::{PlayCraftError, PlayCraftResult};
use crate::models::{ActivityCard, DrmBypassConfig, GameEntry, GameSource, InspectedFileResult};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use walkdir::WalkDir;
use winreg::enums::*;
use winreg::RegKey;

#[cfg(target_os = "windows")]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
pub const HIGH_PRIORITY_CLASS: u32 = 0x00000080;

const IGNORED_EXE_PATTERNS: &[&str] = &[
    "unins",
    "uninstall",
    "crash",
    "redist",
    "vcredist",
    "dxsetup",
    "dotnet",
    "easyanticheat",
    "battleye",
    "cefprocess",
    "unitycrashhandler",
    "setup",
    "updater",
    "helper",
    "reporter",
];

pub fn detect_steam_root() -> Option<PathBuf> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(steam_key) = hkcu.open_subkey("Software\\Valve\\Steam") {
        if let Ok(path_str) = steam_key.get_value::<String, _>("SteamPath") {
            let p = PathBuf::from(path_str.replace('/', "\\"));
            if p.exists() {
                return Some(p);
            }
        }
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for sub in [
        "SOFTWARE\\WOW6432Node\\Valve\\Steam",
        "SOFTWARE\\Valve\\Steam",
    ] {
        if let Ok(steam_key) = hklm.open_subkey(sub) {
            if let Ok(path_str) = steam_key.get_value::<String, _>("InstallPath") {
                let p = PathBuf::from(path_str);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    for candidate in [
        "C:\\Program Files (x86)\\Steam",
        "C:\\Program Files\\Steam",
        "D:\\Steam",
        "E:\\Steam",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

fn extract_vdf_kv(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") || !trimmed.starts_with('"') {
        return None;
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut prev_backslash = false;

    for ch in trimmed.chars() {
        if ch == '"' && !prev_backslash {
            if in_quotes {
                parts.push(current.clone());
                current.clear();
                in_quotes = false;
            } else {
                in_quotes = true;
            }
        } else if in_quotes {
            current.push(ch);
        }
        prev_backslash = ch == '\\' && !prev_backslash;
    }

    if parts.len() >= 2 {
        Some((parts[0].clone(), parts[1].replace("\\\\", "\\")))
    } else {
        None
    }
}

pub fn discover_steam_libraries() -> Vec<PathBuf> {
    let mut libs = Vec::new();
    let Some(root) = detect_steam_root() else {
        return libs;
    };
    libs.push(root.clone());

    let vdf_path = root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(content) = fs::read_to_string(&vdf_path) {
        for line in content.lines() {
            if let Some((k, v)) = extract_vdf_kv(line) {
                if k.eq_ignore_ascii_case("path") {
                    let lib_path = PathBuf::from(v);
                    if lib_path.exists() && !libs.contains(&lib_path) {
                        libs.push(lib_path);
                    }
                }
            }
        }
    }
    libs
}

fn load_steam_playtimes(steam_root: &Path) -> HashMap<u32, f32> {
    let mut map = HashMap::new();
    let userdata_dir = steam_root.join("userdata");
    if !userdata_dir.exists() {
        return map;
    }

    for entry in WalkDir::new(userdata_dir)
        .max_depth(4)
        .into_iter()
        .flatten()
    {
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("localconfig.vdf")
        {
            if let Ok(text) = fs::read_to_string(entry.path()) {
                let mut current_app_id: Option<u32> = None;
                for line in text.lines() {
                    let t = line.trim();
                    if t.starts_with('"') && !t.contains('\t') && t.ends_with('"') {
                        let token = t.trim_matches('"');
                        if let Ok(id) = token.parse::<u32>() {
                            current_app_id = Some(id);
                        }
                    } else if let Some(app_id) = current_app_id {
                        if let Some((k, v)) = extract_vdf_kv(t) {
                            if k.eq_ignore_ascii_case("Playtime") {
                                if let Ok(mins) = v.parse::<f32>() {
                                    map.insert(app_id, (mins / 60.0 * 10.0).round() / 10.0);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

pub fn find_best_executable_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.exists() {
        return None;
    }

    let mut candidates: Vec<(PathBuf, u64, bool)> = Vec::new();
    for entry in WalkDir::new(dir).max_depth(3).into_iter().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("exe") {
            continue;
        }

        let name_lower = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if IGNORED_EXE_PATTERNS
            .iter()
            .any(|pat| name_lower.contains(pat))
        {
            continue;
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let is_shipping = name_lower.contains("shipping");
        candidates.push((path.to_path_buf(), size, is_shipping));
    }

    candidates.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.cmp(&a.1)));
    candidates.first().map(|c| c.0.clone())
}

pub fn scan_steam_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    let libraries = discover_steam_libraries();
    let playtimes = detect_steam_root()
        .map(|r| load_steam_playtimes(&r))
        .unwrap_or_default();

    for lib in libraries {
        let steamapps = lib.join("steamapps");
        let Ok(read_dir) = fs::read_dir(&steamapps) else {
            continue;
        };

        for item in read_dir.flatten() {
            let path = item.path();
            let fname = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if !fname.starts_with("appmanifest_") || !fname.ends_with(".acf") {
                continue;
            }

            let Ok(acf_text) = fs::read_to_string(&path) else {
                continue;
            };

            let mut appid: Option<u32> = None;
            let mut name: Option<String> = None;
            let mut installdir: Option<String> = None;
            let mut size_bytes: u64 = 0;

            for line in acf_text.lines() {
                if let Some((k, v)) = extract_vdf_kv(line) {
                    match k.as_str() {
                        "appid" => appid = v.parse::<u32>().ok(),
                        "name" => name = Some(v),
                        "installdir" => installdir = Some(v),
                        "SizeOnDisk" => size_bytes = v.parse::<u64>().unwrap_or(0),
                        _ => {}
                    }
                }
            }

            let (Some(app_id), Some(game_name), Some(install_folder)) = (appid, name, installdir)
            else {
                continue;
            };

            let lower_title = game_name.to_lowercase();
            if app_id == 228980
                || lower_title.contains("redistributable")
                || lower_title.contains("steamworks common")
                || lower_title.contains("steam linux runtime")
                || lower_title.starts_with("proton ")
            {
                continue;
            }

            let full_install_dir = steamapps.join("common").join(&install_folder);
            let detected_exe = find_best_executable_in_dir(&full_install_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("steam://rungameid/{}", app_id));

            let cover_url = format!(
                "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_600x900.jpg",
                app_id
            );
            let hero_url = format!(
                "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_hero.jpg",
                app_id
            );
            let header_url = format!(
                "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
                app_id
            );

            let hours = playtimes.get(&app_id).copied().unwrap_or(0.0);
            let size_gb = ((size_bytes as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;

            games.push(GameEntry {
                id: format!("steam-{}", app_id),
                title: game_name.clone(),
                subtitle: format!("Steam Installed • AppID {}", app_id),
                description: format!(
                    "Installed in local Steam Library ({}). Ready for direct execution with PlayCraft Deep Optimizations.",
                    full_install_dir.display()
                ),
                genre: "Steam Installed Game".to_string(),
                source: GameSource::Steam,
                exe_path: detected_exe,
                working_dir: full_install_dir.to_string_lossy().to_string(),
                launch_args: String::new(),
                steam_app_id: Some(app_id),
                cover_url,
                hero_url,
                accent_color: "#00f0ff".to_string(),
                playtime_hours: hours,
                last_played: if hours > 0.0 { "Synced from Steam".to_string() } else { "Never Played".to_string() },
                achievements_unlocked: 0,
                achievements_total: 0,
                size_on_disk_gb: size_gb,
                deep_optimize_before_launch: true,
                drm_bypass: DrmBypassConfig::default(),
                activities: vec![
                    ActivityCard {
                        title: format!("{} — Local Installation", game_name),
                        subtitle: full_install_dir.to_string_lossy().to_string(),
                        tag: "STEAM LIBRARY".to_string(),
                        image_url: header_url,
                        progress_percent: None,
                    },
                ],
                notes: String::new(),
                save_folder_path: detect_likely_save_folder(&game_name),
                auto_backup_saves: false,
                mod_folder_path: full_install_dir.to_string_lossy().to_string(),
                active_launch_profile: "Default".to_string(),
                target_refresh_hz: None,
                target_resolution: None,
                is_favorite: false,
            });
        }
    }

    info!("Steam scanner discovered {} local Steam games", games.len());
    games
}

pub fn detect_likely_save_folder(game_title: &str) -> String {
    let clean: String = game_title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    if clean.len() < 3 {
        return String::new();
    }

    let mut search_roots = Vec::new();
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(&profile);
        search_roots.push(p.join("Saved Games"));
        search_roots.push(p.join("Documents").join("My Games"));
        search_roots.push(p.join("Documents"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        search_roots.push(PathBuf::from(appdata));
    }
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        search_roots.push(PathBuf::from(localappdata));
    }

    for root in search_roots {
        let Ok(rd) = fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let folder_clean: String = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            if folder_clean.len() >= 3 && (folder_clean.contains(&clean) || clean.contains(&folder_clean)) {
                return path.to_string_lossy().to_string();
            }
        }
    }
    String::new()
}

pub fn scan_epic_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    let manifests_dir = Path::new("C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests");
    if !manifests_dir.exists() {
        return games;
    }

    let Ok(rd) = fs::read_dir(manifests_dir) else {
        return games;
    };

    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };

        let display_name = json
            .get("DisplayName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let install_loc = json
            .get("InstallLocation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let launch_exe = json
            .get("LaunchExecutable")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let app_name = json
            .get("AppName")
            .and_then(|v| v.as_str())
            .unwrap_or("epic")
            .to_string();
        let install_size = json
            .get("InstallSize")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if display_name.is_empty() || install_loc.is_empty() {
            continue;
        }

        let install_path = PathBuf::from(&install_loc);
        if !install_path.exists() {
            continue;
        }

        let full_exe = if !launch_exe.is_empty() {
            install_path.join(&launch_exe)
        } else {
            find_best_executable_in_dir(&install_path).unwrap_or_else(|| install_path.clone())
        };

        let size_gb = ((install_size as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0;

        let (cover_url, hero_url, desc_text, genre_text, matched_appid) =
            lookup_online_artwork_for_title(&display_name).unwrap_or_else(|| {
                (
                    "assets/playcraft-logo.jpg".to_string(),
                    "assets/wallpapers/wall_1.jpeg".to_string(),
                    format!("Installed via Epic Games Launcher at {}.", install_loc),
                    "Epic Games Store".to_string(),
                    None,
                )
            });

        games.push(GameEntry {
            id: format!("epic-{}", sanitize_id(&app_name, games.len())),
            title: display_name.clone(),
            subtitle: format!("Epic Games • {}", install_loc),
            description: desc_text,
            genre: genre_text,
            source: GameSource::Epic,
            exe_path: full_exe.to_string_lossy().to_string(),
            working_dir: install_loc.clone(),
            launch_args: String::new(),
            steam_app_id: matched_appid,
            cover_url: cover_url.clone(),
            hero_url,
            accent_color: "#00d8f0".to_string(),
            playtime_hours: 0.0,
            last_played: "Never Played".to_string(),
            achievements_unlocked: 0,
            achievements_total: 0,
            size_on_disk_gb: size_gb.max(0.1),
            deep_optimize_before_launch: true,
            drm_bypass: DrmBypassConfig::default(),
            activities: vec![ActivityCard {
                title: format!("{} — Epic Games Installation", display_name),
                subtitle: install_loc.clone(),
                tag: "EPIC GAMES".to_string(),
                image_url: cover_url,
                progress_percent: None,
            }],
            notes: String::new(),
            save_folder_path: detect_likely_save_folder(&display_name),
            auto_backup_saves: false,
            mod_folder_path: install_loc,
            active_launch_profile: "Default".to_string(),
            target_refresh_hz: None,
            target_resolution: None,
            is_favorite: false,
        });
    }

    info!("Epic Games scanner discovered {} installed games", games.len());
    games
}

pub fn lookup_online_artwork_for_title(
    title: &str,
) -> Option<(String, String, String, String, Option<u32>)> {
    let clean = title.trim();
    if clean.is_empty() {
        return None;
    }

    let encoded: String = clean
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => vec![b as char],
            b' ' => vec!['+'],
            _ => format!("%{:02X}", b).chars().collect(),
        })
        .collect();

    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        encoded
    );

    let mut cmd = std::process::Command::new("curl.exe");
    cmd.args(["-s", "-L", "--max-time", "4", &url]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let output = cmd.output().ok()?;
    let body = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&body).ok()?;
    let first = parsed.get("items")?.as_array()?.first()?;
    let app_id = first.get("id")?.as_u64()? as u32;
    let tiny_img = first
        .get("tiny_image")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Also query appdetails for the reliable header_image, background_raw, and short_description
    let details_url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&l=english",
        app_id
    );
    let mut d_cmd = std::process::Command::new("curl.exe");
    d_cmd.args(["-s", "-L", "--max-time", "4", &details_url]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        d_cmd.creation_flags(0x08000000);
    }

    if let Ok(d_out) = d_cmd.output() {
        let d_body = String::from_utf8_lossy(&d_out.stdout);
        if let Ok(d_json) = serde_json::from_str::<serde_json::Value>(&d_body) {
            if let Some(data) = d_json
                .get(app_id.to_string())
                .and_then(|e| e.get("data"))
            {
                let header = data
                    .get("header_image")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        data.get("capsule_image")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| tiny_img.clone());

                let hero = data
                    .get("background_raw")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        data.get("screenshots")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|s| s.get("path_full"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| header.clone());

                let desc = data
                    .get("short_description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let genre = data
                    .get("genres")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|g| g.get("description"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("PC Game")
                    .to_string();

                if !header.is_empty() {
                    return Some((header, hero, desc, genre, Some(app_id)));
                }
            }
        }
    }

    if !tiny_img.is_empty() {
        return Some((
            tiny_img.clone(),
            tiny_img,
            format!("{} — Ready to launch via PlayCraft.", title),
            "PC Game".to_string(),
            Some(app_id),
        ));
    }

    None
}

pub fn scan_gog_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(gog_root) = hklm.open_subkey("SOFTWARE\\WOW6432Node\\GOG.com\\Games") else {
        return games;
    };

    for key_name in gog_root.enum_keys().flatten() {
        let Ok(sub) = gog_root.open_subkey(&key_name) else {
            continue;
        };
        let game_name: String = sub.get_value("gameName").unwrap_or_default();
        let exe_path: String = sub.get_value("exe").unwrap_or_default();
        let work_dir: String = sub.get_value("path").unwrap_or_default();

        if game_name.trim().is_empty() || exe_path.trim().is_empty() || !Path::new(&exe_path).exists() {
            continue;
        }

        let inspected = inspect_executable_file(&exe_path).ok();
        let size_gb = inspected.map(|i| i.size_on_disk_gb).unwrap_or(1.0);

        let (cover_url, hero_url, desc_text, genre_text, matched_appid) =
            lookup_online_artwork_for_title(&game_name).unwrap_or_else(|| {
                (
                    "assets/playcraft-logo.jpg".to_string(),
                    "assets/wallpapers/wall_2.jpeg".to_string(),
                    format!("DRM-Free GOG installation at {}.", work_dir),
                    "GOG Galaxy".to_string(),
                    None,
                )
            });

        games.push(GameEntry {
            id: format!("gog-{}", key_name),
            title: game_name.clone(),
            subtitle: format!("GOG Galaxy • ID {}", key_name),
            description: desc_text,
            genre: genre_text,
            source: GameSource::Gog,
            exe_path,
            working_dir: work_dir.clone(),
            launch_args: String::new(),
            steam_app_id: matched_appid,
            cover_url,
            hero_url,
            accent_color: "#00d8f0".to_string(),
            playtime_hours: 0.0,
            last_played: "Never Played".to_string(),
            achievements_unlocked: 0,
            achievements_total: 0,
            size_on_disk_gb: size_gb,
            deep_optimize_before_launch: true,
            drm_bypass: DrmBypassConfig::default(),
            activities: vec![],
            notes: String::new(),
            save_folder_path: detect_likely_save_folder(&game_name),
            auto_backup_saves: false,
            mod_folder_path: work_dir,
            active_launch_profile: "Default".to_string(),
            target_refresh_hz: None,
            target_resolution: None,
            is_favorite: false,
        });
    }

    info!("GOG scanner discovered {} installed games", games.len());
    games
}

pub fn scan_ubisoft_games() -> Vec<GameEntry> {
    let mut games = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(ubi_root) = hklm.open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs") else {
        return games;
    };

    for key_name in ubi_root.enum_keys().flatten() {
        let Ok(sub) = ubi_root.open_subkey(&key_name) else {
            continue;
        };
        let install_dir: String = sub.get_value("InstallDir").unwrap_or_default();
        let dir_path = PathBuf::from(install_dir.replace('/', "\\"));
        if !dir_path.exists() {
            continue;
        }
        let Some(best_exe) = find_best_executable_in_dir(&dir_path) else {
            continue;
        };
        let title = dir_path
            .file_name()
            .map(|n| prettify_title(&n.to_string_lossy()))
            .unwrap_or_else(|| format!("Ubisoft Title {}", key_name));
        let inspected = inspect_executable_file(&best_exe.to_string_lossy()).ok();
        let size_gb = inspected.map(|i| i.size_on_disk_gb).unwrap_or(1.0);

        let (cover_url, hero_url, desc_text, genre_text, matched_appid) =
            lookup_online_artwork_for_title(&title).unwrap_or_else(|| {
                (
                    "assets/playcraft-logo.jpg".to_string(),
                    "assets/wallpapers/wall_3.jpeg".to_string(),
                    format!("Installed via Ubisoft Connect at {}.", dir_path.display()),
                    "Ubisoft Connect".to_string(),
                    None,
                )
            });

        games.push(GameEntry {
            id: format!("ubisoft-{}", key_name),
            title: title.clone(),
            subtitle: format!("Ubisoft Connect • {}", dir_path.display()),
            description: desc_text,
            genre: genre_text,
            source: GameSource::Ubisoft,
            exe_path: best_exe.to_string_lossy().to_string(),
            working_dir: dir_path.to_string_lossy().to_string(),
            launch_args: String::new(),
            steam_app_id: matched_appid,
            cover_url,
            hero_url,
            accent_color: "#00d8f0".to_string(),
            playtime_hours: 0.0,
            last_played: "Never Played".to_string(),
            achievements_unlocked: 0,
            achievements_total: 0,
            size_on_disk_gb: size_gb,
            deep_optimize_before_launch: true,
            drm_bypass: DrmBypassConfig::default(),
            activities: vec![],
            notes: String::new(),
            save_folder_path: detect_likely_save_folder(&title),
            auto_backup_saves: false,
            mod_folder_path: dir_path.to_string_lossy().to_string(),
            active_launch_profile: "Default".to_string(),
            target_refresh_hz: None,
            target_resolution: None,
            is_favorite: false,
        });
    }

    games
}

pub fn scan_all_stores() -> Vec<GameEntry> {
    let mut all = scan_steam_games();
    for g in scan_epic_games() {
        if !all.iter().any(|existing| existing.exe_path.eq_ignore_ascii_case(&g.exe_path)) {
            all.push(g);
        }
    }
    for g in scan_gog_games() {
        if !all.iter().any(|existing| existing.exe_path.eq_ignore_ascii_case(&g.exe_path)) {
            all.push(g);
        }
    }
    for g in scan_ubisoft_games() {
        if !all.iter().any(|existing| existing.exe_path.eq_ignore_ascii_case(&g.exe_path)) {
            all.push(g);
        }
    }
    all
}

pub fn parse_lnk_target(lnk_path: &Path) -> Option<PathBuf> {
    let bytes = fs::read(lnk_path).ok()?;
    if bytes.len() < 76 || bytes[0..4] != [0x4C, 0x00, 0x00, 0x00] {
        return None;
    }

    let mut ascii_buf = String::new();
    let mut found_paths: Vec<PathBuf> = Vec::new();

    for &b in &bytes[76..] {
        if (32..=126).contains(&b) {
            ascii_buf.push(b as char);
        } else {
            if ascii_buf.len() >= 6 {
                let lower = ascii_buf.to_lowercase();
                if (lower.ends_with(".exe") || lower.ends_with(".bat"))
                    && ascii_buf.chars().nth(1) == Some(':')
                {
                    found_paths.push(PathBuf::from(ascii_buf.clone()));
                }
            }
            ascii_buf.clear();
        }
    }

    let mut u16_chars: Vec<u16> = Vec::new();
    for chunk in bytes[76..].chunks_exact(2) {
        let code = u16::from_le_bytes([chunk[0], chunk[1]]);
        if (32..=126).contains(&code) || code > 160 {
            u16_chars.push(code);
        } else {
            if u16_chars.len() >= 6 {
                if let Ok(s) = String::from_utf16(&u16_chars) {
                    let lower = s.to_lowercase();
                    if (lower.ends_with(".exe") || lower.ends_with(".bat"))
                        && s.chars().nth(1) == Some(':')
                    {
                        found_paths.push(PathBuf::from(s));
                    }
                }
            }
            u16_chars.clear();
        }
    }

    for candidate in &found_paths {
        if candidate.exists() {
            return Some(candidate.clone());
        }
    }
    found_paths.into_iter().next()
}

pub fn scan_directory_for_games(dir_path: &str, max_depth: usize) -> PlayCraftResult<Vec<GameEntry>> {
    let root = Path::new(dir_path);
    if !root.exists() {
        return Err(PlayCraftError::ScannerError(format!(
            "Directory does not exist: {}",
            dir_path
        )));
    }

    let mut discovered = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for entry in WalkDir::new(root)
        .max_depth(max_depth.clamp(1, 6))
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext_lower = ext.to_lowercase();
        if !matches!(ext_lower.as_str(), "exe" | "bat" | "lnk") {
            continue;
        }

        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown Game".to_string());
        let stem_lower = stem.to_lowercase();

        if IGNORED_EXE_PATTERNS.iter().any(|p| stem_lower.contains(p)) {
            continue;
        }

        let (resolved_path, source_kind) = match ext_lower.as_str() {
            "lnk" => {
                let target = parse_lnk_target(path).unwrap_or_else(|| path.to_path_buf());
                (target, GameSource::Shortcut)
            }
            "bat" => (path.to_path_buf(), GameSource::BatchScript),
            _ => (path.to_path_buf(), GameSource::Executable),
        };

        let canonical_key = resolved_path.to_string_lossy().to_lowercase();
        if !seen_paths.insert(canonical_key) {
            continue;
        }

        let root_folder = resolve_game_root_folder(&resolved_path);
        let size_gb = calculate_directory_size_gb(&root_folder, Some(&resolved_path));
        let working_dir = root_folder.to_string_lossy().to_string();

        let pretty_title = prettify_title(&stem);
        let badge = match source_kind {
            GameSource::BatchScript => "Silent Batch Script (.bat)",
            GameSource::Shortcut => "Shortcut Target (.lnk)",
            _ => "Native Windows Executable (.exe)",
        };

        let (cover_url, hero_url, desc_text, genre_text, matched_appid) =
            lookup_online_artwork_for_title(&pretty_title).unwrap_or_else(|| {
                (
                    "assets/playcraft-logo.jpg".to_string(),
                    "assets/wallpapers/wall_1.jpeg".to_string(),
                    format!(
                        "Discovered by PlayCraft Universal Scanner in {}. Configured with zero-popup execution and Deep Optimization hooks.",
                        working_dir
                    ),
                    badge.to_string(),
                    None,
                )
            });

        discovered.push(GameEntry {
            id: format!("local-{}", sanitize_id(&pretty_title, discovered.len())),
            title: pretty_title.clone(),
            subtitle: badge.to_string(),
            description: desc_text,
            genre: genre_text,
            source: source_kind,
            exe_path: resolved_path.to_string_lossy().to_string(),
            working_dir,
            launch_args: String::new(),
            steam_app_id: matched_appid,
            cover_url: cover_url.clone(),
            hero_url,
            accent_color: "#00e5ff".to_string(),
            playtime_hours: 0.0,
            last_played: "Never Played".to_string(),
            achievements_unlocked: 0,
            achievements_total: 0,
            size_on_disk_gb: size_gb.max(0.1),
            deep_optimize_before_launch: true,
            drm_bypass: DrmBypassConfig::default(),
            activities: vec![ActivityCard {
                title: format!("Launch {}", pretty_title),
                subtitle: "Direct hardware-accelerated launch via PlayCraft Engine".to_string(),
                tag: "LOCAL BINARY".to_string(),
                image_url: cover_url,
                progress_percent: None,
            }],
            notes: String::new(),
            save_folder_path: detect_likely_save_folder(&pretty_title),
            auto_backup_saves: false,
            mod_folder_path: String::new(),
            active_launch_profile: "Default".to_string(),
            target_refresh_hz: None,
            target_resolution: None,
            is_favorite: false,
        });

        if discovered.len() >= 60 {
            break;
        }
    }

    info!(
        "Universal directory scan of '{}' found {} targets",
        dir_path,
        discovered.len()
    );
    Ok(discovered)
}

pub fn clean_trademark_text(raw: &str) -> String {
    let mut out = raw
        .replace("&trade;", "™")
        .replace("&TRADE;", "™")
        .replace("&reg;", "®")
        .replace("&REG;", "®")
        .replace("&amp;", "&");

    // Convert patterns like "SpeedTM " or "SpeedTM" at end of word into "Speed™"
    let bytes = out.as_bytes();
    if out.contains("TM") {
        let mut rebuilt = String::with_capacity(out.len() + 4);
        let chars: Vec<char> = out.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if i + 1 < chars.len()
                && chars[i] == 'T'
                && chars[i + 1] == 'M'
                && i > 0
                && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit())
                && (i + 2 == chars.len() || !chars[i + 2].is_ascii_alphanumeric())
            {
                rebuilt.push('™');
                i += 2;
            } else {
                rebuilt.push(chars[i]);
                i += 1;
            }
        }
        let _ = bytes;
        out = rebuilt;
    }
    out
}

pub fn resolve_game_root_folder(exe_path: &Path) -> PathBuf {
    let Some(mut current) = exe_path.parent().map(|p| p.to_path_buf()) else {
        return PathBuf::from(".");
    };

    const SUBFOLDER_NAMES: &[&str] = &[
        "bin",
        "binaries",
        "win64",
        "win32",
        "x64",
        "x86",
        "shipping",
        "retail",
        "release",
        "game",
        "engine",
        "build",
        "client",
        "launcher",
        "scripts",
        "__installer",
        "core",
    ];

    const STOP_CONTAINERS: &[&str] = &[
        "program files",
        "program files (x86)",
        "games",
        "pc games",
        "steamapps",
        "common",
        "dodi-repacks",
        "dodi repacks",
        "fitgirl repacks",
        "fitgirl-repacks",
        "epic games",
        "ubisoft game launcher",
        "gog galaxy",
        "users",
        "desktop",
        "downloads",
        "windows",
        "system32",
    ];

    for _ in 0..4 {
        let Some(folder_name) = current.file_name().and_then(|n| n.to_str()) else {
            break;
        };
        let lower = folder_name.to_lowercase();
        let is_sub = SUBFOLDER_NAMES.iter().any(|&s| lower == s || lower.starts_with("bin_"));
        if !is_sub {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        let parent_name = parent
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if parent_name.is_empty() || STOP_CONTAINERS.iter().any(|&c| parent_name == c) {
            break;
        }
        current = parent.to_path_buf();
    }

    current
}

pub fn calculate_directory_size_gb(root_dir: &Path, fallback_file: Option<&Path>) -> f32 {
    let mut total_bytes: u64 = fallback_file
        .and_then(|f| fs::metadata(f).ok())
        .map(|m| m.len())
        .unwrap_or(0);

    if root_dir.exists() && root_dir.is_dir() {
        // Safety check: don't walk an entire drive root (e.g. "C:\")
        if root_dir.parent().is_some() {
            let mut dir_bytes = 0u64;
            for entry in WalkDir::new(root_dir)
                .max_depth(10)
                .follow_links(false)
                .into_iter()
                .flatten()
                .take(50_000)
            {
                if entry.file_type().is_file() {
                    dir_bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
            if dir_bytes > total_bytes {
                total_bytes = dir_bytes;
            }
        }
    }

    ((total_bytes as f64 / 1_073_741_824.0) * 100.0).round() as f32 / 100.0
}

fn prettify_title(raw: &str) -> String {
    let cleaned = clean_trademark_text(raw)
        .replace("-Win64-Shipping", "")
        .replace("_Win64", "")
        .replace(['_', '-'], " ");
    cleaned
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_id(s: &str, idx: usize) -> String {
    let base: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    format!("{}-{}", base, idx)
}

pub fn ensure_demo_batch_script(base_dir: &Path) -> PathBuf {
    let scripts_dir = base_dir.join("sample_games");
    let _ = fs::create_dir_all(&scripts_dir);
    let bat_path = scripts_dir.join("PlayCraft_Benchmark_Silent.bat");
    if !bat_path.exists() {
        let script = "@echo off\r\n\
            REM PlayCraft Silent Batch Launcher Demo by Rashmina Fernando (www.rashmina.dev)\r\n\
            REM Executed via CREATE_NO_WINDOW without any console popup!\r\n\
            ping 127.0.0.1 -n 6 > nul\r\n\
            exit /b 0\r\n";
        let _ = fs::write(&bat_path, script);
    }
    bat_path
}

pub fn inspect_executable_file(raw_path: &str) -> PlayCraftResult<InspectedFileResult> {
    let input_path = Path::new(raw_path.trim());
    if !input_path.exists() {
        return Err(PlayCraftError::ScannerError(format!(
            "Selected file does not exist: {}",
            raw_path
        )));
    }

    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("exe")
        .to_lowercase();

    let (resolved_path, source_type) = match ext.as_str() {
        "lnk" => {
            let target = parse_lnk_target(input_path).unwrap_or_else(|| input_path.to_path_buf());
            (target, "Shortcut (.lnk)".to_string())
        }
        "bat" | "cmd" => (input_path.to_path_buf(), "Silent Batch Script (.bat)".to_string()),
        _ => (input_path.to_path_buf(), "Native Executable (.exe)".to_string()),
    };

    let root_folder = resolve_game_root_folder(&resolved_path);
    let working_dir = root_folder.to_string_lossy().to_string();

    // Calculate real total size of the game's root folder (all subdirectories & archives)
    let size_on_disk_gb = calculate_directory_size_gb(&root_folder, Some(&resolved_path));

    // Extract intelligent clean title query for online database search
    let stem = resolved_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Game");
    let mut candidate = prettify_title(stem);
    let lower_cand = candidate.to_lowercase();

    if lower_cand.len() <= 3
        || lower_cand == "game"
        || lower_cand == "shipping"
        || lower_cand == "launcher"
        || lower_cand == "client"
        || lower_cand.contains("win64")
    {
        if let Some(folder_name) = root_folder.file_name().and_then(|n| n.to_str()) {
            candidate = prettify_title(folder_name);
        }
    }

    Ok(InspectedFileResult {
        exe_path: resolved_path.to_string_lossy().to_string(),
        working_dir,
        clean_title_query: candidate,
        size_on_disk_gb,
        source_type,
    })
}

pub fn build_initial_library(_app_dir: &Path) -> Vec<GameEntry> {
    // Only return 100% real detected local games across Steam, Epic, GOG & Ubisoft
    scan_all_stores()
}

fn resolve_launch_binary(game: &GameEntry) -> PathBuf {
    let original = PathBuf::from(&game.exe_path);
    if game.drm_bypass.enabled && game.drm_bypass.force_direct_shipping_exe {
        if let Some(parent) = original.parent() {
            if let Some(shipping) = find_best_executable_in_dir(parent) {
                if shipping
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase().contains("shipping"))
                    .unwrap_or(false)
                {
                    info!(
                        "DRM Bypass resolved direct shipping binary: {}",
                        shipping.display()
                    );
                    return shipping;
                }
            }
        }
    }
    original
}

pub fn spawn_game_process(game: &GameEntry) -> PlayCraftResult<u32> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let target_path = resolve_launch_binary(game);
    let ext = target_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mut args_list: Vec<String> = Vec::new();
    if !game.launch_args.trim().is_empty() {
        args_list.extend(
            game.launch_args
                .split_whitespace()
                .map(|s| s.to_string()),
        );
    }

    if game.drm_bypass.enabled {
        if game.drm_bypass.bypass_steam_stub && !args_list.iter().any(|a| a == "-nosteam") {
            args_list.push("-nosteam".to_string());
        }
        if game.drm_bypass.bypass_epic_portal && !args_list.iter().any(|a| a == "-epicportal") {
            args_list.push("-epicportal".to_string());
        }
        if game.drm_bypass.skip_intro_videos && !args_list.iter().any(|a| a == "-novid") {
            args_list.push("-novid".to_string());
        }
        if !game.drm_bypass.custom_args.trim().is_empty() {
            for custom in game.drm_bypass.custom_args.split_whitespace() {
                if !args_list.iter().any(|a| a == custom) {
                    args_list.push(custom.to_string());
                }
            }
        }
    }

    if game.exe_path.starts_with("steam://") && !game.drm_bypass.bypass_steam_stub {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/C", "start", "", &game.exe_path])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let child = cmd
            .spawn()
            .map_err(|e| PlayCraftError::LaunchFailed(e.to_string()))?;
        return Ok(child.id());
    }

    if !target_path.exists() {
        warn!(
            "Target path '{}' not found on disk; falling back to PlayCraft silent demo runner",
            target_path.display()
        );
        let fallback_bat = ensure_demo_batch_script(Path::new("."));
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C")
            .arg(&fallback_bat)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let child = cmd
            .spawn()
            .map_err(|e| PlayCraftError::LaunchFailed(e.to_string()))?;
        return Ok(child.id());
    }

    let mut cmd = if ext == "bat" || ext == "cmd" {
        let mut c = Command::new("cmd.exe");
        c.arg("/C").arg(&target_path).args(&args_list);
        c.creation_flags(CREATE_NO_WINDOW | 0x00000200); // CREATE_NEW_PROCESS_GROUP
        c
    } else {
        let mut c = Command::new(&target_path);
        c.args(&args_list);
        let mut flags = 0x00000200; // CREATE_NEW_PROCESS_GROUP
        if game.drm_bypass.high_priority_process {
            flags |= HIGH_PRIORITY_CLASS;
        }
        c.creation_flags(flags);
        c
    };

    if !game.working_dir.is_empty() && Path::new(&game.working_dir).exists() {
        cmd.current_dir(&game.working_dir);
    } else if let Some(parent) = target_path.parent() {
        if parent.exists() {
            cmd.current_dir(parent);
        }
    }

    if let Some(app_id) = game.steam_app_id {
        cmd.env("SteamAppId", app_id.to_string());
        cmd.env("SteamGameId", app_id.to_string());
    }
    if game.drm_bypass.enabled && game.drm_bypass.offline_mode_env {
        cmd.env("STEAM_OFFLINE", "1");
        cmd.env("EOS_USE_ANTICHEATCLIENTNULL", "1");
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = cmd
        .spawn()
        .map_err(|e| PlayCraftError::LaunchFailed(format!("{}: {}", target_path.display(), e)))?;

    let pid = child.id();
    info!(
        "Launched '{}' (PID: {}) with args {:?}",
        game.title, pid, args_list
    );
    Ok(pid)
}
