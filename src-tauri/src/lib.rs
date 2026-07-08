//! Heim-AI Windows-Shell (Mikro-Phase 2.5).
//!
//! Architektur "voll zentral": Diese Shell ist ein duenner nativer Rahmen.
//! Die komplette User-UI kommt live vom Orchestrator (https://<server>/app),
//! die Shell liefert nur, was ein Browser nicht kann:
//! - System-Tray + Close-to-Tray + Autostart
//! - globale Hotkeys (Chat, Realtime Voice, Screenshot)
//! - Topmost-Fenster: schwebender Voice-Indikator, anpinnbare Antwort-Popups
//! - Desktop-Screenshots (fuer die Bild-Analyse der AI)
//! - Wake Word (openWakeWord/ONNX, lokal)
//!
//! Der Vertrag zur UI (Befehle + Events) ist in
//! docs/protocol-additions-2.5.md des Orchestrator-Repos beschrieben und
//! ueber shell_api_version (commands::SHELL_API_VERSION) versioniert.

mod commands;
mod hotkeys;
mod popups;
mod screenshot;
mod wakeword;
mod windows;

use std::collections::HashSet;
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

/// Gemeinsamer Zustand der Shell.
pub struct AppState {
    /// Basis-URL des Orchestrators (persistiert in settings.json).
    pub server_url: Mutex<Option<String>>,
    /// Payloads offener Antwort-Popups (id -> JSON).
    pub popups: Mutex<popups::PopupStore>,
    /// Laufende Wake-Word-Engine (Stop-Flag), None = aus.
    pub wakeword: Mutex<Option<wakeword::WakeWordHandle>>,
    /// Origins, die bereits eine Runtime-Capability fuer Remote-IPC haben.
    pub granted_origins: Mutex<HashSet<String>>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            server_url: Mutex::new(None),
            popups: Mutex::new(popups::PopupStore::default()),
            wakeword: Mutex::new(None),
            granted_origins: Mutex::new(HashSet::new()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_shell_info,
            commands::get_server_url,
            commands::set_server_url,
            commands::show_main_window,
            commands::set_indicator,
            commands::popup_card,
            commands::get_popup_payload,
            commands::pin_popup,
            commands::close_popup,
            commands::capture_screenshot,
            commands::set_hotkeys,
            commands::set_autostart,
            commands::set_wake_word,
        ])
        .setup(|app| {
            // Gespeicherte Server-URL laden (die Bootstrap-Seite fragt sie
            // per get_server_url ab und verbindet automatisch) und die
            // Remote-IPC-Freigabe fuer diese Origin direkt registrieren.
            let saved_url = commands::load_server_url(app.handle());
            let state = app.state::<AppState>();
            *state.server_url.lock().unwrap() = saved_url.clone();
            if let Some(url) = saved_url {
                commands::grant_remote_ipc(app.handle(), &url);
            }

            setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Hauptfenster: Schliessen = in den Tray (App laeuft weiter,
            // Antworten erscheinen dann als Popups ueber allen Anwendungen).
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht starten");
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Heim-AI öffnen", true, None::<&str>)?;
    let talk = MenuItem::with_id(app, "talk", "Realtime Voice starten/stoppen", true, None::<&str>)?;
    let screenshot = MenuItem::with_id(app, "screenshot", "Screenshot an AI senden", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Beenden", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &talk, &screenshot, &quit])?;

    TrayIconBuilder::with_id("heimai-tray")
        .icon(app.default_window_icon().expect("App-Icon fehlt").clone())
        .tooltip("Heim-AI")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => windows::show_main(app),
            // Tray-Aktionen laufen ueber dieselben Events wie die Hotkeys -
            // die UI (im versteckten Hauptfenster) fuehrt sie aus.
            "talk" => hotkeys::emit_action(app, "talk"),
            "screenshot" => hotkeys::emit_action(app, "screenshot"),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                windows::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
