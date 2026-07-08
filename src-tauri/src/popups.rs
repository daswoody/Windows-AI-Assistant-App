//! Antwort-Popups "ueber allen Anwendungen", anpinnbar (Anforderung 2.5).
//!
//! Bewusst KEINE Windows-Toasts: die verschwinden ins Action Center und
//! sind nicht anpinnbar. Stattdessen rahmenlose Topmost-Fenster oben
//! rechts, die die zentrale Web-UI unter #/popup?id=<n> laden - dort
//! rendert derselbe Karten-Renderer wie im Chat (4.12), inklusive
//! Pin-Knopf und Auto-Close-Timer.

use std::collections::HashMap;

use tauri::{AppHandle, LogicalPosition, Manager, State, WebviewUrl};

use crate::AppState;

const POPUP_WIDTH: f64 = 380.0;
const POPUP_HEIGHT: f64 = 240.0;
const POPUP_GAP: f64 = 12.0;

#[derive(Default)]
pub struct PopupStore {
    next_id: u64,
    payloads: HashMap<String, String>,
    pinned: HashMap<String, bool>,
    /// Belegte Stapel-Plaetze (0 = oben) -> Popup-ID.
    slots: HashMap<usize, String>,
}

impl PopupStore {
    pub fn payload(&self, id: &str) -> Option<String> {
        self.payloads.get(id).cloned()
    }

    pub fn pin(&mut self, id: &str) {
        self.pinned.insert(id.to_string(), true);
    }

    fn allocate(&mut self, payload: String) -> (String, usize) {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let slot = (0..).find(|slot| !self.slots.contains_key(slot)).unwrap();
        self.slots.insert(slot, id.clone());
        self.payloads.insert(id.clone(), payload);
        (id, slot)
    }

    fn release(&mut self, id: &str) {
        self.payloads.remove(id);
        self.pinned.remove(id);
        self.slots.retain(|_, occupant| occupant != id);
    }
}

pub fn open(
    app: &AppHandle,
    state: &State<AppState>,
    server_url: &str,
    payload: String,
) -> Result<String, String> {
    let (id, slot) = state.popups.lock().unwrap().allocate(payload);

    let url: tauri::Url = format!("{server_url}/app/#/popup?id={id}")
        .parse()
        .map_err(|_| "ungueltige URL")?;

    let window =
        tauri::WebviewWindowBuilder::new(app, format!("popup-{id}"), WebviewUrl::External(url))
            .title("Heim-AI")
            .inner_size(POPUP_WIDTH, POPUP_HEIGHT)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .build()
            .map_err(|e| e.to_string())?;

    // Oben rechts stapeln (aeltere Popups rutschen nicht nach - simpel v1).
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_width = monitor.size().width as f64 / scale;
        let x = screen_width - POPUP_WIDTH - 16.0;
        let y = 16.0 + slot as f64 * (POPUP_HEIGHT + POPUP_GAP);
        let _ = window.set_position(LogicalPosition::new(x, y));
    }

    Ok(id)
}

pub fn close(app: &AppHandle, state: &State<AppState>, id: &str) {
    if let Some(window) = app.get_webview_window(&format!("popup-{id}")) {
        let _ = window.close();
    }
    state.popups.lock().unwrap().release(id);
}
