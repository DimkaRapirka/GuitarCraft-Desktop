//! Cabinet room: two close microphones and a stereo pair of room microphones.
//!
//! Close mics: the cab+mic model of `ir.rs`, plus off-axis angle and the time of flight for their
//! distance (relative to the closest mic, so two mics comb-filter like real ones and a single mic adds
//! no latency). Room mics: the cabinet heard from far away, placed in a rectangular room — early
//! reflections from image sources (order ≤ 3) and a decorrelated diffuse tail decaying with the
//! room's RT60. Everything is rendered off the audio thread into impulse responses.
use crate::biquad::{Biquad, Kind};
use crate::blocks::{num, txt};
use crate::util::{lerp, Lcg};
use serde_json::Value;

pub const SPEED_OF_SOUND: f32 = 343.0;

#[derive(Clone, Debug, PartialEq)]
pub struct CloseMic { pub on: bool, pub mic: String, pub position: f32, pub distance: f32, pub angle: f32, pub level_db: f32, pub pan: f32, pub phase: bool }
#[derive(Clone, Debug, PartialEq)]
pub struct RoomMics { pub on: bool, pub room: String, pub mic: String, pub distance: f32, pub width: f32, pub level_db: f32, pub lowcut: f32 }
#[derive(Clone, Debug, PartialEq)]
pub struct CabSetup { pub model: String, pub ir: Option<String>, pub mics: [CloseMic; 2], pub room: RoomMics, pub solo: Option<String>, pub level: f32 }

