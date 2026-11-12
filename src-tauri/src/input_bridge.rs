//! PlayCraft Windows Virtual Controller, ViGEm Xbox 360 Bridge, Custom Keybind Mapper & XInput Poller
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, OnceLock};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

static XINPUT_CONNECTED_COUNT: AtomicU32 = AtomicU32::new(0);
static VIGEM_X360_ACTIVE: AtomicBool = AtomicBool::new(false);
static RUMBLE_TX: OnceLock<broadcast::Sender<(u8, u8)>> = OnceLock::new();

fn rumble_broadcaster() -> &'static broadcast::Sender<(u8, u8)> {
    RUMBLE_TX.get_or_init(|| {
        let (tx, _rx) = broadcast::channel::<(u8, u8)>(32);
        tx
    })
}

pub fn subscribe_xinput_rumble() -> broadcast::Receiver<(u8, u8)> {
    rumble_broadcaster().subscribe()
}

pub fn is_vigem_bus_available() -> bool {
    Path::new("\\\\.\\ViGEmBus").exists()
        || Path::new("C:\\Windows\\System32\\drivers\\ViGEmBus.sys").exists()
}

pub fn is_virtual_x360_active() -> bool {
    VIGEM_X360_ACTIVE.load(Ordering::Relaxed)
}

pub fn virtual_controller_backend_label() -> String {
    if is_virtual_x360_active() {
        "ViGEmBus Real Xbox 360 XInput + Analog Steering Active".to_string()
    } else if is_vigem_bus_available() {
        "ViGEmBus Detected (Xbox 360 Virtual Controller Ready)".to_string()
    } else {
        "Win32 SendInput DirectInput ScanCode Bridge (Active)".to_string()
    }
}

pub fn get_connected_xinput_count() -> u32 {
    XINPUT_CONNECTED_COUNT.load(Ordering::Relaxed)
}

#[cfg(target_os = "windows")]
enum VigemMsg {
    EnsureConnected(mpsc::SyncSender<bool>),
    SetButton { control: String, pressed: bool },
    SetLeftStick { lx: f32, ly: f32 },
    SetRightStick { rx: f32, ry: f32 },
    SetTriggers { lt: Option<f32>, rt: Option<f32> },
    ResetNeutral,
    Shutdown(mpsc::SyncSender<()>),
}

#[cfg(target_os = "windows")]
static VIGEM_TX: OnceLock<mpsc::Sender<VigemMsg>> = OnceLock::new();

