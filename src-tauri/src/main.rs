// Audire entry point.
// Delegates to lib.rs which sets up the Tauri app.
//
// The windows_subsystem attribute below prevents a console window from
// being spawned alongside the GUI on Windows release builds. Debug builds
// keep the console so println!/eprintln! diagnostics remain visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    audire::run();
}
