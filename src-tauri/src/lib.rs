// use std::fs::File; // Removed unused import
// use std::io::Read; // Removed unused import

mod audio_analysis; // Declare the module
mod bpm_analyzer;   // <-- Add this line

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet, 
            audio_analysis::process_audio_file,
            bpm_analyzer::analyze_bpm_for_file,
            audio_analysis::analyze_volume_batch
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
