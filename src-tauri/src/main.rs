// Launch the desktop app without a console in debug and release builds.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    tauri_app_lib::run()
}
