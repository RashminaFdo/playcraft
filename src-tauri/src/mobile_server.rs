//! PlayCraft Native Mobile Controller Bridge Server (Multi-Transport)
//! Transports:
//!   1. TCP WebSocket (0.0.0.0:19550) — Wi-Fi, Phone Hotspot, USB Tethering/ADB, Bluetooth PAN
//!   2. UDP Auto-Discovery Beacon (0.0.0.0:19552) — Instant zero-config PC discovery on LAN/Hotspot
//!   3. Native Windows Bluetooth Classic RFCOMM SPP — Direct Bluetooth without Wi-Fi/Hotspot
//! (All browser website controller HTML/TLS endpoints removed; dedicated to native PlayCraft Android App.)
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

use crate::input_bridge::{
    is_virtual_x360_active, reset_virtual_x360_controller, send_virtual_controller_button,
    send_virtual_mouse_move, send_xinput_button, send_xinput_left_stick, send_xinput_right_stick,
    send_xinput_triggers, subscribe_xinput_rumble, translate_stick_axes_to_direction,
    virtual_controller_backend_label,
};
use crate::models::{
    ConnectedMobileClient, MobileControllerPacket, MobilePairingInfo, AUTHOR_DISPLAY,
};
use futures_util::{SinkExt, StreamExt};
use qrcode::render::svg;
use qrcode::QrCode;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, UdpSocket};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

/// Custom PlayCraft Bluetooth Classic SPP Service UUID: 7a9c1b20-4e3f-4c8d-9a10-706c61796372
pub const PLAYCRAFT_BT_UUID_U128: u128 = 0x7A9C_1B20_4E3F_4C8D_9A10_706C_6179_6372;
pub const MOBILE_DISCOVERY_PORT: u16 = 19552;

#[derive(Debug)]
pub struct MobileServerState {
    pub ip_address: String,
    pub port: u16,
    pub discovery_port: u16,
    pub pin_code: Mutex<String>,
    pub total_inputs: AtomicU64,
    pub clients: Mutex<HashMap<String, ConnectedMobileClient>>,
    pub bt_spp_active: AtomicBool,
    pub bt_pc_name: Mutex<String>,
}

impl MobileServerState {
    pub fn new(ip_address: String, port: u16, pin: String) -> Self {
        Self {
            ip_address,
            port,
            discovery_port: MOBILE_DISCOVERY_PORT,
            pin_code: Mutex::new(pin),
            total_inputs: AtomicU64::new(0),
            clients: Mutex::new(HashMap::new()),
            bt_spp_active: AtomicBool::new(false),
            bt_pc_name: Mutex::new(detect_pc_bluetooth_name()),
        }
    }

    pub fn regenerate_pin(&self) -> MobilePairingInfo {
        let new_pin = format!("{:04}", rand::thread_rng().gen_range(1000..=9999));
        if let Ok(mut guard) = self.pin_code.lock() {
            *guard = new_pin;
        }
        self.build_pairing_info()
    }

    pub fn build_pairing_info(&self) -> MobilePairingInfo {
        let pin = self
            .pin_code
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|_| "1234".to_string());

        let available_ips = collect_all_lan_ipv4s(&self.ip_address);
        let primary_ip = available_ips
            .first()
            .cloned()
            .unwrap_or_else(|| self.ip_address.clone());

        let ips_param = available_ips.join(",");
        let bt_name = self
            .bt_pc_name
            .lock()
            .map(|b| b.clone())
            .unwrap_or_default();
        let bt_spp_active = self.bt_spp_active.load(Ordering::Relaxed);

        let pairing_url = format!(
            "playcraft://pair?ips={}&port={}&discover={}&pin={}&bt={}",
            ips_param, self.port, self.discovery_port, pin, bt_name
        );
        let ws_url = format!("ws://{}:{}/ws", primary_ip, self.port);
        let qr_svg = generate_qr_svg(&pairing_url);

        let connected_clients = self
            .clients
            .lock()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default();

        MobilePairingInfo {
            ip_address: primary_ip,
            available_ips,
            port: self.port,
            discovery_port: self.discovery_port,
            pin_code: pin,
            pairing_url,
            ws_url,
            qr_svg,
            connected_clients,
            total_inputs_processed: self.total_inputs.load(Ordering::Relaxed),
            virtual_controller_backend: virtual_controller_backend_label(),
            vigem_active: is_virtual_x360_active(),
            bluetooth_spp_active: bt_spp_active,
            bluetooth_pc_name: bt_name,
            apk_path: find_built_mobile_apk_path(),
            author: AUTHOR_DISPLAY.to_string(),
        }
    }
}

