//! Core blocks of the chain: amp (4 channels), cab, EQ, delay, reverb. All mono, block-processed.
use crate::biquad::{Biquad, Kind};
use crate::conv::{Convolver, DelayLine};
use crate::shaper::{Curve, Shaper};
use crate::util::*;
use serde_json::Value;

/// Every block in the chain (core blocks and pedals) implements this.
pub trait Processor: Send {
    /// Apply parameters. `v` is the block's own JSON: a pedal's `params`, or `rig.amp`, `rig.cab`, `rig.eq`, …
    fn set(&mut self, v: &Value);
    fn process(&mut self, buf: &mut [f32]);
    /// Receive a prebuilt convolution kernel (built off the audio thread). Returns the old one for disposal.
    fn set_kernel(&mut self, _c: Box<Convolver>) -> Option<Box<Convolver>> { None }
}

pub fn num(v: &Value, k: &str, d: f32) -> f32 { v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(d) }
pub fn txt<'a>(v: &'a Value, k: &str, d: &'a str) -> &'a str { v.get(k).and_then(|x| x.as_str()).unwrap_or(d) }

/// Dry/wet with smoothing; `keep_dry` keeps the dry path at unity (delays, reverbs).
pub struct Mix { dry: Smooth, wet: Smooth }
impl Mix {
    pub fn new(sr: f32) -> Self { Self { dry: Smooth::new(1.0, sr, 0.02), wet: Smooth::new(0.0, sr, 0.02) } }
    pub fn set(&mut self, m: f32, keep_dry: bool) {
        if keep_dry { self.dry.set(1.0); self.wet.set(m); } else { self.dry.set((m * std::f32::consts::FRAC_PI_2).cos()); self.wet.set((m * std::f32::consts::FRAC_PI_2).sin()); }
    }
    #[inline] pub fn mix(&mut self, dry: f32, wet: f32) -> f32 { dry * self.dry.next() + wet * self.wet.next() }
}

// ------------------------------------------------------------------ amp
pub struct Channel { pub id: &'static str, pub name: &'static str, stages: [(f32, f32, Curve); 3], hp: f32, lp: f32, mid: f32, bright: f32, out: f32 }
pub const CHANNELS: [Channel; 4] = [
    Channel { id: "clean", name: "Clean", stages: [(1.0, 0.8, Curve::Tube), (1.0, 0.5, Curve::Soft), (1.0, 0.3, Curve::Soft)], hp: 60.0, lp: 9000.0, mid: 700.0, bright: 4.0, out: 1.0 },
    Channel { id: "crunch", name: "Crunch", stages: [(6.0, 1.4, Curve::Asym), (3.0, 1.0, Curve::Tube), (1.0, 0.4, Curve::Soft)], hp: 80.0, lp: 7500.0, mid: 800.0, bright: 2.0, out: 0.65 },
    Channel { id: "rhythm", name: "Rhythm", stages: [(14.0, 2.0, Curve::Asym), (6.0, 1.6, Curve::Tube), (3.0, 1.2, Curve::Soft)], hp: 140.0, lp: 6500.0, mid: 650.0, bright: 1.0, out: 0.6 },
    Channel { id: "lead", name: "Lead", stages: [(18.0, 2.2, Curve::Asym), (8.0, 1.8, Curve::Tube), (4.0, 1.4, Curve::Soft)], hp: 110.0, lp: 6000.0, mid: 900.0, bright: 0.0, out: 0.48 },
];

