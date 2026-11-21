//! Built-in pedals and DSP-block (builder) pedals. Same ids, parameters and voicing as the catalog.
mod clarinot;
mod fuzz_factory;

use crate::biquad::{Biquad, Kind};
use crate::blocks::{num, txt, Mix, Processor};
use crate::conv::{Convolver, DelayLine};
use crate::dynamics::{Compressor, Crush, Envelope, Gate, Octave};
use crate::shaper::{Curve, Shaper};
use crate::util::*;
use serde_json::Value;

// ------------------------------------------------------------------ generic drive
struct DriveCfg { pre: Kind, pre_f: f32, pre_q: f32, pre_g: f32, hp: f32, kind: Curve, max_gain: f32, tone: (f32, f32), level_db: f32, d_def: f32, tone_def: f32, level_def: f32 }
struct Drive { c: DriveCfg, h: Biquad, mid: Biquad, drive: Smooth, ws: Shaper, post: Biquad, level: Smooth }
impl Drive {
    fn new(sr: f32, c: DriveCfg) -> Self {
        Self { h: Biquad::new(sr, Kind::Highpass, c.hp, 0.6, 0.0), mid: Biquad::new(sr, c.pre, c.pre_f, c.pre_q, c.pre_g), drive: Smooth::new(2.0, sr, 0.02),
            ws: Shaper::new(c.kind, 1.0), post: Biquad::new(sr, Kind::Lowpass, 4000.0, 0.7, 0.0), level: Smooth::new(1.0, sr, 0.02), c }
    }
}
impl Processor for Drive {
    fn set(&mut self, v: &Value) {
        let c = &self.c;
        let d = v.get("drive").or(v.get("gain")).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(c.d_def);
        self.drive.set(lerp(1.0, c.max_gain, d.powf(1.6)));
        self.ws.amount = 0.4 + d * 0.8;
        self.post.set_freq(exp_map(num(v, "tone", c.tone_def), c.tone.0, c.tone.1));
        if let Some(b) = v.get("bass").and_then(|x| x.as_f64()) { self.h.set_freq(exp_map(1.0 - b as f32, 60.0, 700.0)); }
        self.level.set(db_to_gain(c.level_db + (num(v, "level", c.level_def) - 0.5) * 24.0) / (lerp(1.0, c.max_gain, d) / 6.0).sqrt().max(1.0));
    }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() { let x = self.mid.tick(self.h.tick(*s)) * self.drive.next(); *s = self.post.tick(self.ws.tick(x)) * self.level.next(); }
    }
}