pub fn detect_pc_bluetooth_name() -> String {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Devices::Bluetooth::{
            BluetoothFindFirstRadio, BluetoothFindRadioClose, BluetoothGetRadioInfo,
            BLUETOOTH_FIND_RADIO_PARAMS, BLUETOOTH_RADIO_INFO,
        };
        use windows_sys::Win32::Foundation::CloseHandle;

        unsafe {
            let params = BLUETOOTH_FIND_RADIO_PARAMS {
                dwSize: std::mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
            };
            let mut h_radio = 0isize as _;
            let h_find = BluetoothFindFirstRadio(&params, &mut h_radio);
            if !h_find.is_null() {
                let mut info: BLUETOOTH_RADIO_INFO = std::mem::zeroed();
                info.dwSize = std::mem::size_of::<BLUETOOTH_RADIO_INFO>() as u32;
                let res = BluetoothGetRadioInfo(h_radio, &mut info);
                let _ = CloseHandle(h_radio);
                let _ = BluetoothFindRadioClose(h_find);
                if res == 0 {
                    let len = info
                        .szName
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(info.szName.len());
                    let name = String::from_utf16_lossy(&info.szName[..len])
                        .trim()
                        .to_string();
                    if !name.is_empty() {
                        return name;
                    }
                }
            }
        }
    }
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PlayCraft-PC".to_string())
}

pub fn find_built_mobile_apk_path() -> String {
    let candidates = [
        "dist-bundle/PlayCraft-by-Rashmina-Fernando/PlayCraft-Controller.apk",
        "mobile-app/PlayCraft-Controller.apk",
        "mobile-app/app/build/outputs/apk/release/app-release.apk",
        "mobile-app/app/build/outputs/apk/debug/app-debug.apk",
        "PlayCraft-Controller.apk",
    ];
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for rel in candidates {
        let p = cwd.join(rel);
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in candidates {
                let p = dir.join(rel);
                if p.exists() {
                    return p.to_string_lossy().to_string();
                }
            }
        }
    }
    String::new()
}

/// Pure LAN IPv4 auto-detection using temporary UDP socket trick (no external network call).
pub fn detect_lan_ip() -> String {
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = sock.local_addr() {
                let ip = local_addr.ip().to_string();
                if !ip.starts_with("127.") && !ip.contains(':') {
                    return ip;
                }
            }
        }
    }
    collect_all_lan_ipv4s("127.0.0.1")
        .into_iter()
        .next()
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// Collects every active non-loopback IPv4 address (Wi-Fi, Phone Hotspot, USB RNDIS, Bluetooth PAN).
pub fn collect_all_lan_ipv4s(preferred_ip: &str) -> Vec<String> {
    let mut ips: Vec<String> = Vec::new();
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_name, addr) in ifaces {
            if let std::net::IpAddr::V4(v4) = addr {
                let oct = v4.octets();
                if v4.is_loopback() || (oct[0] == 169 && oct[1] == 254) {
                    continue;
                }
                let s = v4.to_string();
                if !ips.contains(&s) {
                    ips.push(s);
                }
            }
        }
    }
    if !preferred_ip.is_empty()
        && !preferred_ip.starts_with("127.")
        && !ips.iter().any(|x| x == preferred_ip)
    {
        ips.insert(0, preferred_ip.to_string());
    } else if let Some(pos) = ips.iter().position(|x| x == preferred_ip) {
        if pos > 0 {
            let item = ips.remove(pos);
            ips.insert(0, item);
        }
    }
    if ips.is_empty() {
        ips.push("127.0.0.1".to_string());
    }
    ips
}

