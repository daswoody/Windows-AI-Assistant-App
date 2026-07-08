fn main() {
    // App-Manifest mit allen eigenen Commands: Tauri generiert daraus
    // allow-*-Permissions (Unterstriche werden zu Bindestrichen). Ohne
    // diese Deklaration sind eigene Commands fuer REMOTE-Origins - also
    // die vom Orchestrator geladene UI - grundsaetzlich gesperrt, egal
    // was die Capability sagt. Die Liste muss mit dem invoke_handler in
    // lib.rs uebereinstimmen; die Freigabe passiert in
    // capabilities/main.json (lokal) + commands::grant_remote_ipc (remote).
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "get_shell_info",
            "get_server_url",
            "set_server_url",
            "show_main_window",
            "set_indicator",
            "popup_card",
            "get_popup_payload",
            "pin_popup",
            "close_popup",
            "capture_screenshot",
            "set_hotkeys",
            "set_autostart",
            "set_wake_word",
        ])),
    )
    .expect("tauri-build fehlgeschlagen");
}