#[cfg(target_os = "windows")]
fn vigem_worker_tx() -> &'static mpsc::Sender<VigemMsg> {
    VIGEM_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<VigemMsg>();
        std::thread::spawn(move || {
            use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

            let mut target: Option<Xbox360Wired<Client>> = None;
            let mut pad = XGamepad::default();

            let try_connect = |tgt: &mut Option<Xbox360Wired<Client>>| -> bool {
                if let Some(ref t) = tgt {
                    if t.is_attached() {
                        VIGEM_X360_ACTIVE.store(true, Ordering::Relaxed);
                        return true;
                    }
                }
                match Client::connect() {
                    Ok(client) => {
                        let mut x360 = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
                        match x360.plugin() {
                            Ok(()) => {
                                let _ = x360.wait_ready();
                                let _ = x360.update(&XGamepad::default());
                                if let Ok(notif) = x360.request_notification() {
                                    let rumble_sender = rumble_broadcaster().clone();
                                    let _ = notif.spawn_thread(move |_req, data| {
                                        let _ = rumble_sender.send((data.large_motor, data.small_motor));
                                    });
                                }
                                info!("ViGEmBus Virtual Xbox 360 Controller plugged in with live force-feedback rumble.");
                                VIGEM_X360_ACTIVE.store(true, Ordering::Relaxed);
                                *tgt = Some(x360);
                                true
                            }
                            Err(e) => {
                                warn!("ViGEmBus plugin() failed: {:?}", e);
                                VIGEM_X360_ACTIVE.store(false, Ordering::Relaxed);
                                false
                            }
                        }
                    }
                    Err(e) => {
                        debug!("ViGEmBus Client::connect() unavailable: {:?}", e);
                        VIGEM_X360_ACTIVE.store(false, Ordering::Relaxed);
                        false
                    }
                }
            };

            while let Ok(msg) = rx.recv() {
                match msg {
                    VigemMsg::EnsureConnected(reply) => {
                        let ok = try_connect(&mut target);
                        let _ = reply.send(ok);
                    }
                    VigemMsg::SetButton { control, pressed } => {
                        if target.is_none() && !try_connect(&mut target) {
                            continue;
                        }
                        let upper = control.trim().to_uppercase();
                        let mask = match upper.as_str() {
                            "CROSS" | "A" => Some(XButtons::A),
                            "CIRCLE" | "B" => Some(XButtons::B),
                            "SQUARE" | "X" => Some(XButtons::X),
                            "TRIANGLE" | "Y" => Some(XButtons::Y),
                            "L1" | "LB" => Some(XButtons::LB),
                            "R1" | "RB" => Some(XButtons::RB),
                            "UP" | "DPAD_UP" => Some(XButtons::UP),
                            "DOWN" | "DPAD_DOWN" => Some(XButtons::DOWN),
                            "LEFT" | "DPAD_LEFT" => Some(XButtons::LEFT),
                            "RIGHT" | "DPAD_RIGHT" => Some(XButtons::RIGHT),
                            "START" | "OPTIONS" | "MENU" => Some(XButtons::START),
                            "SELECT" | "SHARE" | "VIEW" | "BACK" => Some(XButtons::BACK),
                            "L3" | "LTHUMB" => Some(XButtons::LTHUMB),
                            "R3" | "RTHUMB" => Some(XButtons::RTHUMB),
                            "GUIDE" => Some(XButtons::GUIDE),
                            _ => None,
                        };
                        if let Some(bit) = mask {
                            if pressed {
                                pad.buttons.raw |= bit;
                            } else {
                                pad.buttons.raw &= !bit;
                            }
                        } else if upper == "L2" || upper == "LT" {
                            pad.left_trigger = if pressed { 255 } else { 0 };
                        } else if upper == "R2" || upper == "RT" {
                            pad.right_trigger = if pressed { 255 } else { 0 };
                        }
                        if let Some(ref mut x360) = target {
                            let _ = x360.update(&pad);
                        }
                    }
                    VigemMsg::SetLeftStick { lx, ly } => {
                        if target.is_none() && !try_connect(&mut target) {
                            continue;
                        }
                        let cx = lx.clamp(-1.0, 1.0);
                        let cy = ly.clamp(-1.0, 1.0);
                        pad.thumb_lx = (cx * 32767.0).round() as i16;
                        pad.thumb_ly = (-cy * 32767.0).round() as i16;
                        if let Some(ref mut x360) = target {
                            let _ = x360.update(&pad);
                        }
                    }
                    VigemMsg::SetRightStick { rx, ry } => {
                        if target.is_none() && !try_connect(&mut target) {
                            continue;
                        }
                        let cx = rx.clamp(-1.0, 1.0);
                        let cy = ry.clamp(-1.0, 1.0);
                        pad.thumb_rx = (cx * 32767.0).round() as i16;
                        pad.thumb_ry = (-cy * 32767.0).round() as i16;
                        if let Some(ref mut x360) = target {
                            let _ = x360.update(&pad);
                        }
                    }
                    VigemMsg::SetTriggers { lt, rt } => {
                        if target.is_none() && !try_connect(&mut target) {
                            continue;
                        }
                        if let Some(v) = lt {
                            pad.left_trigger = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                        }
                        if let Some(v) = rt {
                            pad.right_trigger = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                        }
                        if let Some(ref mut x360) = target {
                            let _ = x360.update(&pad);
                        }
                    }
                    VigemMsg::ResetNeutral => {
                        pad = XGamepad::default();
                        if let Some(ref mut x360) = target {
                            let _ = x360.update(&pad);
                        }
                    }
                    VigemMsg::Shutdown(done) => {
                        pad = XGamepad::default();
                        if let Some(mut x360) = target.take() {
                            let _ = x360.update(&pad);
                            let _ = x360.unplug();
                            info!("ViGEmBus Virtual Xbox 360 Controller cleanly unplugged.");
                        }
                        VIGEM_X360_ACTIVE.store(false, Ordering::Relaxed);
                        let _ = done.send(());
                        break;
                    }
                }
            }
        });
        tx
    })
}

