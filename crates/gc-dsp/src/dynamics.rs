//! Gate, compressor, limiter, envelope follower; octave, pitch shifter, bitcrusher.
use crate::util::db_to_gain;

pub struct Gate { env: f32, g: f32, open: bool, hold: f32, sr: f32, pub threshold_db: f32, pub release_ms: f32, pub active: bool }
impl Gate {
    pub fn new(sr: f32) -> Self { Self { env: 0.0, g: 1.0, open: false, hold: 0.0, sr, threshold_db: -60.0, release_ms: 80.0, active: true } }
    pub fn is_open(&self) -> bool { self.open }
    pub fn process(&mut self, buf: &mut [f32]) {
        let thr = db_to_gain(self.threshold_db); let close = thr * 0.6;
        let rel = (-1.0 / (self.sr * self.release_ms / 1000.0)).exp();
        let atk = (-1.0 / (self.sr * 0.0008)).exp();
        let env_rel = (-1.0 / (self.sr * 0.012)).exp();
        let hold_n = self.sr * 0.03;
        for s in buf.iter_mut() {
            let pk = s.abs();
            self.env = if pk > self.env { pk } else { self.env * env_rel + pk * (1.0 - env_rel) };
            if self.env > thr { self.open = true; self.hold = hold_n; } else if self.env < close { if self.hold > 0.0 { self.hold -= 1.0; } else { self.open = false; } }
            let target = if !self.active || self.open { 1.0 } else { 0.0 };
            let c = if target > self.g { atk } else { rel };
            self.g = target + (self.g - target) * c;
            *s *= self.g;
        }
    }
}

/// Feed-forward compressor with soft knee (stands in for `DynamicsCompressorNode`, incl. its auto make-up).
pub struct Compressor { env_db: f32, sr: f32, pub threshold: f32, pub ratio: f32, pub knee: f32, pub attack: f32, pub release: f32, pub auto_makeup: bool }
impl Compressor {
    pub fn new(sr: f32) -> Self { Self { env_db: -120.0, sr, threshold: -24.0, ratio: 12.0, knee: 30.0, attack: 0.003, release: 0.25, auto_makeup: true } }
    fn curve(&self, x_db: f32) -> f32 {
        let (t, r, k) = (self.threshold, self.ratio.max(1.0), self.knee.max(0.0));
        if k > 0.0 && (x_db - t).abs() <= k / 2.0 { x_db + (1.0 / r - 1.0) * (x_db - t + k / 2.0).powi(2) / (2.0 * k) }
        else if x_db > t { t + (x_db - t) / r } else { x_db }
    }
    pub fn process(&mut self, buf: &mut [f32]) {
        let a = (-1.0 / (self.sr * self.attack.max(1e-4))).exp(); let rl = (-1.0 / (self.sr * self.release.max(1e-3))).exp();
        let makeup = if self.auto_makeup { db_to_gain(-(self.curve(0.0)) * 0.6) } else { 1.0 };
        for s in buf.iter_mut() {
            let x_db = 20.0 * (s.abs() + 1e-9).log10();
            let gr = self.curve(x_db) - x_db; // ≤ 0
            let c = if gr < self.env_db { a } else { rl };
            self.env_db = gr + (self.env_db - gr) * c;
            *s *= db_to_gain(self.env_db) * makeup;
        }
    }
}

/// Brick-wall-ish output limiter (−1.5 dBFS), 1 ms attack, 80 ms release.
pub struct Limiter { g: f32, sr: f32, pub ceiling: f32 }
impl Limiter {
    pub fn new(sr: f32) -> Self { Self { g: 1.0, sr, ceiling: db_to_gain(-1.5) } }
    #[inline] pub fn tick(&mut self, x: f32) -> f32 {
        // brickwall: gain drops instantly to what the sample needs, recovers with an 80 ms release
        let r = (-1.0 / (self.sr * 0.08)).exp();
        let need = if x.abs() > self.ceiling { self.ceiling / x.abs() } else { 1.0 };
        self.g = if need < self.g { need } else { need + (self.g - need) * r };
        (x * self.g).clamp(-1.0, 1.0)
    }
}