// ------------------------------------------------------------------ simple pedals
struct Boost { hp: Biquad, sh: Biquad, g: Smooth, ws: Shaper }
impl Processor for Boost {
    fn set(&mut self, v: &Value) { let r = num(v, "range", 0.3); self.g.set(db_to_gain(num(v, "level", 0.6) * 18.0)); self.sh.set_gain(r * 12.0); self.hp.set_freq(exp_map(r, 40.0, 500.0)); }
    fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { *s = self.ws.tick(self.sh.tick(self.hp.tick(*s)) * self.g.next()); } }
}
struct Squash { c: Compressor, mk: Smooth, mix: Mix, wet: Vec<f32> }
impl Processor for Squash {
    fn set(&mut self, v: &Value) {
        let k = num(v, "comp", 0.5);
        self.c.threshold = lerp(-10.0, -45.0, k); self.c.ratio = lerp(2.0, 12.0, k); self.c.knee = 8.0; self.c.attack = lerp(0.001, 0.05, num(v, "attack", 0.3)); self.c.release = 0.18;
        self.mk.set(db_to_gain(k * 6.0 + (num(v, "level", 0.5) - 0.5) * 18.0)); // auto make-up already restores most of the level self.mix.set(num(v, "blend", 1.0), false);
    }
    fn process(&mut self, buf: &mut [f32]) {
        let n = buf.len(); self.wet[..n].copy_from_slice(buf); self.c.process(&mut self.wet[..n]);
        for (s, w) in buf.iter_mut().zip(self.wet.iter()) { *s = self.mix.mix(*s, *w * self.mk.next()); }
    }
}
struct GatePedal(Gate);
impl Processor for GatePedal {
    fn set(&mut self, v: &Value) { self.0.threshold_db = num(v, "threshold", -50.0); self.0.release_ms = num(v, "release", 60.0); }
    fn process(&mut self, buf: &mut [f32]) { self.0.process(buf); }
}
struct Quack { env: Envelope, bp: Biquad, depth: f32, q: f32, mix: Mix, n: u32 }
impl Processor for Quack {
    fn set(&mut self, v: &Value) { self.env.sensitivity = num(v, "sens", 0.5); self.depth = exp_map(num(v, "range", 0.6), 400.0, 3500.0); self.q = lerp(1.5, 9.0, num(v, "q", 0.6)); self.mix.set(num(v, "mix", 1.0), false); }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let e = self.env.tick(*s);
            if self.n % 8 == 0 { self.bp.set(350.0 + self.depth * e, self.q, 0.0); } self.n = self.n.wrapping_add(1);
            let wet = self.bp.tick(*s) * 2.5; *s = self.mix.mix(*s, wet);
        }
    }
}
struct FixedWah { pk: Biquad, lp: Biquad, l: Smooth }
impl Processor for FixedWah {
    fn set(&mut self, v: &Value) { self.pk.set(exp_map(num(v, "pos", 0.55), 350.0, 2400.0), lerp(1.5, 6.0, num(v, "q", 0.5)), 14.0); self.l.set(db_to_gain(-6.0 + (num(v, "level", 0.5) - 0.5) * 18.0)); }
    fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { *s = self.lp.tick(self.pk.tick(*s)) * self.l.next(); } }
}
struct Geq { f: Vec<Biquad>, l: Smooth }
impl Processor for Geq {
    fn set(&mut self, v: &Value) { for (i, b) in self.f.iter_mut().enumerate() { b.set_gain(num(v, &format!("b{i}"), 0.0)); } self.l.set(db_to_gain(num(v, "level", 0.0))); }
    fn process(&mut self, buf: &mut [f32]) { for b in self.f.iter_mut() { b.process(buf); } for s in buf.iter_mut() { *s *= self.l.next(); } }
}
/// Modulated delay: chorus (keep dry, no feedback) and flanger (feedback, equal-power mix).
struct ModDelay { line: DelayLine, lfo: Lfo, base: f32, amt: Smooth, fb: Smooth, lp: Option<Biquad>, mix: Mix, sr: f32, last: f32, chorus: bool }
impl Processor for ModDelay {
    fn set(&mut self, v: &Value) {
        if self.chorus {
            self.lfo.freq.set(exp_map(num(v, "rate", 0.35), 0.1, 6.0)); self.amt.set(lerp(0.0005, 0.006, num(v, "depth", 0.5)) * self.sr); self.mix.set(num(v, "mix", 0.5) * 0.7, true);
        } else {
            self.lfo.freq.set(exp_map(num(v, "rate", 0.25), 0.05, 4.0)); self.amt.set(lerp(0.0003, 0.0028, num(v, "depth", 0.6)) * self.sr);
            self.fb.set(num(v, "feedback", 0.6) * 0.85); self.mix.set(num(v, "mix", 0.5), false);
        }
    }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let d = self.base + self.lfo.next() * self.amt.next();
            let mut wet = self.line.read(d.max(1.0));
            self.line.write(*s + if self.chorus { 0.0 } else { wet * self.fb.next() });
            if let Some(lp) = self.lp.as_mut() { wet = lp.tick(wet); }
            self.last = wet; *s = self.mix.mix(*s, wet);
        }
    }
}
struct Phaser { ap: Vec<Biquad>, lfo: Lfo, depth: f32, fb: Smooth, last: f32, mix: Mix, n: u32 }
impl Processor for Phaser {
    fn set(&mut self, v: &Value) { self.lfo.freq.set(exp_map(num(v, "rate", 0.3), 0.08, 6.0)); self.depth = lerp(100.0, 900.0, num(v, "depth", 0.7)); self.fb.set(num(v, "feedback", 0.3) * 0.7); self.mix.set(0.5, false); }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let l = self.lfo.next();
            if self.n % 16 == 0 { let f = (800.0 + l * self.depth).max(30.0); for a in self.ap.iter_mut() { a.set_freq(f); } } self.n = self.n.wrapping_add(1);
            let mut x = *s + self.last * self.fb.next();
            for a in self.ap.iter_mut() { x = a.tick(x); }
            self.last = x; *s = self.mix.mix(*s, x);
        }
    }
}
struct Tremolo { lfo: Lfo, depth: Smooth }
impl Processor for Tremolo {
    fn set(&mut self, v: &Value) { self.lfo.freq.set(exp_map(num(v, "rate", 0.45), 1.0, 12.0)); self.depth.set(num(v, "depth", 0.6) * 0.5); self.lfo.wave = if num(v, "shape", 0.0) > 0.5 { Wave::Square } else { Wave::Sine }; }
    fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { let d = self.depth.next(); *s *= (1.0 - d) + self.lfo.next() * d; } }
}
/// Feedback echoes: tape (dark, saturated, wow) and crystal (clean digital).
struct Echo { line: DelayLine, time: Smooth, fb: Smooth, lp: Biquad, hp: Option<Biquad>, sat: Option<Shaper>, lfo: Lfo, wow: Smooth, mix: Mix, sr: f32, tape: bool }
impl Processor for Echo {
    fn set(&mut self, v: &Value) {
        if self.tape { self.time.set(num(v, "time", 330.0) / 1000.0 * self.sr); self.fb.set(num(v, "feedback", 0.4) * 0.92); self.wow.set(num(v, "wow", 0.3) * 0.0025 * self.sr); self.mix.set(num(v, "mix", 0.35), true); }
        else { self.time.set(num(v, "time", 450.0) / 1000.0 * self.sr); self.fb.set(num(v, "feedback", 0.35) * 0.95); self.mix.set(num(v, "mix", 0.3), true); }
    }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let d = self.time.next() + self.lfo.next() * self.wow.next();
            let mut wet = self.lp.tick(self.line.read(d));
            if let Some(h) = self.hp.as_mut() { wet = h.tick(wet); }
            if let Some(sat) = self.sat.as_mut() { wet = sat.tick(wet); }
            self.line.write(*s + wet * self.fb.next());
            *s = self.mix.mix(*s, wet);
        }
    }
}
/// Convolution reverb pedal (spring, hall). The kernel is built off the audio thread.
struct ConvVerb { conv: Option<Box<Convolver>>, hp: Biquad, mix: Mix, mix_id: &'static str, mix_def: f32 }
impl Processor for ConvVerb {
    fn set(&mut self, v: &Value) { self.mix.set(num(v, self.mix_id, self.mix_def), true); }
    fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { let wet = match self.conv.as_mut() { Some(c) => c.tick(self.hp.tick(*s)), None => 0.0 }; *s = self.mix.mix(*s, wet); } }
    fn set_kernel(&mut self, c: Box<Convolver>) -> Option<Box<Convolver>> { self.conv.replace(c) }
}
struct Sub(Octave);
impl Processor for Sub { fn set(&mut self, v: &Value) { self.0.down = num(v, "down", 0.6); self.0.dry = num(v, "dry", 0.8); } fn process(&mut self, b: &mut [f32]) { self.0.process(b); } }
struct Lofi { c: Crush, mix: Mix, wet: Vec<f32> }
impl Processor for Lofi {
    fn set(&mut self, v: &Value) { self.c.bits = num(v, "bits", 8.0); self.c.downsample = num(v, "rate", 6.0); self.mix.set(num(v, "mix", 0.6), false); }
    fn process(&mut self, buf: &mut [f32]) { let n = buf.len(); self.wet[..n].copy_from_slice(buf); self.c.process(&mut self.wet[..n]); for (s, w) in buf.iter_mut().zip(self.wet.iter()) { *s = self.mix.mix(*s, *w); } }
}
struct Volume(Smooth);
impl Processor for Volume { fn set(&mut self, v: &Value) { self.0.set(db_to_gain(num(v, "level", 0.0))); } fn process(&mut self, b: &mut [f32]) { for s in b.iter_mut() { *s *= self.0.next(); } } }
struct Ring { lfo: Lfo, mix: Mix }
impl Processor for Ring {
    fn set(&mut self, v: &Value) { self.lfo.freq.set(exp_map(num(v, "freq", 0.4), 30.0, 2000.0)); self.mix.set(num(v, "mix", 0.5), false); }
    fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { let w = *s * self.lfo.next(); *s = self.mix.mix(*s, w); } }
}

