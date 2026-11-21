//! RBJ biquads with the exact Web Audio `BiquadFilterNode` semantics
//! (lowpass/highpass Q in dB, shelves with S = 1), so tuned sounds carry over.
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Lowpass, Highpass, Bandpass, Peaking, Lowshelf, Highshelf, Notch, Allpass }

impl Kind {
    pub fn parse(s: &str) -> Kind {
        match s { "highpass" => Kind::Highpass, "bandpass" => Kind::Bandpass, "peaking" => Kind::Peaking, "lowshelf" => Kind::Lowshelf,
            "highshelf" => Kind::Highshelf, "notch" => Kind::Notch, "allpass" => Kind::Allpass, _ => Kind::Lowpass }
    }
}

#[derive(Clone, Debug)]
pub struct Biquad {
    pub kind: Kind, sr: f64,
    b0: f64, b1: f64, b2: f64, a1: f64, a2: f64,
    z1: f64, z2: f64,
    cur: (f32, f32, f32),
}

impl Biquad {
    pub fn new(sr: f32, kind: Kind, f: f32, q: f32, gain_db: f32) -> Self {
        let mut b = Biquad { kind, sr: sr as f64, b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0, cur: (f32::NAN, 0.0, 0.0) };
        b.set(f, q, gain_db); b
    }
    /// Recompute coefficients (cheap; skipped when unchanged).
    pub fn set(&mut self, f: f32, q: f32, gain_db: f32) {
        if self.cur == (f, q, gain_db) { return; }
        self.cur = (f, q, gain_db);
        let (b0, b1, b2, a0, a1, a2) = coefs(self.kind, self.sr, f as f64, q as f64, gain_db as f64);
        self.b0 = b0 / a0; self.b1 = b1 / a0; self.b2 = b2 / a0; self.a1 = a1 / a0; self.a2 = a2 / a0;
    }
    pub fn set_freq(&mut self, f: f32) { let (_, q, g) = self.cur; self.set(f, q, g); }
    pub fn set_gain(&mut self, g: f32) { let (f, q, _) = self.cur; self.set(f, q, g); }
    pub fn set_q(&mut self, q: f32) { let (f, _, g) = self.cur; self.set(f, q, g); }
    pub fn freq(&self) -> f32 { self.cur.0 }
    #[inline] pub fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }
    pub fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { *s = self.tick(*s); } }
    pub fn reset(&mut self) { self.z1 = 0.0; self.z2 = 0.0; }
    /// Magnitude response in dB at frequency `f` (for EQ curves).
    pub fn magnitude_db(&self, f: f32) -> f32 {
        let w = 2.0 * PI * f as f64 / self.sr;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2; let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2; let di = -(self.a1 * s1 + self.a2 * s2);
        (10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).max(1e-30).log10()) as f32
    }
}

fn coefs(kind: Kind, sr: f64, f: f64, q: f64, g: f64) -> (f64, f64, f64, f64, f64, f64) {
    let f = f.clamp(1.0, sr * 0.4999);
    let w0 = 2.0 * PI * f / sr; let (cs, sn) = (w0.cos(), w0.sin());
    let a = 10f64.powf(g / 40.0);
    let alpha_q = sn / (2.0 * q.max(1e-4));
    let alpha_qdb = sn / (2.0 * 10f64.powf(q / 20.0));
    let alpha_s = sn / 2.0 * 2f64.sqrt();
    let sa = 2.0 * a.sqrt() * alpha_s;
    match kind {
        Kind::Lowpass => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0, 1.0 + alpha_qdb, -2.0 * cs, 1.0 - alpha_qdb),
        Kind::Highpass => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0, 1.0 + alpha_qdb, -2.0 * cs, 1.0 - alpha_qdb),
        Kind::Bandpass => (alpha_q, 0.0, -alpha_q, 1.0 + alpha_q, -2.0 * cs, 1.0 - alpha_q),
        Kind::Notch => (1.0, -2.0 * cs, 1.0, 1.0 + alpha_q, -2.0 * cs, 1.0 - alpha_q),
        Kind::Allpass => (1.0 - alpha_q, -2.0 * cs, 1.0 + alpha_q, 1.0 + alpha_q, -2.0 * cs, 1.0 - alpha_q),
        Kind::Peaking => (1.0 + alpha_q * a, -2.0 * cs, 1.0 - alpha_q * a, 1.0 + alpha_q / a, -2.0 * cs, 1.0 - alpha_q / a),
        Kind::Lowshelf => (
            a * ((a + 1.0) - (a - 1.0) * cs + sa), 2.0 * a * ((a - 1.0) - (a + 1.0) * cs), a * ((a + 1.0) - (a - 1.0) * cs - sa),
            (a + 1.0) + (a - 1.0) * cs + sa, -2.0 * ((a - 1.0) + (a + 1.0) * cs), (a + 1.0) + (a - 1.0) * cs - sa),
        Kind::Highshelf => (
            a * ((a + 1.0) + (a - 1.0) * cs + sa), -2.0 * a * ((a - 1.0) + (a + 1.0) * cs), a * ((a + 1.0) + (a - 1.0) * cs - sa),
            (a + 1.0) - (a - 1.0) * cs + sa, 2.0 * ((a - 1.0) - (a + 1.0) * cs), (a + 1.0) - (a - 1.0) * cs - sa),
    }
}
