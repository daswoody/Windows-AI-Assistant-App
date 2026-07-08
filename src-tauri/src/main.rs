// Windows-Subsystem: keine Konsole im Release-Build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    heimai_shell::run();
}
