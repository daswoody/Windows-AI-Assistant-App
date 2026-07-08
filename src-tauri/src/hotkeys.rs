//! Globale Hotkeys (Anforderung 2.5): Chat oeffnen, Realtime Voice
//! starten/stoppen, Screenshot an die AI senden. Die Aktionen gehen als
//! "hotkey"-Event an die (ggf. versteckte) Haupt-UI, die sie ausfuehrt -
//! nur "chat" holt zusaetzlich das Fenster nach vorn (das muss auch ohne
//! reagierende UI funktionieren).

use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::windows;

#[derive(Deserialize, Clone)]
pub struct HotkeyMap {
    pub chat: String,
    pub talk: String,
    pub screenshot: String,
}

pub fn emit_action(app: &AppHandle, action: &str) {
    let _ = app.emit_to("main", "hotkey", serde_json::json!({ "action": action }));
}

pub fn apply(app: &AppHandle, hotkeys: &HotkeyMap) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;

    for (accelerator, action) in [
        (&hotkeys.chat, "chat"),
        (&hotkeys.talk, "talk"),
        (&hotkeys.screenshot, "screenshot"),
    ] {
        if accelerator.trim().is_empty() {
            continue;
        }
        let action = action.to_string();
        shortcuts
            .on_shortcut(accelerator.as_str(), move |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    if action == "chat" {
                        windows::show_main(app);
                    }
                    emit_action(app, &action);
                }
            })
            .map_err(|e| format!("Hotkey '{accelerator}' ungueltig: {e}"))?;
    }
    Ok(())
}