pub struct Amp {
    hp: Biquad, bright: Biquad, g: [Smooth; 3], ws: [Shaper; 3], cpl: [Biquad; 3], lp: [Biquad; 3], norm: [Smooth; 3],
    bass: Biquad, mid: Biquad, treble: Biquad, power_drive: Smooth, power: Shaper, presence: Biquad, resonance: Biquad, level: Smooth,
}
impl Amp {
    pub fn new(sr: f32) -> Self {
        let bq = |k, f, q, g| Biquad::new(sr, k, f, q, g);
        let sm = |v| Smooth::new(v, sr, 0.02);
        Self {
            hp: bq(Kind::Highpass, 80.0, 0.7, 0.0), bright: bq(Kind::Highshelf, 2500.0, 0.7, 0.0),
            g: [sm(1.0), sm(1.0), sm(1.0)], ws: [Shaper::new(Curve::Soft, 1.0), Shaper::new(Curve::Soft, 1.0), Shaper::new(Curve::Soft, 1.0)],
            cpl: [bq(Kind::Highpass, 30.0, 0.6, 0.0), bq(Kind::Highpass, 60.0, 0.6, 0.0), bq(Kind::Highpass, 40.0, 0.6, 0.0)],
            lp: [bq(Kind::Lowpass, 9000.0, 0.6, 0.0), bq(Kind::Lowpass, 8000.0, 0.6, 0.0), bq(Kind::Lowpass, 7000.0, 0.6, 0.0)],
            norm: [sm(0.7), sm(0.7), sm(0.7)],
            bass: bq(Kind::Lowshelf, 120.0, 0.7, 0.0), mid: bq(Kind::Peaking, 700.0, 0.8, 0.0), treble: bq(Kind::Highshelf, 3200.0, 0.7, 0.0),
            power_drive: sm(1.0), power: Shaper::new(Curve::Tube, 0.6), presence: bq(Kind::Highshelf, 4500.0, 0.7, 0.0), resonance: bq(Kind::Peaking, 95.0, 1.2, 2.0), level: sm(1.0),
        }
    }
}
impl Processor for Amp {
    /// `v` = rig.amp: { channel, channels: { clean: {gain, bass, …}, … } }
    fn set(&mut self, v: &Value) {
        let ch_id = txt(v, "channel", "crunch");
        let c = CHANNELS.iter().find(|c| c.id == ch_id).unwrap_or(&CHANNELS[1]);
        let k = v.get("channels").and_then(|x| x.get(ch_id)).cloned().unwrap_or(Value::Null);
        let gn = num(&k, "gain", 0.5);
        self.hp.set_freq(c.hp);
        self.bright.set_gain(c.bright * (1.0 - gn * 0.6));
        for i in 0..3 {
            let (sg, amt, kind) = c.stages[i];
            let drive = if i == 0 { lerp(1.0, sg, gn.powf(1.3)) } else { lerp(1.0, sg, gn * 0.8 + 0.2) };
            self.g[i].set(drive); self.ws[i].kind = kind; self.ws[i].amount = amt;
            self.lp[i].set_freq(c.lp - i as f32 * 600.0);
            self.norm[i].set(1.0 / drive.sqrt() * 0.95);
        }
        self.cpl[0].set_freq(match ch_id { "rhythm" => 160.0, "lead" => 120.0, _ => 40.0 });
        self.bass.set_gain((num(&k, "bass", 0.5) - 0.5) * 24.0);
        self.mid.set(c.mid, 0.8, (num(&k, "mid", 0.5) - 0.5) * 20.0);
        self.treble.set_gain((num(&k, "treble", 0.5) - 0.5) * 20.0);
        self.presence.set_gain((num(&k, "presence", 0.5) - 0.5) * 14.0);
        let m = num(&k, "master", 0.5);
        self.power_drive.set(lerp(0.3, 1.6, m));
        self.level.set(c.out * lerp(0.5, 1.15, m) / lerp(0.3, 1.6, m) * 1.2);
    }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let mut x = self.bright.tick(self.hp.tick(*s));
            for i in 0..3 { x = self.ws[i].tick(x * self.g[i].next()); x = self.lp[i].tick(self.cpl[i].tick(x)) * self.norm[i].next(); }
            x = self.treble.tick(self.mid.tick(self.bass.tick(x)));
            x = self.power.tick(x * self.power_drive.next());
            x = self.resonance.tick(self.presence.tick(x));
            *s = x * self.level.next();
        }
    }
}

// ------------------------------------------------------------------ cab
pub struct Cab { conv: Option<Box<Convolver>>, level: Smooth }
impl Cab { pub fn new(sr: f32) -> Self { Self { conv: None, level: Smooth::new(1.0, sr, 0.02) } } }
impl Processor for Cab {
    fn set(&mut self, v: &Value) { self.level.set(db_to_gain((num(v, "level", 0.5) - 0.5) * 24.0)); }
    fn process(&mut self, buf: &mut [f32]) {
        if let Some(c) = self.conv.as_mut() { c.process(buf); }
        for s in buf.iter_mut() { *s *= self.level.next(); }
    }
    fn set_kernel(&mut self, c: Box<Convolver>) -> Option<Box<Convolver>> { self.conv.replace(c) }
}

