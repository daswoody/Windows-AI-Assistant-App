//! Fenster-Verwaltung: Hauptfenster-Navigation auf die Server-UI und der
//! schwebende Voice-Indikator (rahmenloses Topmost-Fenster oben mittig,
//! Anforderung 2.5). Der Indikator rendert dieselbe Web-UI unter
//! #/indicator - es gibt genau EINEN UI-Codebestand.

use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl};

const INDICATOR_LABEL: &str = "indicator";
const INDICATOR_WIDTH: f64 = 260.0;
const INDICATOR_HEIGHT: f64 = 52.0;

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hauptfenster auf die zentrale UI des Orchestrators navigieren.
pub fn navigate_main_to_app(app: &AppHandle, server_url: &str) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("Hauptfenster fehlt")?;
    let url: tauri::Url = format!("{server_url}/app/").parse().map_err(|_| "ungueltige URL")?;
    window.navigate(url).map_err(|e| e.to_string())
}

/// Indikator anzeigen/verstecken. Zustaende: hidden|listening|thinking|speaking.
pub fn set_indicator(app: &AppHandle, server_url: Option<&str>, state: &str) {
    if state == "hidden" {
        if let Some(window) = app.get_webview_window(INDICATOR_LABEL) {
            let _ = window.hide();
        }
        return;
    }

    let window = match app.get_webview_window(INDICATOR_LABEL) {
        Some(window) => window,
        None => {
            let Some(server) = server_url else { return };
            match build_indicator(app, server) {
                Ok(window) => window,
                Err(error) => {
                    eprintln!("Indikator-Fenster fehlgeschlagen: {error}");
                    return;
                }
            }
        }
    };

    let _ = window.show();
    // Zustand an die Indikator-UI weiterreichen (Event-Vertrag, siehe
    // protocol-additions-2.5.md Abschnitt 5).
    let _ = app.emit_to(INDICATOR_LABEL, "indicator-state", state.to_string());
}

fn build_indicator(app: &AppHandle, server_url: &str) -> Result<tauri::WebviewWindow, String> {
    let url: tauri::Url = format!("{server_url}/app/#/indicator")
        .parse()
        .map_err(|_| "ungueltige URL")?;

    let window = tauri::WebviewWindowBuilder::new(app, INDICATOR_LABEL, WebviewUrl::External(url))
        .title("Heim-AI Indikator")
        .inner_size(INDICATOR_WIDTH, INDICATOR_HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;

    // Oben mittig am Bildschirmrand positionieren.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_width = monitor.size().width as f64 / scale;
        let x = (screen_width - INDICATOR_WIDTH) / 2.0;
        let _ = window.set_position(LogicalPosition::new(x, 8.0));
        let _ = window.set_size(LogicalSize::new(INDICATOR_WIDTH, INDICATOR_HEIGHT));
    }
    Ok(window)
}