pub const MAX_BLOCK: usize = 4096;

/// Create a built-in pedal by catalog id.
pub fn builtin(id: &str, sr: f32) -> Option<Box<dyn Processor>> {
    let bq = |k, f, q, g| Biquad::new(sr, k, f, q, g);
    let dc = |pre, pre_f, pre_q, pre_g, hp, kind, max_gain, tone, level_db, d_def, tone_def, level_def| DriveCfg { pre, pre_f, pre_q, pre_g, hp, kind, max_gain, tone, level_db, d_def, tone_def, level_def };
    let p: Box<dyn Processor> = match id {
        "gc.verdant" => Box::new(Drive::new(sr, dc(Kind::Peaking, 720.0, 0.9, 7.0, 320.0, Curve::Diode, 40.0, (1200.0, 6500.0), 2.0, 0.35, 0.55, 0.6))),
        "gc.klar" => Box::new(Drive::new(sr, dc(Kind::Peaking, 900.0, 0.9, 2.0, 90.0, Curve::Soft, 18.0, (2000.0, 11000.0), 0.0, 0.3, 0.5, 0.55))),
        "gc.ironclad" => Box::new(Drive::new(sr, dc(Kind::Peaking, 1000.0, 0.9, 3.0, 120.0, Curve::Hard, 120.0, (900.0, 9000.0), -6.0, 0.6, 0.5, 0.5))),
        "gc.fuzz-factory" => Box::new(fuzz_factory::FuzzFactory::new(sr)),
        "gc.molten" => Box::new(Drive::new(sr, dc(Kind::Lowshelf, 200.0, 0.7, 4.0, 60.0, Curve::Fuzz, 200.0, (700.0, 5000.0), -8.0, 0.7, 0.4, 0.5))),
        "gc.crown" => Box::new(Drive::new(sr, dc(Kind::Peaking, 1500.0, 0.7, 4.0, 100.0, Curve::Asym, 50.0, (1500.0, 9000.0), 0.0, 0.5, 0.55, 0.5))),
        "gc.amber" => Box::new(Boost { hp: bq(Kind::Highpass, 40.0, 0.6, 0.0), sh: bq(Kind::Highshelf, 1500.0, 0.7, 0.0), g: Smooth::new(1.0, sr, 0.02), ws: Shaper::new(Curve::Soft, 0.15) }),
        "gc.squash" => Box::new(Squash { c: Compressor::new(sr), mk: Smooth::new(1.0, sr, 0.02), mix: Mix::new(sr), wet: vec![0.0; MAX_BLOCK] }),
        "gc.silencer" => Box::new(GatePedal(Gate::new(sr))),
        "gc.quack" => Box::new(Quack { env: Envelope::new(sr), bp: bq(Kind::Bandpass, 350.0, 4.0, 0.0), depth: 1500.0, q: 4.0, mix: Mix::new(sr), n: 0 }),
        "gc.cocked" => Box::new(FixedWah { pk: bq(Kind::Peaking, 900.0, 3.0, 14.0), lp: bq(Kind::Lowpass, 5000.0, 0.7, 0.0), l: Smooth::new(0.5, sr, 0.02) }),
        "gc.geq" => Box::new(Geq { f: [100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0].iter().map(|f| bq(Kind::Peaking, *f, 1.4, 0.0)).collect(), l: Smooth::new(1.0, sr, 0.02) }),
        "gc.clarinot-clean" => Box::new(clarinot::Clarinot::new(sr)),
        "gc.lagoon" => Box::new(ModDelay { line: DelayLine::new((sr * 0.06) as usize), lfo: Lfo::new(sr, 0.5, Wave::Triangle), base: 0.012 * sr, amt: Smooth::new(0.003 * sr, sr, 0.02), fb: Smooth::new(0.0, sr, 0.02), lp: Some(bq(Kind::Lowpass, 6000.0, 0.7, 0.0)), mix: Mix::new(sr), sr, last: 0.0, chorus: true }),
        "gc.jet" => Box::new(ModDelay { line: DelayLine::new((sr * 0.03) as usize), lfo: Lfo::new(sr, 0.3, Wave::Triangle), base: 0.003 * sr, amt: Smooth::new(0.002 * sr, sr, 0.02), fb: Smooth::new(0.5, sr, 0.02), lp: None, mix: Mix::new(sr), sr, last: 0.0, chorus: false }),
        "gc.orbit" => Box::new(Phaser { ap: (0..4).map(|_| bq(Kind::Allpass, 800.0, 0.7, 0.0)).collect(), lfo: Lfo::new(sr, 0.5, Wave::Sine), depth: 600.0, fb: Smooth::new(0.3, sr, 0.02), last: 0.0, mix: Mix::new(sr), n: 0 }),
        "gc.pulse" => Box::new(Tremolo { lfo: Lfo::new(sr, 4.0, Wave::Sine), depth: Smooth::new(0.3, sr, 0.02) }),
        "gc.tapeecho" => Box::new(Echo { line: DelayLine::new((sr * 2.0) as usize), time: Smooth::new(0.33 * sr, sr, 0.1), fb: Smooth::new(0.37, sr, 0.02), lp: bq(Kind::Lowpass, 2800.0, 0.7, 0.0), hp: Some(bq(Kind::Highpass, 200.0, 0.6, 0.0)), sat: Some(Shaper::new(Curve::Soft, 0.3)), lfo: Lfo::new(sr, 0.9, Wave::Sine), wow: Smooth::new(0.0, sr, 0.02), mix: Mix::new(sr), sr, tape: true }),
        "gc.crystal" => Box::new(Echo { line: DelayLine::new((sr * 3.0) as usize), time: Smooth::new(0.45 * sr, sr, 0.08), fb: Smooth::new(0.33, sr, 0.02), lp: bq(Kind::Lowpass, 9000.0, 0.6, 0.0), hp: None, sat: None, lfo: Lfo::new(sr, 0.1, Wave::Sine), wow: Smooth::new(0.0, sr, 0.02), mix: Mix::new(sr), sr, tape: false }),
        "gc.spring" => Box::new(ConvVerb { conv: None, hp: bq(Kind::Highpass, 300.0, 0.7, 0.0), mix: Mix::new(sr), mix_id: "mix", mix_def: 0.35 }),
        "gc.cathedral" => Box::new(ConvVerb { conv: None, hp: bq(Kind::Highpass, 250.0, 0.7, 0.0), mix: Mix::new(sr), mix_id: "mix", mix_def: 0.4 }),
        "gc.sub" => Box::new(Sub(Octave::new(sr))),
        "gc.lofi" => Box::new(Lofi { c: Crush::new(), mix: Mix::new(sr), wet: vec![0.0; MAX_BLOCK] }),
        "gc.volume" => Box::new(Volume(Smooth::new(1.0, sr, 0.02))),
        "gc.ring" => Box::new(Ring { lfo: Lfo::new(sr, 200.0, Wave::Sine), mix: Mix::new(sr) }),
        _ => return None,
    };
    Some(p)
}

