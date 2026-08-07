// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Velopack hooks the install and update lifecycle by re-invoking this
    // executable with its own flags — first run after install, first run after
    // update, uninstall. `run()` handles those and *terminates the process*
    // when it has, so anything above this line executes during an install and
    // then dies half-finished.
    //
    // That is why it is here rather than inside `lunarlauncher_lib::run`,
    // which already reads a .env and installs a log subscriber before Tauri
    // starts. Those would run during an install hook and write to disk under a
    // process that is about to exit.
    #[cfg(target_os = "windows")]
    velopack::VelopackApp::build().run();

    lunarlauncher_lib::run()
}
