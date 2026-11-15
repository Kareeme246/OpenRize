// Hides the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = openrize_lib::run_llama_watchdog() {
        std::process::exit(code);
    }
    openrize_lib::run()
}
