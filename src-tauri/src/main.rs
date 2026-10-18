// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before anything else: it sets env vars, which isn't safe once threads exist.
    boxdeck_core::vagrant::locate::fix_path();
    boxdeck_lib::run()
}
