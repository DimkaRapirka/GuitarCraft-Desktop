//! MIDI input via CoreMIDI (midir): every connected port, re-scanned every 2 s, forwarded to the UI.
use std::collections::HashMap;
use tauri::{AppHandle, Emitter};

pub fn start(app: AppHandle) {
    std::thread::Builder::new().name("gc-midi".into()).spawn(move || {
        let mut conns: HashMap<String, midir::MidiInputConnection<()>> = HashMap::new();
        loop {
            if let Ok(probe) = midir::MidiInput::new("GuitarCraft-probe") {
                let names: Vec<String> = probe.ports().iter().filter_map(|p| probe.port_name(p).ok()).collect();
                conns.retain(|n, _| names.contains(n));
                let missing: Vec<String> = names.iter().filter(|n| !conns.contains_key(*n)).cloned().collect();
                for name in missing.iter() {
                    let Ok(mut input) = midir::MidiInput::new("GuitarCraft") else { continue };
                    input.ignore(midir::Ignore::SysexAndTime);
                    let Some(port) = input.ports().into_iter().find(|p| input.port_name(p).ok().as_deref() == Some(name)) else { continue };
                    let app2 = app.clone(); let from = name.clone();
                    if let Ok(c) = input.connect(&port, "gc-in", move |_t, msg, _| { let _ = app2.emit("midi", serde_json::json!({ "data": msg, "from": from })); }, ()) { conns.insert(name.clone(), c); }
                }
                let _ = app.emit("midi-devices", serde_json::json!(names));
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }).unwrap();
}
