use parking_lot::Mutex;
use serde_json::json;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// Progress is reported to the window as `update:progress` events:
/// `{stage: "download", done, total}`, then `{stage: "install"}` and `{stage: "restart"}`.
fn report(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit_to("main", "update:progress", payload);
}

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
    let mut done: u64 = 0;
    let mut last = Instant::now() - Duration::from_secs(1);
    let (progress, finished) = (app.clone(), app.clone());
    report(&app, json!({ "stage": "download", "done": 0, "total": null }));
    let result = update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                if last.elapsed() >= Duration::from_millis(120) || total == Some(done) {
                    last = Instant::now();
                    report(&progress, json!({ "stage": "download", "done": done, "total": total }));
                }
            },
            move || report(&finished, json!({ "stage": "install" })),
        )
        .await;
    if let Err(err) = result {
        *pending.0.lock() = Some(update);
        return Err(err.to_string());
    }
    report(&app, json!({ "stage": "restart" }));
    app.restart();
}