pub struct Envelope { env: f32, sr: f32, pub sensitivity: f32, pub attack_ms: f32, pub release_ms: f32 }
impl Envelope {
    pub fn new(sr: f32) -> Self { Self { env: 0.0, sr, sensitivity: 0.5, attack_ms: 5.0, release_ms: 120.0 } }
    #[inline] pub fn tick(&mut self, x: f32) -> f32 {
        let a = (-1.0 / (self.sr * self.attack_ms / 1000.0)).exp(); let r = (-1.0 / (self.sr * self.release_ms / 1000.0)).exp();
        let v = x.abs() * (1.0 + self.sensitivity * 40.0);
        let c = if v > self.env { a } else { r };
        self.env = v + (self.env - v) * c; self.env.min(1.0)
    }
}

pub struct Octave { sign: f32, last: f32, env: f32, lp: f32, lp2: f32, k: f32, pub down: f32, pub dry: f32 }
impl Octave {
    pub fn new(sr: f32) -> Self { Self { sign: 1.0, last: 0.0, env: 0.0, lp: 0.0, lp2: 0.0, k: 1.0 - (-2.0 * std::f32::consts::PI * 900.0 / sr).exp(), down: 0.6, dry: 0.8 } }
    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let x = *s;
            self.lp += self.k * (x - self.lp);
            if self.last <= 0.0 && self.lp > 0.0 { self.sign = -self.sign; }
            self.last = self.lp;
            let ax = x.abs(); self.env = if ax > self.env { ax } else { self.env * 0.9995 };
            self.lp2 += self.k * 0.5 * (self.sign * self.env - self.lp2);
            *s = x * self.dry + self.lp2 * self.down * 1.6;
        }
    }
}

/// Two read heads sweeping a circular buffer with Hann crossfade (global Transpose).
pub struct PitchShift { buf: Vec<f32>, w: usize, phase: f32, win: f32, pub semitones: i32 }
impl PitchShift {
    pub fn new(sr: f32) -> Self { Self { buf: vec![0.0; 8192], w: 0, phase: 0.0, win: (sr * 0.045).floor(), semitones: 0 } }
    pub fn process(&mut self, x: &mut [f32]) {
        if self.semitones == 0 { for s in x.iter() { self.buf[self.w] = *s; self.w = (self.w + 1) % self.buf.len(); } return; }
        let ratio = 2f32.powf(self.semitones as f32 / 12.0); let step = (1.0 - ratio) / self.win; let n = self.buf.len();
        for s in x.iter_mut() {
            self.buf[self.w] = *s;
            self.phase += step; if self.phase >= 1.0 { self.phase -= 1.0; } if self.phase < 0.0 { self.phase += 1.0; }
            let mut y = 0.0;
            for ph in [self.phase, (self.phase + 0.5) % 1.0] {
                let d = ph * self.win + 2.0; let mut r = self.w as f32 - d; while r < 0.0 { r += n as f32; }
                let i0 = r.floor() as usize; let fr = r - r.floor();
                y += (self.buf[i0 % n] * (1.0 - fr) + self.buf[(i0 + 1) % n] * fr) * (std::f32::consts::PI * ph).sin().powi(2);
            }
            self.w = (self.w + 1) % n;
            *s = y;
        }
    }
}

pub struct Crush { cnt: u32, hold: f32, pub bits: f32, pub downsample: f32 }
impl Crush {
    pub fn new() -> Self { Self { cnt: 0, hold: 0.0, bits: 8.0, downsample: 4.0 } }
    pub fn process(&mut self, buf: &mut [f32]) {
        let step = 2f32.powf(self.bits - 1.0); let ds = self.downsample.round().max(1.0) as u32;
        for s in buf.iter_mut() { if self.cnt % ds == 0 { self.hold = (*s * step).round() / step; } self.cnt = self.cnt.wrapping_add(1); *s = self.hold; }
    }
}
