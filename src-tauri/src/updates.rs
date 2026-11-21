use parking_lot::Mutex;
use std::time::Duration;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<String>, String> {
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let version = update.as_ref().map(|u| u.version.clone());
    *pending.0.lock() = update;
    Ok(version)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let update = pending.0.lock().take().ok_or("No pending update")?;
    if let Err(err) = update.download_and_install(|_, _| {}, || {}).await {
        *pending.0.lock() = Some(update);
        return Err(err.to_string());
    }
    app.restart();
}