/// Reverb kernel needed by a pedal or core block: (cache key, mono IR). None = no kernel for this block.
pub fn kernel_spec(kind: &str, v: &Value, sr: f32) -> Option<(String, Box<dyn FnOnce() -> Vec<f32> + Send>)> {
    use crate::ir::{reverb_ir, ReverbKind};
    match kind {
        "gc.spring" => { let (d, t) = (num(v, "dwell", 0.5), num(v, "tone", 0.5)); Some((format!("spr:{d:.2}:{t:.2}"), Box::new(move || reverb_ir(sr, lerp(0.8, 3.5, d), 0.008, t, ReverbKind::Spring).0))) }
        "gc.cathedral" => { let (d, t) = (num(v, "decay", 0.6), num(v, "tone", 0.6)); Some((format!("cat:{d:.2}:{t:.2}"), Box::new(move || reverb_ir(sr, lerp(2.0, 9.0, d), 0.035, t, ReverbKind::Hall).0))) }
        "reverb" => { let (d, p, t) = (num(v, "decay", 2.2), num(v, "predelay", 15.0), num(v, "tone", 0.55)); let k = txt(v, "kind", "hall").to_string();
            Some((format!("rev:{d:.2}:{p:.0}:{t:.2}:{k}"), Box::new(move || reverb_ir(sr, d, p / 1000.0, t, match k.as_str() { "spring" => ReverbKind::Spring, "room" => ReverbKind::Room, _ => ReverbKind::Hall }).0))) }
        _ => None,
    }
}