pub struct RoomType { pub id: &'static str, pub name: &'static str, pub dims: (f32, f32, f32), pub rt60: f32, pub damping: f32, pub desc: &'static str }
/// Rooms the room mics can stand in. `damping` 0..1: how fast high frequencies die (soft rooms are darker).
pub const ROOMS: &[RoomType] = &[
    RoomType { id: "booth", name: "Iso Booth", dims: (2.2, 1.8, 2.3), rt60: 0.18, damping: 0.85, desc: "Заглушённая будка: почти сухо, только плотность" },
    RoomType { id: "studio", name: "Studio A", dims: (6.5, 5.0, 3.2), rt60: 0.38, damping: 0.6, desc: "Контрольная студия: короткая ровная комната" },
    RoomType { id: "live", name: "Live Room", dims: (11.0, 8.0, 4.5), rt60: 0.85, damping: 0.4, desc: "Деревянная live-комната: объём без хвоста" },
    RoomType { id: "garage", name: "Garage", dims: (6.0, 3.6, 2.6), rt60: 0.95, damping: 0.15, desc: "Бетонный гараж: яркие звенящие отражения" },
    RoomType { id: "hall", name: "Hall", dims: (26.0, 16.0, 9.0), rt60: 1.8, damping: 0.45, desc: "Концертный зал: широкий и далёкий" },
    RoomType { id: "church", name: "Stone Church", dims: (34.0, 13.0, 13.0), rt60: 3.2, damping: 0.3, desc: "Каменная церковь: длинный светлый хвост" },
];
pub fn room_type(id: &str) -> &'static RoomType { ROOMS.iter().find(|r| r.id == id).unwrap_or(&ROOMS[1]) }

fn db_from_gain(g: f32) -> f32 { if g <= 1e-6 { -96.0 } else { 20.0 * g.log10() } }

/// Read `rig.cab`. New layout: `mics: {a: {...}, b: {...}}`, `room: {...}`, `solo`.
/// Older presets (`mic`, `position`, `distance`, `mic2`, `position2`, `distance2`, `blend`, `invert2`) map onto it
/// so they sound the same.
pub fn cab_setup(v: &Value) -> CabSetup {
    let legacy_b = txt(v, "mic2", "").to_string();
    let blend = num(v, "blend", 0.5).clamp(0.0, 1.0);
    let has_b = !legacy_b.is_empty() && blend > 0.0;
    let mic = |key: &str, d: CloseMic| -> CloseMic {
        let m = v.get("mics").and_then(|m| m.get(key)).cloned().unwrap_or(Value::Null);
        if m.is_null() { return d; }
        CloseMic {
            on: m.get("on").and_then(|x| x.as_bool()).unwrap_or(d.on), mic: txt(&m, "mic", &d.mic).to_string(),
            position: num(&m, "position", d.position).clamp(0.0, 1.0), distance: num(&m, "distance", d.distance).clamp(0.0, 1.0),
            angle: num(&m, "angle", d.angle).clamp(0.0, 1.0), level_db: num(&m, "level", d.level_db).clamp(-60.0, 12.0),
            pan: num(&m, "pan", d.pan).clamp(-1.0, 1.0), phase: m.get("phase").and_then(|x| x.as_bool()).unwrap_or(d.phase),
        }
    };
    let a = mic("a", CloseMic { on: true, mic: txt(v, "mic", "dyn57").into(), position: num(v, "position", 0.3), distance: num(v, "distance", 0.2), angle: 0.0,
        level_db: if has_b { db_from_gain(1.0 - blend) } else { 0.0 }, pan: 0.0, phase: false });
    let b = mic("b", CloseMic { on: has_b, mic: if legacy_b.is_empty() { "ribbon121".into() } else { legacy_b.clone() }, position: num(v, "position2", 0.65),
        distance: num(v, "distance2", 0.25), angle: 0.0, level_db: if has_b { db_from_gain(blend) } else { -6.0 }, pan: 0.0, phase: v["invert2"].as_bool().unwrap_or(false) });
    let r = v.get("room").cloned().unwrap_or(Value::Null);
    let room = RoomMics { on: r.get("on").and_then(|x| x.as_bool()).unwrap_or(false), room: txt(&r, "room", "studio").into(), mic: txt(&r, "mic", "cond414").into(),
        distance: num(&r, "distance", 0.35).clamp(0.0, 1.0), width: num(&r, "width", 0.6).clamp(0.0, 1.0), level_db: num(&r, "level", -8.0).clamp(-60.0, 12.0),
        lowcut: num(&r, "lowcut", 120.0).clamp(20.0, 600.0) };
    let ir = if txt(v, "mode", "model") == "ir" { v.get("irPath").and_then(|x| x.as_str()).map(String::from) } else { None };
    let solo = v.get("solo").and_then(|x| x.as_str()).filter(|s| ["a", "b", "room"].contains(s)).map(String::from);
    CabSetup { model: txt(v, "model", "4x12-v").into(), ir, mics: [a, b], room, solo, level: num(v, "level", 0.5) }
}

/// Distance knob (0..1) → metres from the grille for a close mic.
pub fn close_distance_m(d: f32) -> f32 { 0.01 + d * 0.6 }

/// Close-mic IR: cab + mic model, off-axis angle, and `delay_m` of extra flight time.
pub fn close_mic_ir(sr: f32, cab: &str, m: &CloseMic, delay_m: f32) -> Vec<f32> {
    let mut ir = crate::ir::cab_ir(sr, cab, &m.mic, m.position, m.distance);
    if m.angle > 0.0 {
        // pointing the capsule away from the dust cap loses top end and some presence
        let mut f = [Biquad::new(sr, Kind::Highshelf, 2500.0, 0.7, -m.angle * 9.0), Biquad::new(sr, Kind::Peaking, 4200.0, 1.0, -m.angle * 3.0), Biquad::new(sr, Kind::Lowpass, lerp(16000.0, 6500.0, m.angle), 0.6, 0.0)];
        for b in f.iter_mut() { b.process(&mut ir); }
    }
    delayed(&ir, delay_m / SPEED_OF_SOUND * sr)
}

fn delayed(x: &[f32], d: f32) -> Vec<f32> {
    if d < 0.01 { return x.to_vec(); }
    let i0 = d.floor() as usize; let fr = d - d.floor();
    let mut y = vec![0f32; x.len() + i0 + 1];
    for (i, v) in x.iter().enumerate() { y[i + i0] += v * (1.0 - fr); y[i + i0 + 1] += v * fr; }
    y
}

fn add_tap(buf: &mut [f32], t: f32, g: f32) {
    let i0 = t.floor() as usize; let fr = t - t.floor();
    if i0 + 1 < buf.len() { buf[i0] += g * (1.0 - fr); buf[i0 + 1] += g * fr; }
}

/// Stereo room response from the cabinet to a spaced pair of room mics.
/// `distance` 0..1 moves the pair from 0.5 m to ~70 % of the room length; `width` 0..1 spaces them 0.1–2 m.
pub fn room_response(sr: f32, room: &RoomType, distance: f32, width: f32) -> (Vec<f32>, Vec<f32>) {
    let (lx, ly, lz) = room.dims;
    let v = lx * ly * lz; let s = 2.0 * (lx * ly + lx * lz + ly * lz);
    // Sabine: average absorption that gives this RT60, and the matching reflection amplitude
    let alpha = (0.161 * v / (room.rt60 * s)).clamp(0.02, 0.95);
    let beta = (1.0 - alpha).sqrt();
    let src = (lx * 0.5, 0.6f32.min(ly * 0.3), 0.5f32);
    let d = 0.5 + distance * (ly * 0.7 - 0.5).max(0.2);
    let spacing = 0.1 + width * 1.9;
    let mics = [(lx * 0.5 - spacing / 2.0, (src.1 + d).min(ly - 0.3), 1.5f32.min(lz - 0.3)), (lx * 0.5 + spacing / 2.0, (src.1 + d).min(ly - 0.3), 1.5f32.min(lz - 0.3))];
    let len = ((room.rt60 * 1.25 + 0.08) * sr) as usize;
    let mut out = [vec![0f32; len], vec![0f32; len]];
    let order = 3i32;
    for (c, m) in mics.iter().enumerate() {
        let mut early = vec![0f32; len];
        let mut first = f32::MAX;
        for nx in -order..=order { for ny in -order..=order { for nz in -order..=order {
            for (px, py, pz) in [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 0), (1, 0, 1), (0, 1, 1), (1, 1, 1)] {
                // Allen & Berkley: an image (n, p) on one axis has |n - p| + |n| wall reflections
                let k = (nx - px).abs() + nx.abs() + (ny - py).abs() + ny.abs() + (nz - pz).abs() + nz.abs();
                if k > order { continue; }
                let ix = 2.0 * nx as f32 * lx + if px == 1 { -src.0 } else { src.0 };
                let iy = 2.0 * ny as f32 * ly + if py == 1 { -src.1 } else { src.1 };
                let iz = 2.0 * nz as f32 * lz + if pz == 1 { -src.2 } else { src.2 };
                let r = ((ix - m.0).powi(2) + (iy - m.1).powi(2) + (iz - m.2).powi(2)).sqrt().max(0.3);
                let t = r / SPEED_OF_SOUND * sr;
                if t as usize + 2 >= len { continue; }
                first = first.min(t);
                // each bounce loses level and a little top end (spread the tap for later orders)
                let g = beta.powi(k) / r * (1.0 - room.damping * 0.25).powi(k);
                if k <= 1 { add_tap(&mut early, t, g); } else { add_tap(&mut early, t - 1.0, g * 0.25); add_tap(&mut early, t, g * 0.5); add_tap(&mut early, t + 1.0, g * 0.25); }
            }
        }}}
        // diffuse tail: decorrelated noise, fading in after the early field, darker with time
        let e_early: f32 = early.iter().map(|x| x * x).sum::<f32>();
        let mut rnd = Lcg(if c == 0 { 90127 } else { 31337 });
        let t0 = first.max(1.0); let mix_t = t0 + 0.012 * sr * (v.powf(1.0 / 3.0) / 4.0);
        let mut tail = vec![0f32; len]; let mut lp = 0f32;
        for i in t0 as usize..len {
            let t = (i as f32 - t0) / sr;
            let fade = ((i as f32 - t0) / (mix_t - t0 + 1.0)).min(1.0);
            let env = (-6.9 * t / room.rt60).exp();
            let k = (1.0 - room.damping * 0.8) * (-t * (1.0 + room.damping * 3.0)).exp() + 0.03;
            lp += k.min(1.0) * (rnd.next() - lp);
            tail[i] = lp * env * fade;
        }
        let e_tail: f32 = tail.iter().map(|x| x * x).sum::<f32>();
        // the diffuse field carries more energy than the early reflections in live rooms
        let ratio = (room.rt60 / 0.4).clamp(0.4, 6.0);
        let tg = (e_early * ratio / (e_tail + 1e-12)).sqrt();
        for i in 0..len { out[c][i] = early[i] + tail[i] * tg; }
    }
    let [l, r] = out; (l, r)
}

