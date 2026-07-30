use std::path::PathBuf;

fn config_dir() -> PathBuf {
    std::env::var_os("GRID_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(grid::NodeConfig::default_dir)
}

#[tauri::command]
async fn wallet_snapshot() -> grid::gui::WalletSnapshot {
    grid::gui::snapshot(&config_dir()).await
}

#[tauri::command]
async fn wallet_action(
    action: serde_json::Value,
) -> Result<grid::gui::ActionResult, String> {
    let action: grid::gui::WalletAction =
        serde_json::from_value(action).map_err(|error| error.to_string())?;
    grid::gui::act(&config_dir(), action)
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![wallet_snapshot, wallet_action])
        .run(tauri::generate_context!())
        .expect("Phoenix GRID Wallet failed to start");
}