pub fn init_virtual_x360_controller() -> bool {
    #[cfg(target_os = "windows")]
    {
        if !is_vigem_bus_available() {
            return false;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        if vigem_worker_tx().send(VigemMsg::EnsureConnected(tx)).is_ok() {
            return rx
                .recv_timeout(std::time::Duration::from_millis(800))
                .unwrap_or(false);
        }
    }
    false
}

pub fn is_xinput_mappable_control(control: &str) -> bool {
    matches!(
        control.trim().to_uppercase().as_str(),
        "CROSS"
            | "A"
            | "CIRCLE"
            | "B"
            | "SQUARE"
            | "X"
            | "TRIANGLE"
            | "Y"
            | "L1"
            | "LB"
            | "R1"
            | "RB"
            | "L2"
            | "LT"
            | "R2"
            | "RT"
            | "UP"
            | "DPAD_UP"
            | "DOWN"
            | "DPAD_DOWN"
            | "LEFT"
            | "DPAD_LEFT"
            | "RIGHT"
            | "DPAD_RIGHT"
            | "START"
            | "OPTIONS"
            | "MENU"
            | "SELECT"
            | "SHARE"
            | "VIEW"
            | "BACK"
            | "L3"
            | "LTHUMB"
            | "R3"
            | "RTHUMB"
    )
}

pub fn send_xinput_button(control: &str, pressed: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        if !is_virtual_x360_active() && !init_virtual_x360_controller() {
            return false;
        }
        if !is_xinput_mappable_control(control) {
            return false;
        }
        let _ = vigem_worker_tx().send(VigemMsg::SetButton {
            control: control.to_string(),
            pressed,
        });
        return true;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (control, pressed);
        false
    }
}

pub fn send_xinput_left_stick(lx: f32, ly: f32) -> bool {
    #[cfg(target_os = "windows")]
    {
        if !is_virtual_x360_active() && !init_virtual_x360_controller() {
            return false;
        }
        let _ = vigem_worker_tx().send(VigemMsg::SetLeftStick { lx, ly });
        return true;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (lx, ly);
        false
    }
}

pub fn send_xinput_right_stick(rx: f32, ry: f32) -> bool {
    #[cfg(target_os = "windows")]
    {
        if !is_virtual_x360_active() && !init_virtual_x360_controller() {
            return false;
        }
        let _ = vigem_worker_tx().send(VigemMsg::SetRightStick { rx, ry });
        return true;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (rx, ry);
        false
    }
}

pub fn send_xinput_triggers(lt: Option<f32>, rt: Option<f32>) -> bool {
    #[cfg(target_os = "windows")]
    {
        if !is_virtual_x360_active() && !init_virtual_x360_controller() {
            return false;
        }
        let _ = vigem_worker_tx().send(VigemMsg::SetTriggers { lt, rt });
        return true;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (lt, rt);
        false
    }
}

pub fn reset_virtual_x360_controller() {
    #[cfg(target_os = "windows")]
    if let Some(tx) = VIGEM_TX.get() {
        let _ = tx.send(VigemMsg::ResetNeutral);
    }
}