/// Linear convolution of two IRs (FFT), used to put the cabinet's far-field sound into the room.
pub fn convolve(a: &[f32], b: &[f32]) -> Vec<f32> {
    use realfft::RealFftPlanner;
    let n = (a.len() + b.len() - 1).next_power_of_two();
    let mut p = RealFftPlanner::<f32>::new(); let f = p.plan_fft_forward(n); let inv = p.plan_fft_inverse(n);
    let (mut xa, mut xb) = (vec![0f32; n], vec![0f32; n]);
    xa[..a.len()].copy_from_slice(a); xb[..b.len()].copy_from_slice(b);
    let (mut fa, mut fb) = (f.make_output_vec(), f.make_output_vec());
    f.process(&mut xa, &mut fa).ok(); f.process(&mut xb, &mut fb).ok();
    for (x, y) in fa.iter_mut().zip(fb.iter()) { *x *= y; }
    fa[0].im = 0.0; let last = fa.len() - 1; fa[last].im = 0.0;
    let mut out = vec![0f32; n]; inv.process(&mut fa, &mut out).ok();
    out.truncate(a.len() + b.len() - 1);
    out.iter_mut().for_each(|v| *v /= n as f32);
    out
}

/// Room-mic IRs (L, R) for a cabinet: far-field cab sound × room × room-mic low cut, loudness-matched to a close mic.
pub fn room_mic_irs(sr: f32, source: &[f32], r: &RoomMics) -> (Vec<f32>, Vec<f32>) {
    let room = room_type(&r.room);
    let (rl, rr) = room_response(sr, room, r.distance, r.width);
    let mut l = convolve(source, &rl); let mut rt = convolve(source, &rr);
    for ir in [&mut l, &mut rt] {
        let mut hp = [Biquad::new(sr, Kind::Highpass, r.lowcut, 0.7, 0.0), Biquad::new(sr, Kind::Highpass, r.lowcut, 0.7, 0.0)];
        for b in hp.iter_mut() { b.process(ir); }
    }
    let e: f64 = l.iter().chain(rt.iter()).map(|v| (*v as f64).powi(2)).sum();
    let k = (0.5 / (e / 2.0 + 1e-12).sqrt()) as f32;
    l.iter_mut().chain(rt.iter_mut()).for_each(|v| *v *= k);
    (l, rt)
}

