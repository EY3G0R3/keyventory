pub mod cli;
mod scanner;

#[tauri::command]
fn scan_bindings() -> scanner::ScanResult {
    scanner::scan_all()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![scan_bindings])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