// ------------------------------------------------------------------ eq
pub struct Eq { pub f: Vec<Biquad> }
impl Eq {
    pub fn new(sr: f32) -> Self {
        let bq = |k, f, q| Biquad::new(sr, k, f, q, 0.0);
        Self { f: vec![bq(Kind::Highpass, 20.0, 0.71), bq(Kind::Highpass, 20.0, 0.71), bq(Kind::Lowshelf, 100.0, 0.7), bq(Kind::Peaking, 400.0, 1.0),
            bq(Kind::Peaking, 1000.0, 1.0), bq(Kind::Peaking, 2500.0, 1.0), bq(Kind::Highshelf, 6000.0, 0.7), bq(Kind::Lowpass, 20000.0, 0.71)] }
    }
    /// Sum of all bands in dB at `freq` (used for the EQ curve in the UI).
    pub fn response_db(&self, freq: f32) -> f32 { self.f.iter().map(|b| b.magnitude_db(freq)).sum() }
}
impl Processor for Eq {
    fn set(&mut self, v: &Value) {
        let lc = num(v, "lowcut", 20.0); self.f[0].set_freq(lc); self.f[1].set_freq(lc);
        for (i, k) in ["low", "lowmid", "mid", "highmid", "high"].iter().enumerate() { self.f[2 + i].set_gain(num(v, k, 0.0)); }
        self.f[7].set_freq(num(v, "highcut", 20000.0));
    }
    fn process(&mut self, buf: &mut [f32]) { for b in self.f.iter_mut() { b.process(buf); } }
}

// ------------------------------------------------------------------ delay
pub struct DelayFx { line: DelayLine, fb: Smooth, time: Smooth, lp: Biquad, hp: Biquad, lfo: Lfo, depth: Smooth, mix: Mix, sr: f32 }
impl DelayFx {
    pub fn new(sr: f32) -> Self {
        Self { line: DelayLine::new((sr * 3.2) as usize), fb: Smooth::new(0.35, sr, 0.02), time: Smooth::new(0.38 * sr, sr, 0.08),
            lp: Biquad::new(sr, Kind::Lowpass, 5000.0, 0.5, 0.0), hp: Biquad::new(sr, Kind::Highpass, 120.0, 0.5, 0.0),
            lfo: Lfo::new(sr, 0.6, Wave::Sine), depth: Smooth::new(0.0, sr, 0.02), mix: Mix::new(sr), sr }
    }
}
impl Processor for DelayFx {
    fn set(&mut self, v: &Value) {
        self.time.set(num(v, "time", 380.0) / 1000.0 * self.sr);
        self.fb.set(num(v, "feedback", 0.35));
        self.lp.set_freq(exp_map(num(v, "tone", 0.5), 1500.0, 12000.0));
        self.depth.set(num(v, "mod", 0.15) * 0.003 * self.sr);
        self.mix.set(num(v, "mix", 0.25), true);
    }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let d = self.time.next() + self.lfo.next() * self.depth.next();
            let wet = self.hp.tick(self.lp.tick(self.line.read(d)));
            self.line.write(*s + wet * self.fb.next());
            *s = self.mix.mix(*s, wet);
        }
    }
}

// ------------------------------------------------------------------ reverb
pub struct ReverbFx { conv: Option<Box<Convolver>>, hp: Biquad, mix: Mix }
impl ReverbFx { pub fn new(sr: f32) -> Self { Self { conv: None, hp: Biquad::new(sr, Kind::Highpass, 200.0, 0.6, 0.0), mix: Mix::new(sr) } } }
impl Processor for ReverbFx {
    fn set(&mut self, v: &Value) { self.hp.set_freq(num(v, "lowcut", 200.0)); self.mix.set(num(v, "mix", 0.2), true); }
    fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let wet = match self.conv.as_mut() { Some(c) => c.tick(self.hp.tick(*s)), None => 0.0 };
            *s = self.mix.mix(*s, wet);
        }
    }
    fn set_kernel(&mut self, c: Box<Convolver>) -> Option<Box<Convolver>> { self.conv.replace(c) }
}
