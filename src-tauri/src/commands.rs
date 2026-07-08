//! IPC-Befehle der Shell (Vertrag zur Web-UI, versioniert ueber
//! SHELL_API_VERSION - die Server-UI deklariert in /app/version.json,
//! welche Version sie braucht; die Bootstrap-Seite prueft das vor der
//! Navigation).

use std::fs;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{hotkeys, popups, screenshot, wakeword, windows, AppState};

pub const SHELL_API_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct ShellInfo {
    pub shell_version: String,
    pub api_version: u32,
    pub platform: String,
}

#[tauri::command]
pub fn get_shell_info() -> ShellInfo {
    ShellInfo {
        shell_version: env!("CARGO_PKG_VERSION").to_string(),
        api_version: SHELL_API_VERSION,
        platform: std::env::consts::OS.to_string(),
    }
}

// ---- Server-URL (settings.json im App-Config-Verzeichnis) -------------------

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("settings.json"))
}

pub fn load_server_url(app: &AppHandle) -> Option<String> {
    let raw = fs::read_to_string(settings_path(app)?).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value["server_url"].as_str().map(str::to_string)
}

/// Gibt der vom Server geladenen UI (Remote-Origin!) Zugriff auf die
/// Shell-Befehle. Tauri blockt Remote-IPC komplett, bis eine Capability
/// mit passender Origin UND den Command-Permissions existiert - wir
/// registrieren sie zur Laufzeit mit der EXAKTEN Origin des verbundenen
/// Servers (kein Wildcard-Raten, minimale Angriffsflaeche).
pub fn grant_remote_ipc(app: &AppHandle, server_url: &str) {
    let Ok(parsed) = tauri::Url::parse(server_url) else {
        return;
    };
    let origin = parsed.origin().ascii_serialization();

    let state = app.state::<crate::AppState>();
    let mut granted = state.granted_origins.lock().unwrap();
    if !granted.insert(origin.clone()) {
        return; // schon registriert (Capability-IDs muessen eindeutig sein)
    }

    let mut capability = tauri::ipc::CapabilityBuilder::new(format!("remote-ui-{origin}"))
        .local(false)
        .remote(origin.clone())
        .windows(["main", "indicator", "popup-*"])
        .permission("core:default");
    for permission in [
        "allow-get-shell-info",
        "allow-get-server-url",
        "allow-set-server-url",
        "allow-show-main-window",
        "allow-set-indicator",
        "allow-popup-card",
        "allow-get-popup-payload",
        "allow-pin-popup",
        "allow-close-popup",
        "allow-capture-screenshot",
        "allow-set-hotkeys",
        "allow-set-autostart",
        "allow-set-wake-word",
    ] {
        capability = capability.permission(permission);
    }

    if let Err(error) = app.add_capability(capability) {
        eprintln!("Remote-IPC-Freigabe fuer {origin} fehlgeschlagen: {error}");
    }
}

#[tauri::command]
pub fn get_server_url(state: State<AppState>) -> Option<String> {
    state.server_url.lock().unwrap().clone()
}

/// Von der Bootstrap-Seite nach erfolgreichem Health-/Versions-Check
/// aufgerufen: URL persistieren und das Hauptfenster auf die zentrale
/// UI des Servers navigieren.
#[tauri::command]
pub fn set_server_url(app: AppHandle, state: State<AppState>, url: String) -> Result<(), String> {
    let url = url.trim_end_matches('/').to_string();
    *state.server_url.lock().unwrap() = Some(url.clone());

    if let Some(path) = settings_path(&app) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, serde_json::json!({ "server_url": url }).to_string());
    }

    // WICHTIG: Remote-IPC freigeben, BEVOR das Fenster auf die Server-UI
    // navigiert - sonst laufen deren invoke-Aufrufe (Hotkeys, Screenshot,
    // Indikator, ...) in die ACL-Ablehnung.
    grant_remote_ipc(&app, &url);
    windows::navigate_main_to_app(&app, &url)
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    windows::show_main(&app);
}

// ---- Indikator / Popups ------------------------------------------------------

#[tauri::command]
pub fn set_indicator(app: AppHandle, state: State<AppState>, indicator_state: String) {
    let server = state.server_url.lock().unwrap().clone();
    windows::set_indicator(&app, server.as_deref(), &indicator_state);
}

#[tauri::command]
pub fn popup_card(app: AppHandle, state: State<AppState>, payload: String) -> Result<String, String> {
    let server = state
        .server_url
        .lock()
        .unwrap()
        .clone()
        .ok_or("kein Server verbunden")?;
    popups::open(&app, &state, &server, payload)
}

#[tauri::command]
pub fn get_popup_payload(state: State<AppState>, id: String) -> Option<String> {
    state.popups.lock().unwrap().payload(&id)
}

#[tauri::command]
pub fn pin_popup(state: State<AppState>, id: String) {
    state.popups.lock().unwrap().pin(&id);
}

#[tauri::command]
pub fn close_popup(app: AppHandle, state: State<AppState>, id: String) {
    popups::close(&app, &state, &id);
}

// ---- Screenshot ---------------------------------------------------------------

#[derive(Serialize)]
pub struct Screenshot {
    pub b64: String,
    pub mime: String,
}

#[tauri::command]
pub async fn capture_screenshot() -> Result<Screenshot, String> {
    // Blocking-Arbeit (Capture + PNG-Encode) nicht auf dem Main-Thread.
    tauri::async_runtime::spawn_blocking(|| {
        screenshot::capture_primary().map(|b64| Screenshot { b64, mime: "image/png".into() })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- Hotkeys / Autostart / Wake Word -------------------------------------------

#[tauri::command]
pub fn set_hotkeys(app: AppHandle, hotkeys: hotkeys::HotkeyMap) -> Result<(), String> {
    hotkeys::apply(&app, &hotkeys)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled { manager.enable() } else { manager.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_wake_word(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
    threshold: f32,
) -> Result<(), String> {
    let mut slot = state.wakeword.lock().unwrap();
    // Bestehende Engine immer stoppen; bei enabled danach neu starten
    // (deckt auch Threshold-Aenderungen ab).
    if let Some(handle) = slot.take() {
        handle.stop();
    }
    if enabled {
        *slot = Some(wakeword::start(app, threshold)?);
    }
    Ok(())
}