pub fn shutdown_virtual_x360_controller() {
    #[cfg(target_os = "windows")]
    if let Some(tx) = VIGEM_TX.get() {
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        if tx.send(VigemMsg::Shutdown(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(std::time::Duration::from_millis(250));
        }
    }
}

fn map_control_to_vk(control: &str) -> Option<u16> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let upper = control.trim().to_uppercase();
    match upper.as_str() {
        "UP" | "DPAD_UP" | "ARROW_UP" => Some(VK_UP),
        "DOWN" | "DPAD_DOWN" | "ARROW_DOWN" => Some(VK_DOWN),
        "LEFT" | "DPAD_LEFT" | "ARROW_LEFT" => Some(VK_LEFT),
        "RIGHT" | "DPAD_RIGHT" | "ARROW_RIGHT" => Some(VK_RIGHT),
        "CROSS" | "A" | "ENTER" | "RETURN" => Some(VK_RETURN),
        "CIRCLE" | "B" | "ESCAPE" | "ESC" => Some(VK_ESCAPE),
        "SQUARE" | "X" => Some(b'X' as u16),
        "TRIANGLE" | "Y" => Some(b'Y' as u16),
        "L1" | "LB" | "KEY_Q" | "Q" => Some(b'Q' as u16),
        "R1" | "RB" | "KEY_E" | "E" => Some(b'E' as u16),
        "L2" | "LT" | "SHIFT" => Some(VK_SHIFT),
        "R2" | "RT" | "SPACE" => Some(VK_SPACE),
        "L3" | "LTHUMB" | "CTRL" | "CONTROL" => Some(VK_CONTROL),
        "R3" | "RTHUMB" => Some(b'V' as u16),
        "ALT" => Some(VK_MENU),
        "START" | "OPTIONS" | "MENU" | "TAB" => Some(VK_TAB),
        "SELECT" | "SHARE" | "VIEW" | "BACKSPACE" => Some(VK_BACK),
        "STICK_UP" | "KEY_W" | "W" => Some(b'W' as u16),
        "STICK_DOWN" | "KEY_S" | "S" => Some(b'S' as u16),
        "STICK_LEFT" | "KEY_A" | "A_KEY" => Some(b'A' as u16),
        "STICK_RIGHT" | "KEY_D" | "D" => Some(b'D' as u16),
        "KEY_R" | "R" => Some(b'R' as u16),
        "KEY_F" | "F" => Some(b'F' as u16),
        "KEY_C" | "C" => Some(b'C' as u16),
        "KEY_V" | "V" => Some(b'V' as u16),
        "KEY_G" | "G" => Some(b'G' as u16),
        "KEY_Z" | "Z" => Some(b'Z' as u16),
        "KEY_1" | "1" => Some(b'1' as u16),
        "KEY_2" | "2" => Some(b'2' as u16),
        "KEY_3" | "3" => Some(b'3' as u16),
        "KEY_4" | "4" => Some(b'4' as u16),
        "F1" => Some(VK_F1),
        "F2" => Some(VK_F2),
        "F3" => Some(VK_F3),
        "F4" => Some(VK_F4),
        "F5" => Some(VK_F5),
        _ => {
            if upper.len() == 1 {
                let b = upper.as_bytes()[0];
                if b.is_ascii_alphanumeric() {
                    return Some(b as u16);
                }
            }
            None
        }
    }
}

#[cfg(target_os = "windows")]
pub fn send_virtual_controller_button(control: &str, pressed: bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEINPUT, VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR,
        VK_RIGHT, VK_UP,
    };

    let upper = control.trim().to_uppercase();
    if upper == "MOUSE_LEFT" || upper == "MOUSE_RIGHT" {
        let flag = match (upper.as_str(), pressed) {
            ("MOUSE_LEFT", true) => MOUSEEVENTF_LEFTDOWN,
            ("MOUSE_LEFT", false) => MOUSEEVENTF_LEFTUP,
            ("MOUSE_RIGHT", true) => MOUSEEVENTF_RIGHTDOWN,
            _ => MOUSEEVENTF_RIGHTUP,
        };
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flag,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            let _ = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
        }
        return;
    }

    let Some(vk) = map_control_to_vk(&upper) else {
        return;
    };

    unsafe {
        let scan_code = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16;
        let mut flags = if scan_code != 0 {
            KEYEVENTF_SCANCODE
        } else {
            0
        };
        if matches!(
            vk,
            VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_INSERT | VK_DELETE
        ) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !pressed {
            flags |= KEYEVENTF_KEYUP;
        }

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let sent = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
        debug!(
            "Injected Win32 Virtual Controller Input: control={} vk=0x{:X} scan=0x{:X} pressed={} sent={}",
            control, vk, scan_code, pressed, sent
        );
    }
}