pub fn generate_qr_svg(url: &str) -> String {
    match QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let rendered = code
                .render::<svg::Color>()
                .min_dimensions(210, 210)
                .max_dimensions(210, 210)
                .dark_color(svg::Color("#0b0f19"))
                .light_color(svg::Color("#ffffff"))
                .quiet_zone(true)
                .build();
            if let Some(idx) = rendered.find("<svg") {
                rendered[idx..].to_string()
            } else {
                rendered
            }
        }
        Err(_) => "<svg xmlns='http://www.w3.org/2000/svg' width='210' height='210'></svg>".into(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!(
        "{:02}:{:02}:{:02} UTC",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Async stream wrapper that replays prefetched HTTP Upgrade bytes before reading the inner TCP stream.
struct PrefetchStream<S> {
    inner: S,
    prefetched: Vec<u8>,
    offset: usize,
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefetchStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.offset < self.prefetched.len() {
            let remaining = &self.prefetched[self.offset..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.offset += to_copy;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefetchStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Starts TCP WebSocket server (19550), UDP Auto-Discovery server (19552), and Native Windows Bluetooth RFCOMM SPP listener.
pub async fn start_mobile_server(
    app_handle: AppHandle,
    state: Arc<MobileServerState>,
) -> anyhow::Result<()> {
    let ws_bind_addr = format!("0.0.0.0:{}", state.port);
    let tcp_listener = TcpListener::bind(&ws_bind_addr).await?;
    info!(
        "PlayCraft Native Mobile WebSocket Server listening on ws://0.0.0.0:{}",
        state.port
    );

    // 1. Spawn UDP Auto-Discovery Beacon (handles Wi-Fi, Phone Hotspot, USB RNDIS, BT PAN)
    let udp_state = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_udp_discovery_server(udp_state).await {
            warn!("PlayCraft UDP Discovery server stopped: {}", e);
        }
    });

    // 2. Spawn Native Windows Bluetooth Classic RFCOMM SPP Server (Direct Bluetooth without Wi-Fi)
    #[cfg(target_os = "windows")]
    {
        let bt_app = app_handle.clone();
        let bt_state = state.clone();
        std::thread::Builder::new()
            .name("playcraft-bt-spp".to_string())
            .spawn(move || {
                run_windows_bluetooth_spp_server(bt_app, bt_state);
            })
            .ok();
    }

    // 3. Accept WebSocket controller connections
    loop {
        match tcp_listener.accept().await {
            Ok((stream, peer_addr)) => {
                let _ = stream.set_nodelay(true);
                let app = app_handle.clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = handle_tcp_connection(stream, peer_addr, app, st).await {
                        warn!("Mobile WS connection closed ({}): {}", peer_addr, e);
                    }
                });
            }
            Err(e) => {
                warn!("Mobile TCP accept error: {}", e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn run_udp_discovery_server(state: Arc<MobileServerState>) -> anyhow::Result<()> {
    let bind_addr = format!("0.0.0.0:{}", state.discovery_port);
    let sock = UdpSocket::bind(&bind_addr).await?;
    let _ = sock.set_broadcast(true);
    info!(
        "PlayCraft UDP Auto-Discovery listening on 0.0.0.0:{}",
        state.discovery_port
    );

    let mut buf = [0u8; 1024];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, src)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                if text.trim().starts_with("PLAYCRAFT_DISCOVER") {
                    let info = state.build_pairing_info();
                    let pc_name =
                        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PlayCraft-PC".into());
                    let reply = serde_json::json!({
                        "app": "PlayCraft",
                        "pc_name": pc_name,
                        "ips": info.available_ips,
                        "ws_port": info.port,
                        "bt_spp": info.bluetooth_spp_active,
                        "bt_name": info.bluetooth_pc_name,
                        "vigem": info.vigem_active,
                    });
                    let _ = sock.send_to(reply.to_string().as_bytes(), src).await;
                }
            }
            Err(e) => {
                warn!("UDP discovery recv warning: {}", e);
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }
}

async fn read_http_head<S: AsyncRead + Unpin>(stream: &mut S) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while buf.len() < 8192 {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            break;
        }
        match tokio::time::timeout(rem, stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => break,
        }
    }
    Ok(buf)
}

async fn handle_tcp_connection<S>(
    mut stream: S,
    peer_addr: SocketAddr,
    app_handle: AppHandle,
    state: Arc<MobileServerState>,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let head_bytes = read_http_head(&mut stream).await?;
    if head_bytes.is_empty() {
        return Ok(());
    }

    let req_head = String::from_utf8_lossy(&head_bytes);
    let is_websocket = req_head
        .lines()
        .any(|l| l.to_ascii_lowercase().contains("upgrade: websocket"));

    if is_websocket {
        let replay = PrefetchStream {
            inner: stream,
            prefetched: head_bytes,
            offset: 0,
        };
        return handle_websocket_client(replay, peer_addr, app_handle, state).await;
    }

    // Plain HTTP info JSON (website controller UI intentionally removed; native APK connects via WS/BT).
    let info = state.build_pairing_info();
    let payload = serde_json::json!({
        "service": "PlayCraft Native Controller Bridge",
        "note": "Web browser controller removed. Open the PlayCraft Controller Android App.",
        "ws_port": info.port,
        "discovery_port": info.discovery_port,
        "bluetooth_spp": info.bluetooth_spp_active,
        "vigem": info.vigem_active,
    })
    .to_string();
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

// ================== SHARED INPUT DISPATCH ENGINE ==================

struct ClientSession {
    client_id: String,
    transport_label: String,
    remote_addr: String,
    authenticated: bool,
    registered_client: bool,
    held_keys: HashSet<String>,
    current_lt: f32,
    current_rt: f32,
}

impl ClientSession {
    fn new(client_id: String, transport_label: String, remote_addr: String) -> Self {
        Self {
            client_id,
            transport_label,
            remote_addr,
            authenticated: false,
            registered_client: false,
            held_keys: HashSet::new(),
            current_lt: 0.0,
            current_rt: 0.0,
        }
    }
}

fn default_button_keymap(button: &str) -> &'static str {
    match button.to_ascii_uppercase().as_str() {
        "CROSS" | "A" => "SPACE",
        "CIRCLE" | "B" => "C",
        "TRIANGLE" | "Y" => "R",
        "SQUARE" | "X" => "SHIFT",
        "L1" | "LB" => "Q",
        "R1" | "RB" => "E",
        "L2" | "LT" => "S",
        "R2" | "RT" => "W",
        "L3" | "LS" => "SHIFT",
        "R3" | "RS" => "V",
        "UP" => "W",
        "DOWN" => "S",
        "LEFT" => "A",
        "RIGHT" => "D",
        "OPTIONS" | "START" => "ESC",
        "SHARE" | "SELECT" | "BACK" => "TAB",
        "PS" | "XBOX" | "GUIDE" | "PS_HOME" => "ENTER",
        "HANDBRAKE" => "SPACE",
        _ => "",
    }
}

fn map_button_to_ui_nav(button: &str) -> Option<&'static str> {
    match button.to_ascii_uppercase().as_str() {
        "CROSS" | "A" => Some("CROSS"),
        "CIRCLE" | "B" => Some("CIRCLE"),
        "UP" => Some("UP"),
        "DOWN" => Some("DOWN"),
        "LEFT" => Some("LEFT"),
        "RIGHT" => Some("RIGHT"),
        "PS" | "XBOX" | "GUIDE" | "PS_HOME" => Some("PS_HOME"),
        _ => None,
    }
}

fn sync_held_key(held: &mut HashSet<String>, key: &str, want_down: bool) {
    if want_down {
        if held.insert(key.to_string()) {
            send_virtual_controller_button(key, true);
        }
    } else if held.remove(key) {
        send_virtual_controller_button(key, false);
    }
}

/// Processes one JSON controller packet identically across WebSocket and Bluetooth RFCOMM.
/// Optionally returns a JSON string reply to send back to the mobile app.
fn process_packet(
    packet: MobileControllerPacket,
    session: &mut ClientSession,
    app_handle: &AppHandle,
    state: &Arc<MobileServerState>,
) -> Option<String> {
    if packet.packet_type == "auth" {
        let expected_pin = state
            .pin_code
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default();
        let incoming_pin = packet.pin.as_deref().unwrap_or("").trim();
        if incoming_pin == expected_pin {
            session.authenticated = true;
            session.registered_client = true;
            let style = packet
                .controller_style
                .clone()
                .unwrap_or_else(|| "PlayCraft App".to_string());
            if let Ok(mut clients_guard) = state.clients.lock() {
                clients_guard.insert(
                    session.client_id.clone(),
                    ConnectedMobileClient {
                        id: session.client_id.clone(),
                        remote_addr: session.remote_addr.clone(),
                        user_agent: format!("{} • {}", session.transport_label, style),
                        connected_at: now_hms(),
                        last_input: format!("Paired ({})", session.transport_label),
                        latency_ms: 0,
                    },
                );
            }
            let _ = app_handle.emit("mobile-pairing-updated", state.build_pairing_info());
            let pc_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PlayCraft-PC".into());
            return Some(
                serde_json::json!({
                    "type": "auth_ok",
                    "vigem": is_virtual_x360_active(),
                    "pc_name": pc_name,
                    "transport": session.transport_label,
                })
                .to_string(),
            );
        } else {
            return Some(
                serde_json::json!({
                    "type": "auth_err",
                    "message": "Wrong PIN — enter the 4-digit PIN displayed on PlayCraft PC."
                })
                .to_string(),
            );
        }
    }

    if packet.packet_type == "ping" {
        let rtt = packet
            .client_ts
            .map(|ts| now_ms().saturating_sub(ts).min(9999) as u32)
            .unwrap_or(0);
        if let Ok(mut clients_guard) = state.clients.lock() {
            if let Some(c) = clients_guard.get_mut(&session.client_id) {
                c.latency_ms = rtt;
            }
        }
        return Some(
            serde_json::json!({
                "type": "pong",
                "vigem": is_virtual_x360_active(),
                "ts": packet.client_ts.unwrap_or(0),
            })
            .to_string(),
        );
    }

    if !session.authenticated {
        return None;
    }

    state.total_inputs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut clients_guard) = state.clients.lock() {
        if let Some(c) = clients_guard.get_mut(&session.client_id) {
            if let Some(m) = packet.controller_style.as_ref() {
                c.user_agent = format!("{} • {}", session.transport_label, m);
            }
            c.last_input = packet
                .control
                .clone()
                .unwrap_or_else(|| packet.packet_type.clone());
        }
    }

    let use_x360 = is_virtual_x360_active() && packet.input_mode.as_deref() != Some("keyboard");

    match packet.packet_type.as_str() {
        "button" | "dpad" | "menu" => {
            let btn = packet.control.clone().unwrap_or_default();
            let pressed = packet
                .state
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("down"))
                .unwrap_or(false);
            let upper = btn.to_ascii_uppercase();

            if pressed {
                if let Some(nav_cmd) = map_button_to_ui_nav(&upper) {
                    let _ = app_handle.emit(
                        "mobile-controller-input",
                        serde_json::json!({
                            "control": nav_cmd,
                            "state": "down",
                        }),
                    );
                }
            }

            if use_x360 {
                if upper == "L2" || upper == "LT" {
                    session.current_lt = if pressed { 1.0 } else { 0.0 };
                    send_xinput_triggers(Some(session.current_lt), Some(session.current_rt));
                } else if upper == "R2" || upper == "RT" {
                    session.current_rt = if pressed { 1.0 } else { 0.0 };
                    send_xinput_triggers(Some(session.current_lt), Some(session.current_rt));
                } else {
                    let xbtn = if upper == "HANDBRAKE" {
                        "A"
                    } else {
                        upper.as_str()
                    };
                    send_xinput_button(xbtn, pressed);
                }
            } else {
                let target_key = packet
                    .custom_key
                    .as_deref()
                    .filter(|k| !k.trim().is_empty())
                    .unwrap_or_else(|| default_button_keymap(&upper))
                    .to_ascii_uppercase();
                if !target_key.is_empty() {
                    sync_held_key(&mut session.held_keys, &target_key, pressed);
                }
            }
        }
        "trigger" => {
            if let Some(lt) = packet.lt {
                session.current_lt = lt.clamp(0.0, 1.0);
            }
            if let Some(rt) = packet.rt {
                session.current_rt = rt.clamp(0.0, 1.0);
            }
            if use_x360 {
                send_xinput_triggers(Some(session.current_lt), Some(session.current_rt));
            } else {
                sync_held_key(&mut session.held_keys, "S", session.current_lt >= 0.25);
                sync_held_key(&mut session.held_keys, "W", session.current_rt >= 0.25);
            }
        }
        "left_stick" => {
            let x = packet.lx.unwrap_or(0.0).clamp(-1.0, 1.0);
            let y = packet.ly.unwrap_or(0.0).clamp(-1.0, 1.0);
            if use_x360 {
                send_xinput_left_stick(x, -y);
            } else {
                sync_held_key(&mut session.held_keys, "A", x < -0.35);
                sync_held_key(&mut session.held_keys, "D", x > 0.35);
                sync_held_key(&mut session.held_keys, "W", y < -0.35);
                sync_held_key(&mut session.held_keys, "S", y > 0.35);
                let _ = translate_stick_axes_to_direction(x, y);
            }
        }
        "right_stick" => {
            let x = packet.rx.unwrap_or(0.0).clamp(-1.0, 1.0);
            let y = packet.ry.unwrap_or(0.0).clamp(-1.0, 1.0);
            if use_x360 {
                send_xinput_right_stick(x, -y);
            } else if x.abs() > 0.14 || y.abs() > 0.14 {
                send_virtual_mouse_move((x * 16.0) as i32, (y * 14.0) as i32);
            }
        }
        "gyro_steer" => {
            let steer_axis = packet
                .steer
                .or_else(|| packet.tilt_deg.map(|deg| (deg / 40.0).clamp(-1.0, 1.0)))
                .unwrap_or(0.0)
                .clamp(-1.0, 1.0);
            if use_x360 {
                send_xinput_left_stick(steer_axis, 0.0);
            } else {
                sync_held_key(&mut session.held_keys, "A", steer_axis < -0.14);
                sync_held_key(&mut session.held_keys, "D", steer_axis > 0.14);
            }
        }
        "mouse" => {
            let dx = packet.dx.unwrap_or(0);
            let dy = packet.dy.unwrap_or(0);
            if dx != 0 || dy != 0 {
                send_virtual_mouse_move(dx, dy);
            }
            if let Some(ctrl) = packet.control.as_deref() {
                if ctrl.eq_ignore_ascii_case("LEFT_CLICK") {
                    send_virtual_controller_button("MOUSE_LEFT", true);
                    send_virtual_controller_button("MOUSE_LEFT", false);
                } else if ctrl.eq_ignore_ascii_case("RIGHT_CLICK") {
                    send_virtual_controller_button("MOUSE_RIGHT", true);
                    send_virtual_controller_button("MOUSE_RIGHT", false);
                }
            }
        }
        _ => {}
    }

    None
}

