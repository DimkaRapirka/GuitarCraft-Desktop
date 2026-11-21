//! GuitarCraft MCP server (stdio, JSON-RPC 2.0, newline-delimited) in Rust.
//! Same tools as before; every call goes to the running app's local control API (127.0.0.1 + token from
//! ~/Library/Application Support/GuitarCraft/control.json) with source "mcp", so it lands in the undo history.
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;

const INSTRUCTIONS: &str = "GuitarCraft is a guitar amp & pedal app (Rust core). Signal flow: input → gate → chain → output. The chain mixes pedals with fixed core blocks: amp, cab, eq, delay, reverb.
Workflow: chain_get → catalog_search (Russian or English: name, type or sound character, e.g. \"тёплый овердрайв\") → chain_add / chain_replace / chain_move → param_set → tone_analyze to check → preset_save.
Targets: a pedal uid (from chain_get), a pedal name, or gate | amp | cab | eq | delay | reverb | input | output.
Pedal and amp knobs are 0..1 unless chain_get shows a unit. EQ: lowcut 20..400 Hz, low/lowmid/mid/highmid/high -12..12 dB, highcut 2000..20000 Hz.
To remove excess low end: run tone_analyze, look at deviation_from_reference_db for sub/low; raise eq.lowcut (100-160 Hz) and cut eq.low; re-run tone_analyze until the low deviation is within about ±3 dB.
All changes are undoable with history_undo. The user sees every action in the app. Agent access must be enabled by the user in GuitarCraft → Settings → Agents (MCP).";

fn control() -> Result<(u16, String), String> {
    let path = std::env::var_os("GC_CONTROL_FILE").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("GC_DATA_DIR").map(|p| std::path::PathBuf::from(p).join("control.json")))
        .or_else(|| dirs::data_dir().map(|p| p.join("GuitarCraft/control.json")))
        .ok_or("Не найдена папка данных GuitarCraft")?;
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|_| "GuitarCraft не запущен: откройте приложение".to_string())?).map_err(|e| e.to_string())?;
    Ok((v["port"].as_u64().unwrap_or(47820) as u16, v["token"].as_str().unwrap_or("").to_string()))
}