/// All cab kernels: [close A, close B, room L, room R] (empty = not needed).
pub fn cab_kernels(sr: f32, s: &CabSetup, user_ir: Option<Vec<f32>>) -> Vec<Vec<f32>> {
    let closest = s.mics.iter().filter(|m| m.on).map(|m| close_distance_m(m.distance)).fold(f32::MAX, f32::min);
    let closest = if closest == f32::MAX { 0.0 } else { closest };
    let mut out: Vec<Vec<f32>> = match &user_ir {
        Some(ir) => vec![ir.clone(), vec![]],
        None => s.mics.iter().map(|m| close_mic_ir(sr, &s.model, m, if m.on { close_distance_m(m.distance) - closest } else { 0.0 })).collect(),
    };
    if s.room.on {
        let source = match &user_ir { Some(ir) => ir.clone(), None => crate::ir::cab_ir(sr, &s.model, &s.room.mic, 0.5, 1.0) };
        let (l, r) = room_mic_irs(sr, &source, &s.room);
        out.push(l); out.push(r);
    } else { out.push(vec![]); out.push(vec![]); }
    out
}

/// Cache key: everything that changes the kernels (levels, pans, phase, mute and solo are applied live).
pub fn kernel_key(s: &CabSetup) -> String {
    let m = |c: &CloseMic| format!("{}:{:.3}:{:.3}:{:.3}:{}", c.mic, c.position, c.distance, c.angle, c.on);
    let r = &s.room;
    format!("cab2:{}:{}:{}:{}:{}", s.model, s.ir.as_deref().unwrap_or(""), m(&s.mics[0]), m(&s.mics[1]),
        if r.on { format!("{}:{}:{:.3}:{:.3}:{:.0}", r.room, r.mic, r.distance, r.width, r.lowcut) } else { "off".into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_presets_map_to_two_close_mics() {
        let s = cab_setup(&json!({ "mic": "dyn57", "mic2": "ribbon121", "blend": 0.5, "invert2": true }));
        assert!(s.mics[0].on && s.mics[1].on);
        assert!((s.mics[0].level_db + 6.02).abs() < 0.05 && (s.mics[1].level_db + 6.02).abs() < 0.05);
        assert!(s.mics[1].phase && !s.room.on);
        let single = cab_setup(&json!({ "mic": "cond414" }));
        assert!(!single.mics[1].on && single.mics[0].level_db == 0.0 && single.mics[0].mic == "cond414");
    }

    #[test]
    fn farther_close_mic_arrives_later() {
        let s = cab_setup(&json!({ "mics": { "a": { "distance": 0.0 }, "b": { "on": true, "distance": 1.0 } } }));
        let k = cab_kernels(48000.0, &s, None);
        let onset = |x: &[f32]| x.iter().position(|v| v.abs() > 1e-4).unwrap();
        let expected = (close_distance_m(1.0) - close_distance_m(0.0)) / SPEED_OF_SOUND * 48000.0;
        assert!((onset(&k[1]) as f32 - onset(&k[0]) as f32 - expected).abs() < 3.0);
    }

    #[test]
    fn rooms_are_stereo_and_bigger_rooms_ring_longer() {
        let tail = |id: &str| {
            let (l, r) = room_response(48000.0, room_type(id), 0.5, 0.8);
            assert_ne!(l, r, "room pair must be decorrelated");
            let e: Vec<f32> = l.iter().map(|v| v * v).collect(); let total: f32 = e.iter().sum();
            let mut acc = 0.0; e.iter().position(|v| { acc += v; acc > total * 0.99 }).unwrap() as f32 / 48000.0
        };
        assert!(tail("booth") < tail("studio") && tail("studio") < tail("hall") && tail("hall") < tail("church"));
    }
}
