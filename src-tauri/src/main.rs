//! GuitarCraft — Rust core in a Tauri window. Audio, DSP, files, MIDI and the control API are native;
//! the HTML/CSS interface talks to it through the commands below.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod backgrounds;
mod catalog;
mod control;
mod engine;
mod files;
mod midi;
mod updates;

use engine::{AudioConfig, Engine};
use files::Dirs;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

struct AppState { engine: Arc<Engine>, dirs: Dirs, bridge: Arc<control::Bridge> }
type S<'a> = State<'a, AppState>;
type R<T> = Result<T, String>;

// ---------------- files ----------------
#[tauri::command] fn fs_dirs(s: S) -> Value { s.dirs.json() }
#[tauri::command] fn presets_list(s: S) -> Value { files::list_presets(&s.dirs) }
#[tauri::command] fn presets_save(s: S, preset: Value, bank: Option<String>) -> R<Value> { files::save_preset(&s.dirs, &preset, bank.as_deref()) }
#[tauri::command] fn presets_delete(s: S, file: String) -> R<bool> { files::delete_preset(&s.dirs, &file) }
#[tauri::command] fn settings_load(s: S) -> Value { files::load_settings(&s.dirs) }
#[tauri::command] fn settings_save(s: S, data: Value) -> R<bool> { files::save_settings(&s.dirs, data) }
#[tauri::command] fn read_text(file: String) -> R<String> { std::fs::read_to_string(file).map_err(|e| e.to_string()) }
#[tauri::command] fn import_to(s: S, file: String, kind: String) -> R<String> { files::import_to(&s.dirs, &file, &kind) }
#[tauri::command]
fn backgrounds_list(s: S) -> Vec<backgrounds::Background> {
    backgrounds::list(&s.dirs.backgrounds)
}
#[tauri::command]
async fn background_import(s: S<'_>, file: String) -> R<backgrounds::Background> {
    let dir = s.dirs.backgrounds.clone();
    tauri::async_runtime::spawn_blocking(move || backgrounds::import(&dir, std::path::Path::new(&file)))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command] fn list_files(s: S, kind: String, ext: Vec<String>) -> Vec<String> {
    let Some(d) = s.dirs.kind(&kind) else { return vec![] }; let e: Vec<&str> = ext.iter().map(|x| x.as_str()).collect();
    files::walk(d, &e).into_iter().map(|p| p.to_string_lossy().to_string()).collect()
}
#[tauri::command] fn write_json(s: S, kind: String, name: String, data: Value) -> R<String> {
    let d = s.dirs.kind(&kind).ok_or("bad kind")?; let f = d.join(files::safe(&name));
    std::fs::write(&f, serde_json::to_string_pretty(&data).unwrap()).map_err(|e| e.to_string())?; Ok(f.to_string_lossy().to_string())
}
#[tauri::command]
fn reveal(file: String) -> R<()> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg("-R").arg(file).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe").arg(format!("/select,{file}")).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(std::path::Path::new(&file).parent().unwrap_or(std::path::Path::new("."))).spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}
#[tauri::command]
fn clipboard_write(text: String) -> R<()> {
    arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)).map_err(|e| e.to_string())
}
#[tauri::command]
fn catalog_discover(s: S, source: String) -> R<Vec<Value>> {
    catalog::discover(&s.dirs, &source)
}
#[tauri::command] fn plugins_scan(s: S) -> Value { files::scan_plugins(&s.dirs) }
#[tauri::command] async fn file_open(app: tauri::AppHandle, title: String, filters: Vec<Value>) -> Option<String> {
    let mut d = app.dialog().file().set_title(title);
    for f in &filters {
        let name = f.get("name").and_then(|x| x.as_str()).unwrap_or("Files").to_string();
        let ext: Vec<String> = f.get("extensions").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect()).unwrap_or_default();
        let refs: Vec<&str> = ext.iter().map(|x| x.as_str()).collect(); d = d.add_filter(name, &refs);
    }
    d.blocking_pick_file().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().to_string())
}
#[tauri::command] fn app_relaunch(app: tauri::AppHandle) { app.restart(); }
#[tauri::command] fn app_info(s: S) -> Value {
    let name = format!("guitarcraft-mcp{}", std::env::consts::EXE_SUFFIX);
    let sibling = std::env::current_exe().unwrap_or_default().with_file_name(&name);
    let mcp = if sibling.exists() { sibling } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/release").join(name)
    };
    json!({ "control": s.dirs.control_file(), "mcpServer": mcp, "version": env!("CARGO_PKG_VERSION"), "engine": "rust" })
}
#[tauri::command] fn rpc_response(s: S, id: u64, ok: bool, result: Option<Value>, error: Option<String>) {
    s.bridge.respond(id, if ok { Ok(result.unwrap_or(Value::Null)) } else { Err(error.unwrap_or_default()) });
}