fn rpc(method: &str, params: Value) -> Result<Value, String> {
    let (port, token) = control()?;
    let body = json!({ "method": method, "params": params, "source": "mcp" }).to_string();
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|_| "GuitarCraft не отвечает: откройте приложение".to_string())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(130))).ok();
    write!(s, "POST /rpc HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())?;
    let mut resp = String::new(); s.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    let payload = resp.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    let v: Value = serde_json::from_str(payload).map_err(|e| format!("bad response: {e}"))?;
    if let Some(e) = v.get("error") { return Err(e.as_str().unwrap_or("error").to_string()); }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

fn obj(props: Value, required: &[&str]) -> Value { json!({ "type": "object", "properties": props, "required": required }) }
fn place() -> Value { json!({
    "index": { "type": "integer", "description": "position in the chain array (0 = first)" },
    "before": { "type": "string", "description": "insert before this block (uid, name or core block)" },
    "after": { "type": "string", "description": "insert after this block" },
    "position": { "type": "string", "enum": ["pre-amp", "post-amp", "end"], "description": "shortcut; default pre-amp" } }) }
fn merge(a: Value, b: Value) -> Value { let mut a = a; if let (Some(x), Some(y)) = (a.as_object_mut(), b.as_object()) { for (k, v) in y { x.insert(k.clone(), v.clone()); } } a }
const TARGET: &str = "pedal uid or name, or gate|amp|cab|eq|delay|reverb|input|output";

/// (name, method in the app, description, input schema)
fn tools() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    let cats = json!(["drive", "dynamics", "filter", "modulation", "time", "pitch", "utility"]);
    vec![
        ("chain_get", "state.get", "Show the current rig: preset, chain blocks in order with uids, parameters (value/min/max/unit), unsaved changes, recent undo entries.", obj(json!({}), &[])),
        ("catalog_search", "catalog.search", "Search the same catalog as the \"+\" browser: built-in pedals, user DSP-block pedals and scanned third-party plugins. Query by name, type or sound character (RU/EN).",
            obj(json!({ "query": { "type": "string" }, "category": { "type": "string", "enum": cats }, "source": { "type": "string", "enum": ["builtin", "user", "extension", "plugin"] }, "favorites": { "type": "boolean" }, "recent": { "type": "boolean" }, "limit": { "type": "integer" } }), &[])),
        ("chain_add", "chain.add", "Insert a pedal from the catalog into the chain. Default place: right before the amp.", obj(merge(json!({ "pedal": { "type": "string", "description": "catalog id, e.g. gc.verdant" }, "params": { "type": "object" } }), place()), &["pedal"])),
        ("chain_move", "chain.move", "Move a pedal, amp, cabinet, EQ, delay or reverb to another place in the chain.", obj(merge(json!({ "target": { "type": "string", "description": TARGET } }), place()), &["target"])),
        ("chain_remove", "chain.remove", "Remove a pedal (core blocks can only be bypassed with chain_toggle).", obj(json!({ "target": { "type": "string", "description": TARGET } }), &["target"])),
        ("chain_replace", "chain.replace", "Replace a pedal in place with another catalog pedal.", obj(json!({ "target": { "type": "string" }, "pedal": { "type": "string" } }), &["target", "pedal"])),
        ("chain_toggle", "chain.toggle", "Bypass or enable a pedal or core block.", obj(json!({ "target": { "type": "string" }, "on": { "type": "boolean" } }), &["target"])),
        ("param_set", "param.set", "Set one parameter. amp: channel (clean|crunch|rhythm|lead), gain, bass, mid, treble, presence, master (0..1). cab: model, mic, position, distance, level, irPath. eq: lowcut, low, lowmid, mid, highmid, high, highcut. gate: threshold dB, release ms. input/output: dB.",
            obj(json!({ "target": { "type": "string", "description": TARGET }, "param": { "type": "string" }, "value": { "type": ["number", "string"] } }), &["target", "param", "value"])),
        ("params_set", "params.set", "Set several parameters at once (each becomes an undo step).", obj(json!({ "changes": { "type": "array", "items": obj(json!({ "target": { "type": "string" }, "param": { "type": "string" }, "value": { "type": ["number", "string"] } }), &["target", "param", "value"]) } }), &["changes"])),
        ("tone_analyze", "tone.analyze", "Listen to the app output for N seconds and return energy per band (sub, low, lowmid, mid, highmid, high), deviation from a reference balance for distorted guitar and an assessment. Aim for |deviation| ≤ 3 dB on the band you fix; re-run after changes.", obj(json!({ "seconds": { "type": "number", "minimum": 0.5, "maximum": 10 } }), &[])),
        ("audition", "audition.*", "Temporarily insert a catalog pedal into the chain without changing the preset (A/B). action=start|on|off|stop.", obj(merge(json!({ "action": { "type": "string", "enum": ["start", "on", "off", "stop"] }, "pedal": { "type": "string" }, "replace": { "type": "string" } }), place()), &["action"])),
        ("preset_list", "preset.list", "List factory and user presets.", obj(json!({}), &[])),
        ("preset_load", "preset.load", "Load a preset by name.", obj(json!({ "name": { "type": "string" } }), &["name"])),
        ("preset_save", "preset.save", "Save the current rig. With name: Save As (new preset, user bank). Without name: overwrite the current user preset.", obj(json!({ "name": { "type": "string" }, "bank": { "type": "string" } }), &[])),
        ("history_undo", "history.undo", "Undo the last N changes (UI or agent).", obj(json!({ "steps": { "type": "integer", "minimum": 1, "maximum": 50 } }), &[])),
        ("history_redo", "history.redo", "Redo N undone changes.", obj(json!({ "steps": { "type": "integer", "minimum": 1, "maximum": 50 } }), &[])),
        ("history_list", "history.list", "Show undo/redo history labels.", obj(json!({}), &[])),
        ("pedal_create", "pedal.create", "Create a user pedal from DSP blocks (gain, filter, drive, delay, tremolo, chorus, ring). Values can be numbers or bound to a knob: {\"knob\":\"drive\",\"from\":1,\"to\":40,\"exp\":true}. It appears in the catalog (source user) and is processed by the Rust core.",
            obj(json!({ "pedal": obj(json!({ "name": { "type": "string" }, "type": { "type": "string" }, "category": { "type": "string", "enum": cats }, "color": { "type": "string" }, "character": { "type": "array", "items": { "type": "string" } }, "desc": { "type": "string" },
                "knobs": { "type": "array", "items": obj(json!({ "id": { "type": "string" }, "name": { "type": "string" }, "def": { "type": "number" } }), &["id"]) }, "blocks": { "type": "array", "items": { "type": "object" } } }), &["name", "knobs", "blocks"]) }), &["pedal"])),
        ("plugin_formats", "catalog.formats", "Which third-party plugin and file formats work in GuitarCraft.", obj(json!({}), &[])),
        ("tuner_read", "tuner.read", "Current tuner reading of the input (note, cents, Hz) or null.", obj(json!({}), &[])),
    ]
}

fn call_tool(name: &str, args: Value) -> Result<Value, String> {
    let list = tools();
    let (_, method, _, _) = list.iter().find(|t| t.0 == name).ok_or(format!("unknown tool {name}"))?;
    if *method == "audition.*" {
        return match args.get("action").and_then(|x| x.as_str()).unwrap_or("") {
            "start" => rpc("audition.start", args), "stop" => rpc("audition.stop", json!({})),
            a => rpc("audition.ab", json!({ "active": a == "on" })),
        };
    }
    rpc(method, args)
}

fn main() {
    let stdin = std::io::stdin(); let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break }; if line.trim().is_empty() { continue; }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|x| x.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result: Option<Result<Value, (i64, String)>> = match method {
            "initialize" => Some(Ok(json!({ "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
                "capabilities": { "tools": { "listChanged": false } }, "serverInfo": { "name": "guitarcraft", "version": env!("CARGO_PKG_VERSION") }, "instructions": INSTRUCTIONS }))),
            "ping" => Some(Ok(json!({}))),
            "tools/list" => Some(Ok(json!({ "tools": tools().into_iter().map(|(n, _, d, s)| json!({ "name": n, "description": d, "inputSchema": s })).collect::<Vec<_>>() }))),
            "tools/call" => {
                let name = params.get("name").and_then(|x| x.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Some(Ok(match call_tool(name, args) {
                    Ok(v) => json!({ "content": [{ "type": "text", "text": if v.is_string() { v.as_str().unwrap().to_string() } else { serde_json::to_string_pretty(&v).unwrap() } }] }),
                    Err(e) => json!({ "isError": true, "content": [{ "type": "text", "text": e }] }),
                }))
            }
            m if m.starts_with("notifications/") => None,
            _ => Some(Err((-32601, format!("method not found: {method}")))),
        };
        if let (Some(id), Some(r)) = (id, result) {
            let resp = match r { Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }), Err((c, m)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": c, "message": m } }) };
            let _ = writeln!(out, "{resp}"); let _ = out.flush();
        }
    }
}