// ------------------------------------------------------------------ builder pedals (.gcpedal.json)
fn resolve(v: &Value, knobs: &Value, def: f32) -> f32 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(def as f64) as f32,
        Value::Object(o) if o.contains_key("knob") => {
            let id = o.get("knob").and_then(|x| x.as_str()).unwrap_or("");
            let t = knobs.get(id).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(0.5);
            let from = o.get("from").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32; let to = o.get("to").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
            if o.get("exp").and_then(|x| x.as_bool()).unwrap_or(false) && from > 0.0 { exp_map(t, from, to) } else { lerp(from, to, t) }
        }
        _ => def,
    }
}
enum Blk { Gain(Smooth), Filter(Biquad), Drive(Smooth, Shaper, Smooth), Delay(DelayLine, Smooth, Smooth, Mix), Trem(Lfo, Smooth), Chorus(DelayLine, Lfo, Smooth, Mix), Ring(Lfo, Mix) }
pub struct BlockPedal { def: Value, blocks: Vec<Blk>, sr: f32, defaults: Value }
impl BlockPedal {
    pub fn new(def: Value, sr: f32) -> Self {
        let mut blocks = vec![];
        for b in def.get("blocks").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
            let t = txt(&b, "type", "");
            blocks.push(match t {
                "gain" => Blk::Gain(Smooth::new(1.0, sr, 0.02)),
                "filter" => Blk::Filter(Biquad::new(sr, Kind::parse(txt(&b, "mode", "lowpass")), 1000.0, 0.7, 0.0)),
                "drive" => Blk::Drive(Smooth::new(8.0, sr, 0.02), Shaper::new(Curve::parse(txt(&b, "curve", "soft")), 1.0), Smooth::new(0.5, sr, 0.02)),
                "delay" => Blk::Delay(DelayLine::new((sr * 2.0) as usize), Smooth::new(0.3 * sr, sr, 0.08), Smooth::new(0.3, sr, 0.02), Mix::new(sr)),
                "tremolo" => Blk::Trem(Lfo::new(sr, 5.0, Wave::Sine), Smooth::new(0.25, sr, 0.02)),
                "chorus" => Blk::Chorus(DelayLine::new((sr * 0.06) as usize), Lfo::new(sr, 0.8, Wave::Sine), Smooth::new(0.0, sr, 0.02), Mix::new(sr)),
                "ring" => Blk::Ring(Lfo::new(sr, 200.0, Wave::Sine), Mix::new(sr)),
                _ => continue,
            });
        }
        let mut defaults = serde_json::Map::new();
        for k in def.get("knobs").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
            if let Some(id) = k.get("id").and_then(|x| x.as_str()) { defaults.insert(id.into(), k.get("def").cloned().unwrap_or(Value::from(0.5))); }
        }
        let mut s = Self { def, blocks, sr, defaults: Value::Object(defaults) };
        let d = s.defaults.clone(); s.set(&d); s
    }
}
impl Processor for BlockPedal {
    fn set(&mut self, v: &Value) {
        let mut knobs = self.defaults.clone();
        if let (Some(k), Some(o)) = (knobs.as_object_mut(), v.as_object()) { for (a, b) in o { k.insert(a.clone(), b.clone()); } }
        let defs = self.def.get("blocks").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let defs: Vec<&Value> = defs.iter().filter(|b| ["gain", "filter", "drive", "delay", "tremolo", "chorus", "ring"].contains(&txt(b, "type", ""))).collect();
        let sr = self.sr;
        for (blk, b) in self.blocks.iter_mut().zip(defs.iter()) {
            let r = |k: &str, d: f32| resolve(b.get(k).unwrap_or(&Value::Null), &knobs, d);
            match blk {
                Blk::Gain(g) => g.set(db_to_gain(r("db", 0.0))),
                Blk::Filter(f) => f.set(r("freq", 1000.0), r("q", 0.7), r("gain", 0.0)),
                Blk::Drive(g, _, n) => { let a = r("amount", 8.0); g.set(a); n.set(1.0 / (a / 2.0).sqrt().max(1.0)); }
                Blk::Delay(_, t, fb, m) => { t.set(r("time", 300.0) / 1000.0 * sr); fb.set(r("feedback", 0.3)); m.set(r("mix", 0.3), true); }
                Blk::Trem(l, d) => { l.freq.set(r("rate", 5.0)); d.set(r("depth", 0.5) / 2.0); }
                Blk::Chorus(_, l, d, m) => { l.freq.set(r("rate", 0.8)); d.set(r("depth", 0.5) * 0.005 * sr); m.set(r("mix", 0.5), true); }
                Blk::Ring(l, m) => { l.freq.set(r("freq", 200.0)); m.set(r("mix", 0.5), false); }
            }
        }
    }
    fn process(&mut self, buf: &mut [f32]) {
        let sr = self.sr;
        for blk in self.blocks.iter_mut() {
            match blk {
                Blk::Gain(g) => for s in buf.iter_mut() { *s *= g.next(); },
                Blk::Filter(f) => f.process(buf),
                Blk::Drive(g, ws, n) => for s in buf.iter_mut() { *s = ws.tick(*s * g.next()) * n.next(); },
                Blk::Delay(line, t, fb, m) => for s in buf.iter_mut() { let w = line.read(t.next()); line.write(*s + w * fb.next()); *s = m.mix(*s, w); },
                Blk::Trem(l, d) => for s in buf.iter_mut() { let dd = d.next(); *s *= (1.0 - dd) + l.next() * dd; },
                Blk::Chorus(line, l, d, m) => for s in buf.iter_mut() { line.write(*s); let w = line.read(0.012 * sr + l.next() * d.next()); *s = m.mix(*s, w); },
                Blk::Ring(l, m) => for s in buf.iter_mut() { let w = *s * l.next(); *s = m.mix(*s, w); },
            }
        }
    }
}