fn cleanup_session(session: &mut ClientSession, app_handle: &AppHandle, state: &Arc<MobileServerState>) {
    for key in session.held_keys.drain() {
        send_virtual_controller_button(&key, false);
    }
    if session.registered_client {
        let remaining = state
            .clients
            .lock()
            .map(|mut g| {
                g.remove(&session.client_id);
                g.len()
            })
            .unwrap_or(0);
        if remaining == 0 {
            reset_virtual_x360_controller();
        }
        let _ = app_handle.emit("mobile-pairing-updated", state.build_pairing_info());
    }
}

async fn handle_websocket_client<S>(
    stream: S,
    peer_addr: SocketAddr,
    app_handle: AppHandle,
    state: Arc<MobileServerState>,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let transport = if peer_addr.ip().is_loopback() {
        "USB/Local".to_string()
    } else if peer_addr.ip().to_string().starts_with("192.168.43.")
        || peer_addr.ip().to_string().starts_with("192.168.137.")
    {
        "Hotspot".to_string()
    } else {
        "Wi-Fi / LAN".to_string()
    };

    let mut session = ClientSession::new(
        format!("ws-{}", peer_addr),
        transport,
        peer_addr.ip().to_string(),
    );
    let mut rumble_rx = subscribe_xinput_rumble();
    let mut last_activity = tokio::time::Instant::now();

    loop {
        let idle_deadline = last_activity + Duration::from_secs(15);
        tokio::select! {
            _ = tokio::time::sleep_until(idle_deadline) => {
                break;
            }
            rumble_res = rumble_rx.recv() => {
                if let Ok((large, small)) = rumble_res {
                    if session.authenticated {
                        let rumble_msg = serde_json::json!({
                            "type": "rumble",
                            "large": large,
                            "small": small,
                        })
                        .to_string();
                        if ws_tx.send(Message::Text(rumble_msg.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            next_msg = ws_rx.next() => {
                let msg = match next_msg {
                    Some(Ok(m)) => m,
                    Some(Err(_)) | None => break,
                };
                last_activity = tokio::time::Instant::now();

                match msg {
                    Message::Ping(payload) => {
                        let _ = ws_tx.send(Message::Pong(payload)).await;
                    }
                    Message::Close(_) => break,
                    Message::Text(text) => {
                        let Ok(packet) = serde_json::from_str::<MobileControllerPacket>(&text) else {
                            continue;
                        };
                        if let Some(reply) = process_packet(packet, &mut session, &app_handle, &state) {
                            if ws_tx.send(Message::Text(reply.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    cleanup_session(&mut session, &app_handle, &state);
    Ok(())
}

// ================== NATIVE WINDOWS BLUETOOTH CLASSIC RFCOMM SPP ==================

#[cfg(target_os = "windows")]
fn run_windows_bluetooth_spp_server(app_handle: AppHandle, state: Arc<MobileServerState>) {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Devices::Bluetooth::{AF_BTH, BTHPROTO_RFCOMM, NS_BTH, SOCKADDR_BTH};
    use windows_sys::Win32::Networking::WinSock::{
        accept, bind, closesocket, getsockname, listen, recv, send, setsockopt, socket,
        WSACleanup, WSASetServiceW, WSAStartup, CSADDR_INFO, INVALID_SOCKET,
        RNRSERVICE_REGISTER, SOCKADDR, SOCKET_ADDRESS, SOCKET_ERROR, SOCK_STREAM, SOL_SOCKET,
        SO_RCVTIMEO, WSADATA, WSAQUERYSETW,
    };

    unsafe {
        let mut wsa: WSADATA = zeroed();
        if WSAStartup(0x0202, &mut wsa) != 0 {
            warn!("WSAStartup failed; Bluetooth SPP server inactive");
            return;
        }

        let listen_sock = socket(AF_BTH as i32, SOCK_STREAM, BTHPROTO_RFCOMM as i32);
        if listen_sock == INVALID_SOCKET {
            info!("No local Bluetooth radio ready for RFCOMM SPP (Wi-Fi/Hotspot/USB still active).");
            WSACleanup();
            return;
        }

        let mut bind_addr: SOCKADDR_BTH = zeroed();
        bind_addr.addressFamily = AF_BTH;
        bind_addr.btAddr = 0;
        bind_addr.serviceClassId = zeroed();
        bind_addr.port = u32::MAX; // BT_PORT_ANY

        if bind(
            listen_sock,
            &bind_addr as *const SOCKADDR_BTH as *const SOCKADDR,
            size_of::<SOCKADDR_BTH>() as i32,
        ) == SOCKET_ERROR
        {
            info!("Bluetooth RFCOMM bind unavailable (Bluetooth may be turned off).");
            closesocket(listen_sock);
            WSACleanup();
            return;
        }

        let mut local_addr: SOCKADDR_BTH = zeroed();
        let mut addr_len = size_of::<SOCKADDR_BTH>() as i32;
        if getsockname(
            listen_sock,
            &mut local_addr as *mut SOCKADDR_BTH as *mut SOCKADDR,
            &mut addr_len,
        ) == SOCKET_ERROR
        {
            closesocket(listen_sock);
            WSACleanup();
            return;
        }

        let mut service_uuid = windows_sys::core::GUID::from_u128(PLAYCRAFT_BT_UUID_U128);
        let mut instance_name: Vec<u16> = "PlayCraft Controller SPP\0".encode_utf16().collect();
        let mut comment: Vec<u16> = "PlayCraft Low-Latency Gamepad & Gyro Bridge\0"
            .encode_utf16()
            .collect();

        let mut cs_addr = CSADDR_INFO {
            LocalAddr: SOCKET_ADDRESS {
                lpSockaddr: &mut local_addr as *mut SOCKADDR_BTH as *mut SOCKADDR,
                iSockaddrLength: size_of::<SOCKADDR_BTH>() as i32,
            },
            RemoteAddr: SOCKET_ADDRESS {
                lpSockaddr: std::ptr::null_mut(),
                iSockaddrLength: 0,
            },
            iSocketType: SOCK_STREAM,
            iProtocol: BTHPROTO_RFCOMM as i32,
        };

        let mut query_set: WSAQUERYSETW = zeroed();
        query_set.dwSize = size_of::<WSAQUERYSETW>() as u32;
        query_set.lpszServiceInstanceName = instance_name.as_mut_ptr();
        query_set.lpServiceClassId = &mut service_uuid;
        query_set.lpszComment = comment.as_mut_ptr();
        query_set.dwNameSpace = NS_BTH;
        query_set.dwNumberOfCsAddrs = 1;
        query_set.lpcsaBuffer = &mut cs_addr;

        let sdp_rc = WSASetServiceW(&query_set, RNRSERVICE_REGISTER, 0);
        if sdp_rc != 0 {
            warn!("Bluetooth SDP registration returned {}", sdp_rc);
        }

        if listen(listen_sock, 4) == SOCKET_ERROR {
            closesocket(listen_sock);
            WSACleanup();
            return;
        }

        let bound_channel = { local_addr.port };
        state.bt_spp_active.store(true, Ordering::Relaxed);
        info!(
            "PlayCraft Bluetooth Classic RFCOMM SPP listening on channel {}",
            bound_channel
        );
        let _ = app_handle.emit("mobile-pairing-updated", state.build_pairing_info());

        loop {
            let mut client_bth: SOCKADDR_BTH = zeroed();
            let mut client_len = size_of::<SOCKADDR_BTH>() as i32;
            let client_sock = accept(
                listen_sock,
                &mut client_bth as *mut SOCKADDR_BTH as *mut SOCKADDR,
                &mut client_len,
            );
            if client_sock == INVALID_SOCKET {
                std::thread::sleep(Duration::from_millis(120));
                continue;
            }

            let timeout_ms: u32 = 15000;
            let _ = setsockopt(
                client_sock,
                SOL_SOCKET,
                SO_RCVTIMEO,
                &timeout_ms as *const u32 as *const u8,
                size_of::<u32>() as i32,
            );

            let raw_bt_addr = { client_bth.btAddr };
            let bt_mac = format_bt_mac(raw_bt_addr);
            let app = app_handle.clone();
            let st = state.clone();

            std::thread::Builder::new()
                .name("playcraft-bt-client".to_string())
                .spawn(move || {
                    let mut session = ClientSession::new(
                        format!("bt-{}", bt_mac),
                        "Bluetooth SPP".to_string(),
                        bt_mac,
                    );
                    let bt_authed = Arc::new(AtomicBool::new(false));
                    let bt_stop = Arc::new(AtomicBool::new(false));
                    let send_guard = Arc::new(Mutex::new(()));

                    // Forward live XInput game rumble over Bluetooth SPP while client is alive
                    let rumble_auth = bt_authed.clone();
                    let rumble_stop = bt_stop.clone();
                    let rumble_guard = send_guard.clone();
                    let mut bt_rumble_rx = subscribe_xinput_rumble();
                    std::thread::Builder::new()
                        .name("playcraft-bt-rumble".to_string())
                        .spawn(move || {
                            while !rumble_stop.load(Ordering::Relaxed) {
                                match bt_rumble_rx.blocking_recv() {
                                    Ok((large, small)) => {
                                        if rumble_auth.load(Ordering::Relaxed) {
                                            let mut line = serde_json::json!({
                                                "type": "rumble",
                                                "large": large,
                                                "small": small,
                                            })
                                            .to_string();
                                            line.push('\n');
                                            if let Ok(_lk) = rumble_guard.lock() {
                                                let sent = send(
                                                    client_sock,
                                                    line.as_ptr(),
                                                    line.len() as i32,
                                                    0,
                                                );
                                                if sent <= 0 {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                    Err(_) => break,
                                }
                            }
                        })
                        .ok();

                    let mut acc = String::new();
                    let mut buf = [0u8; 2048];

                    loop {
                        let n = recv(client_sock, buf.as_mut_ptr(), buf.len() as i32, 0);
                        if n <= 0 {
                            break;
                        }
                        acc.push_str(&String::from_utf8_lossy(&buf[..n as usize]));
                        while let Some(pos) = acc.find('\n') {
                            let line = acc[..pos].trim().to_string();
                            acc.drain(..=pos);
                            if line.is_empty() {
                                continue;
                            }
                            let Ok(packet) = serde_json::from_str::<MobileControllerPacket>(&line)
                            else {
                                continue;
                            };
                            if let Some(mut reply) = process_packet(packet, &mut session, &app, &st)
                            {
                                bt_authed.store(session.authenticated, Ordering::Relaxed);
                                reply.push('\n');
                                let sent = if let Ok(_lk) = send_guard.lock() {
                                    send(client_sock, reply.as_ptr(), reply.len() as i32, 0)
                                } else {
                                    -1
                                };
                                if sent <= 0 {
                                    break;
                                }
                            }
                        }
                    }

                    bt_stop.store(true, Ordering::Relaxed);
                    closesocket(client_sock);
                    cleanup_session(&mut session, &app, &st);
                })
                .ok();
        }
    }
}

#[cfg(target_os = "windows")]
fn format_bt_mac(addr: u64) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        (addr >> 40) & 0xFF,
        (addr >> 32) & 0xFF,
        (addr >> 24) & 0xFF,
        (addr >> 16) & 0xFF,
        (addr >> 8) & 0xFF,
        addr & 0xFF
    )
}

/// Bridges an Android phone plugged in via USB Debugging using `adb reverse tcp:19550 tcp:19550`.
pub fn setup_adb_usb_bridge(ws_port: u16) -> Result<String, String> {
    let mut adb_cmd = std::process::Command::new("adb");
    let map_arg = format!("tcp:{}", ws_port);
    adb_cmd.args(["reverse", &map_arg, &map_arg]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        adb_cmd.creation_flags(0x08000000);
    }
    match adb_cmd.output() {
        Ok(out) if out.status.success() => Ok(format!(
            "USB ADB bridge active (tcp:{} <-> Phone 127.0.0.1:{}). Open USB mode in PlayCraft Controller App.",
            ws_port, ws_port
        )),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if err.is_empty() {
                "No USB-debugging Android phone detected. Plug USB cable or use USB Tethering instead.".to_string()
            } else {
                format!("ADB notice: {}", err)
            })
        }
        Err(_) => Err(
            "Android ADB CLI not found in PATH. Enable USB Tethering on your phone or connect over Wi-Fi / Hotspot / Bluetooth instead."
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_info_exposes_native_app_uri_and_ips() {
        let state = MobileServerState::new("192.168.1.29".to_string(), 19550, "7391".to_string());
        let info = state.build_pairing_info();
        assert!(info.pairing_url.starts_with("playcraft://pair?"));
        assert!(info.pairing_url.contains("pin=7391"));
        assert!(info.pairing_url.contains("discover=19552"));
        assert_eq!(info.ws_url, format!("ws://{}:19550/ws", info.ip_address));
        assert!(!info.available_ips.is_empty());
        assert!(info.qr_svg.starts_with("<svg"));
    }

    #[test]
    fn button_navigation_and_keymap_defaults_work() {
        assert_eq!(map_button_to_ui_nav("CROSS"), Some("CROSS"));
        assert_eq!(map_button_to_ui_nav("CIRCLE"), Some("CIRCLE"));
        assert_eq!(default_button_keymap("HANDBRAKE"), "SPACE");
        assert_eq!(default_button_keymap("R2"), "W");
    }
}
