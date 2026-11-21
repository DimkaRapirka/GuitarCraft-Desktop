//! Local control API (127.0.0.1, bearer token in control.json) — same protocol as the Electron version,
//! so the MCP server and the recording tools keep working. RPCs are executed by the UI store
//! (undo history lives there) and answered through the `rpc_response` command.
use crate::files::Dirs;
use crossbeam_channel::{bounded, Sender};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

pub struct Bridge { pending: Mutex<HashMap<u64, Sender<Result<Value, String>>>>, seq: AtomicU64 }
impl Bridge {
    pub fn new() -> Arc<Self> { Arc::new(Self { pending: Mutex::new(HashMap::new()), seq: AtomicU64::new(1) }) }
    pub fn respond(&self, id: u64, r: Result<Value, String>) { if let Some(tx) = self.pending.lock().remove(&id) { let _ = tx.send(r); } }
    fn call(&self, app: &AppHandle, method: &str, params: Value, timeout_ms: u64) -> Result<Value, String> {
        let id = self.seq.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = bounded(1);
        self.pending.lock().insert(id, tx);
        app.emit_to("main", "rpc:request", json!({ "id": id, "method": method, "params": params })).map_err(|e| e.to_string())?;
        let r = rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)).map_err(|_| format!("timeout: {method}"));
        self.pending.lock().remove(&id);
        r?
    }
}

fn token() -> Result<String, getrandom::Error> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn bounds(app: &AppHandle) -> Value {
    let Some(w) = app.get_webview_window("main") else { return Value::Null };
    let sf = w.scale_factor().unwrap_or(1.0);
    let pos = w.outer_position().map(|p| p.to_logical::<f64>(sf)).unwrap_or_default();
    let size = w.outer_size().map(|s| s.to_logical::<f64>(sf)).unwrap_or_default();
    json!({ "x": pos.x.round(), "y": pos.y.round(), "width": size.width.round(), "height": size.height.round() })
}

pub fn start(app: AppHandle, bridge: Arc<Bridge>, dirs: &Dirs) {
    let port: u16 = std::env::var("GC_CONTROL_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47820);
    let Ok(tok) = token() else { eprintln!("[gc] control API: no secure randomness"); return; };
    let file = dirs.control_file();
    std::thread::Builder::new().name("gc-control".into()).spawn(move || {
        let server = match tiny_http::Server::http(("127.0.0.1", port)) { Ok(s) => s, Err(e) => { eprintln!("[gc] control API: {e}"); return; } };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).truncate(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(0o600);
            if file.exists() && std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).is_err() { return; }
        }
        let state = json!({ "port": port, "token": tok, "pid": std::process::id() });
        if options.open(&file).and_then(|mut f| write!(f, "{state}")).is_err() { return; }
        println!("[GuitarCraft] control API on 127.0.0.1:{port}");
        for mut req in server.incoming_requests() {
            let (app, bridge, tok) = (app.clone(), bridge.clone(), tok.clone());
            std::thread::spawn(move || {
                let respond = |req: tiny_http::Request, code: u16, v: Value| {
                    let h = tiny_http::Header::from_bytes(&b"content-type"[..], &b"application/json"[..]).unwrap();
                    let _ = req.respond(tiny_http::Response::from_string(v.to_string()).with_status_code(code).with_header(h));
                };
                let auth = req.headers().iter().find(|h| h.field.equiv("authorization")).map(|h| h.value.as_str().to_string());
                if auth.as_deref() != Some(&format!("Bearer {tok}")) { return respond(req, 401, json!({ "error": "unauthorized" })); }
                if req.method() != &tiny_http::Method::Post || req.url() != "/rpc" { return respond(req, 404, json!({ "error": "not found" })); }
                let mut body = String::new(); let _ = req.as_reader().take(20_000_000).read_to_string(&mut body);
                let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let mut params = v.get("params").cloned().unwrap_or(json!({}));
                let source = v.get("source").and_then(|x| x.as_str()).unwrap_or("api").to_string();
                if let Some(o) = params.as_object_mut() { o.insert("__source".into(), json!(source)); }
                match method.as_str() {
                    "window.front" => {
                        #[cfg(target_os = "macos")] { let _ = app.show(); } // activate the app, not just the window
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show(); let _ = w.unminimize(); let _ = w.set_focus();
                            if let Some(on) = params.get("onTop").and_then(|x| x.as_bool()) { let _ = w.set_always_on_top(on); }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(120));
                        respond(req, 200, json!({ "result": bounds(&app) }))
                    }
                    "window.bounds" => {
                        if let (Some(w), Some(width)) = (app.get_webview_window("main"), params.get("width").and_then(|x| x.as_f64())) {
                            let h = params.get("height").and_then(|x| x.as_f64()).unwrap_or(900.0);
                            let _ = w.set_size(tauri::LogicalSize::new(width, h));
                            if let (Some(x), Some(y)) = (params.get("x").and_then(|x| x.as_f64()), params.get("y").and_then(|x| x.as_f64())) { let _ = w.set_position(tauri::LogicalPosition::new(x, y)); }
                        }
                        respond(req, 200, json!({ "result": bounds(&app) }))
                    }
                    _ => {
                        let timeout = if method == "record.stop" { 120_000 } else { params.get("timeoutMs").and_then(|x| x.as_u64()).unwrap_or(30_000) };
                        match bridge.call(&app, &method, params, timeout) { Ok(r) => respond(req, 200, json!({ "result": r })), Err(e) => respond(req, 500, json!({ "error": e })) }
                    }
                }
            });
        }
    }).unwrap();
}
