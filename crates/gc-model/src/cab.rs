//! Cabinet room layout: close mics `a` and `b`, a stereo pair of room mics, and solo.
//! Older presets stored one blended IR (`mic`, `mic2`, `blend`, `invert2`…); `migrate` turns them into the
//! new layout with the same sound (the DSP reads both, see `gc_dsp::room::cab_setup`).
use serde_json::{json, Value};

fn num(v: &Value, k: &str, d: f64) -> f64 { v.get(k).and_then(Value::as_f64).unwrap_or(d) }
fn db(g: f64) -> f64 { if g <= 1e-6 { -60.0 } else { (20.0 * g.log10() * 100.0).round() / 100.0 } }

/// Give `cab` the `mics` / `room` layout if it still uses the old one. Keeps `mic` in sync for older readers.
pub fn migrate(cab: &mut Value) {
    if !cab.is_object() { *cab = json!({}); }
    if cab.get("mics").map_or(false, Value::is_object) {
        if cab.get("room").map_or(true, |r| !r.is_object()) { cab["room"] = default_room(); }
        return;
    }
    let b_mic = cab.get("mic2").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
    let blend = num(cab, "blend", 0.5).clamp(0.0, 1.0);
    let has_b = b_mic.is_some() && blend > 0.0;
    cab["mics"] = json!({
        "a": { "on": true, "mic": cab.get("mic").and_then(Value::as_str).unwrap_or("dyn57"), "position": num(cab, "position", 0.3), "distance": num(cab, "distance", 0.2),
               "angle": 0.0, "level": if has_b { db(1.0 - blend) } else { 0.0 }, "pan": 0.0, "phase": false },
        "b": { "on": has_b, "mic": b_mic.unwrap_or_else(|| "ribbon121".into()), "position": num(cab, "position2", 0.65), "distance": num(cab, "distance2", 0.25),
               "angle": 0.0, "level": if has_b { db(blend) } else { -6.0 }, "pan": 0.0, "phase": cab.get("invert2").and_then(Value::as_bool).unwrap_or(false) },
    });
    cab["room"] = default_room();
    if let Some(o) = cab.as_object_mut() { for k in ["mic2", "position2", "distance2", "blend", "invert2"] { o.remove(k); } }
}

pub fn default_room() -> Value { json!({ "on": false, "room": "studio", "mic": "cond414", "distance": 0.35, "width": 0.6, "level": -8.0, "lowcut": 120.0 }) }

/// Ready-made rooms for one click: room settings that work for a typical guitar mix.
pub fn room_presets() -> Value {
    json!([
        { "id": "tight", "name": "Плотно", "room": { "on": true, "room": "booth", "distance": 0.2, "width": 0.4, "level": -10.0 } },
        { "id": "studio", "name": "Студия", "room": { "on": true, "room": "studio", "distance": 0.35, "width": 0.6, "level": -8.0 } },
        { "id": "big", "name": "Большая комната", "room": { "on": true, "room": "live", "distance": 0.5, "width": 0.8, "level": -6.0 } },
        { "id": "hall", "name": "Зал", "room": { "on": true, "room": "hall", "distance": 0.6, "width": 0.9, "level": -7.0 } },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_two_mic_cab_becomes_two_close_mics() {
        let mut c = json!({ "mic": "dyn57", "mic2": "ribbon121", "blend": 0.5, "invert2": true, "position2": 0.7 });
        migrate(&mut c);
        assert_eq!(c["mics"]["b"]["on"], true);
        assert_eq!(c["mics"]["b"]["phase"], true);
        assert_eq!(c["mics"]["b"]["position"], 0.7);
        assert!((c["mics"]["a"]["level"].as_f64().unwrap() + 6.02).abs() < 0.01);
        assert!(c.get("blend").is_none() && c["room"]["on"] == false);
        let before = c.clone(); migrate(&mut c); assert_eq!(c, before, "migration is idempotent");
    }
}