// ---------------- engine ----------------
#[tauri::command] async fn engine_configure(s: S<'_>, cfg: AudioConfig) -> R<Value> { let e = s.engine.clone(); tauri::async_runtime::spawn_blocking(move || e.configure(cfg).map(|i| json!(i))).await.map_err(|e| e.to_string())? }
#[tauri::command] fn engine_devices() -> Value { engine::list_devices() }
#[tauri::command] fn engine_set_rig(s: S, rig: Value) { s.engine.set_rig(rig); }
#[tauri::command] fn engine_user_pedals(s: S, defs: Vec<Value>) { s.engine.set_user_pedals(defs); }
#[tauri::command] fn engine_mute(s: S, on: bool) { s.engine.mute(on); }
#[tauri::command] fn engine_meters(s: S) -> Value { s.engine.meters() }
#[tauri::command] fn engine_spectrum(s: S, n: Option<usize>) -> Vec<f32> { s.engine.spectrum(n.unwrap_or(8192).next_power_of_two().min(32768)) }
#[tauri::command] async fn engine_analyze(s: S<'_>, seconds: f32) -> R<Value> { let e = s.engine.clone(); tauri::async_runtime::spawn_blocking(move || e.analyze(seconds)).await.map_err(|e| e.to_string()) }
#[tauri::command] fn engine_tuner(s: S, a4: Option<f32>) -> Value { s.engine.tuner(a4.unwrap_or(440.0)) }
#[tauri::command] fn engine_status(s: S) -> Value {
    let e = &s.engine; let i = e.info.lock().clone(); use std::sync::atomic::Ordering::Relaxed;
    let out_ms = e.shared.out_latency_us.load(Relaxed) as f32 / 1000.0;
    let capture_ms = e.shared.in_latency_us.load(Relaxed) as f32 / 1000.0;
    let queue_ms = e.shared.queued_frames.load(Relaxed) as f32 / i.sample_rate.max(1) as f32 * 1000.0;
    json!({ "engine": "rust", "sr": i.sample_rate, "buffer": i.buffer,
        "base_ms": capture_ms + queue_ms, "capture_ms": capture_ms, "queue_ms": queue_ms, "out_ms": out_ms,
        "output": i.output, "input": i.input, "outChannels": i.out_channels, "inChannels": i.in_channels, "channel": i.channel, "sampleRates": i.sample_rates,
        "live": e.shared.live.load(Relaxed), "di": *e.di_name.lock(), "diPlaying": e.shared.di_playing.load(Relaxed), "meters": e.meters() })
}
#[tauri::command] fn engine_di_bytes(s: S, request: tauri::ipc::Request) -> R<Value> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("expected raw bytes".into()) };
    let name = request.headers().get("x-name").and_then(|h| h.to_str().ok()).map(|x| percent_decode(x)).unwrap_or_else(|| "DI".into());
    let (d, sr) = engine::decode_bytes(bytes.clone(), Some("wav".into()))?;
    s.engine.load_di(d, sr, &name); Ok(json!({ "name": name, "sr": sr }))
}
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes(); let mut out = vec![]; let mut i = 0;
    while i < b.len() { if b[i] == b'%' && i + 2 < b.len() { if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; } } out.push(b[i]); i += 1; }
    String::from_utf8_lossy(&out).to_string()
}
#[tauri::command] fn engine_di_path(s: S, path: String) -> R<Value> {
    let (d, sr) = engine::decode_file(&path)?; let name = std::path::Path::new(&path).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    s.engine.load_di(d, sr, &name); Ok(json!({ "name": name }))
}
#[tauri::command] fn engine_di_play(s: S, on: bool) { use std::sync::atomic::Ordering::Relaxed; if on && !s.engine.shared.di_playing.load(Relaxed) { s.engine.shared.di_restart.store(true, Relaxed); } s.engine.shared.di_playing.store(on, Relaxed); }
#[tauri::command] fn engine_test_tone(s: S) { s.engine.shared.test_tone.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
#[tauri::command] fn engine_metronome(s: S, on: bool, bpm: f32, level: f32) {
    use std::sync::atomic::Ordering::Relaxed; let sh = &s.engine.shared;
    sh.bpm.store(bpm.to_bits(), Relaxed); sh.metro_level.store(level.to_bits(), Relaxed); sh.metro_on.store(on, Relaxed);
}
#[tauri::command] fn engine_eq_curve(s: S, eq: Value, freqs: Vec<f32>) -> Vec<f32> {
    use gc_dsp::blocks::Processor;
    let mut e = gc_dsp::blocks::Eq::new(s.engine.sr()); e.set(&eq); freqs.iter().map(|f| e.response_db(*f)).collect()
}
#[tauri::command] fn record_start(s: S, name: String) -> R<Value> {
    let path = s.dirs.recordings.join(files::safe(&name)); s.engine.record_start(path.clone())?;
    let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    Ok(json!({ "file": path, "wall": wall }))
}
#[tauri::command] fn record_stop(s: S, name: Option<String>) -> R<Value> {
    let (mut p, bytes) = s.engine.record_stop().ok_or("not recording")?;
    if let Some(n) = name { let dest = s.dirs.recordings.join(files::safe(&n)); if std::fs::rename(&p, &dest).is_ok() { p = dest; } }
    Ok(json!({ "file": p, "bytes": bytes }))
}