#[cfg(target_os = "windows")]
pub fn send_virtual_mouse_move(dx: i32, dy: i32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    if dx == 0 && dy == 0 {
        return;
    }
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn translate_stick_axes_to_direction(lx: f32, ly: f32) -> Option<&'static str> {
    let deadzone = 0.42;
    if ly < -deadzone && ly.abs() >= lx.abs() {
        Some("UP")
    } else if ly > deadzone && ly.abs() >= lx.abs() {
        Some("DOWN")
    } else if lx < -deadzone && lx.abs() > ly.abs() {
        Some("LEFT")
    } else if lx > deadzone && lx.abs() > ly.abs() {
        Some("RIGHT")
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
pub fn spawn_xinput_poller(app_handle: tauri::AppHandle) {
    use tauri::Emitter;
    use windows_sys::Win32::UI::Input::XboxController::{
        XInputGetState, XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK,
        XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT,
        XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_LEFT_SHOULDER, XINPUT_GAMEPAD_RIGHT_SHOULDER,
        XINPUT_GAMEPAD_START, XINPUT_GAMEPAD_X, XINPUT_GAMEPAD_Y, XINPUT_STATE,
    };

    tauri::async_runtime::spawn(async move {
        let mut prev_buttons = [0u16; 4];
        loop {
            let mut connected = 0u32;
            for idx in 0..4u32 {
                let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
                let res = unsafe { XInputGetState(idx, &mut state) };
                if res == 0 {
                    connected += 1;
                    let btns = state.Gamepad.wButtons;
                    let newly_pressed = btns & !prev_buttons[idx as usize];
                    prev_buttons[idx as usize] = btns;

                    let combo_overlay = (btns & XINPUT_GAMEPAD_BACK != 0)
                        && (btns & XINPUT_GAMEPAD_START != 0)
                        && (newly_pressed != 0);
                    let guide_pressed = (newly_pressed & 0x0400) != 0;

                    if combo_overlay || guide_pressed {
                        let _ = app_handle.emit("playcraft-overlay-toggle", "xinput-guide");
                        continue;
                    }

                    let emit_btn = |name: &str| {
                        let _ = app_handle.emit(
                            "mobile-controller-input",
                            serde_json::json!({
                                "control": name,
                                "state": "down",
                                "source": format!("XInput Pad #{}", idx + 1)
                            }),
                        );
                    };

                    if newly_pressed & XINPUT_GAMEPAD_DPAD_LEFT != 0 {
                        emit_btn("LEFT");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_DPAD_RIGHT != 0 {
                        emit_btn("RIGHT");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_DPAD_UP != 0 {
                        emit_btn("UP");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_DPAD_DOWN != 0 {
                        emit_btn("DOWN");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_A != 0 {
                        emit_btn("CROSS");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_B != 0 {
                        emit_btn("CIRCLE");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_X != 0 {
                        emit_btn("SQUARE");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_Y != 0 {
                        emit_btn("TRIANGLE");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_LEFT_SHOULDER != 0 {
                        emit_btn("L1");
                    }
                    if newly_pressed & XINPUT_GAMEPAD_RIGHT_SHOULDER != 0 {
                        emit_btn("R1");
                    }
                } else {
                    prev_buttons[idx as usize] = 0;
                }
            }
            XINPUT_CONNECTED_COUNT.store(connected, Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(32)).await;
        }
    });
}