fn main() {
    let dirs = Dirs::new();
    let engine = Engine::new();
    let bridge = control::Bridge::new();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::PendingUpdate::default())
        .manage(AppState { engine, dirs: Dirs::new(), bridge: bridge.clone() })
        .setup(move |app| {
            control::start(app.handle().clone(), bridge.clone(), &dirs);
            midi::start(app.handle().clone());
            if let Some(w) = app.get_webview_window("main") {
                let x = std::env::var("GC_X").ok().and_then(|v| v.parse::<f64>().ok()); let y = std::env::var("GC_Y").ok().and_then(|v| v.parse::<f64>().ok());
                if let (Some(x), Some(y)) = (x, y) { let _ = w.set_position(tauri::LogicalPosition::new(x, y)); }
            }
            Ok(())
        })
        .on_window_event(|w, ev| { if let tauri::WindowEvent::Destroyed = ev { let st = w.state::<AppState>(); let file = st.dirs.control_file(); if std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["pid"].as_u64()) == Some(std::process::id() as u64) { let _ = std::fs::remove_file(file); } } })
        .invoke_handler(tauri::generate_handler![
            updates::check_update, updates::install_update,
            fs_dirs, presets_list, presets_save, presets_delete, settings_load, settings_save, read_text, import_to, backgrounds_list, background_import, list_files, write_json, reveal, clipboard_write,
            catalog_discover, plugins_scan, file_open, app_relaunch, app_info, rpc_response,
            engine_configure, engine_devices, engine_set_rig, engine_user_pedals, engine_mute, engine_meters, engine_spectrum, engine_analyze, engine_tuner, engine_status,
            engine_di_bytes, engine_di_path, engine_di_play, engine_test_tone, engine_metronome, engine_eq_curve, record_start, record_stop
        ])
        .run(tauri::generate_context!())
        .expect("error while running GuitarCraft");
}
