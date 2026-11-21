//! Waveshaping with 4× oversampling (replaces `WaveShaperNode` with `oversample = '4x'`).
use crate::util::clamp;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve { Soft, Asym, Hard, Fuzz, Diode, Tube }
impl Curve {
    pub fn parse(s: &str) -> Curve {
        match s { "asym" => Curve::Asym, "hard" => Curve::Hard, "fuzz" => Curve::Fuzz, "diode" => Curve::Diode, "tube" => Curve::Tube, _ => Curve::Soft }
    }
}

/// Same transfer functions as the JS `curve(kind, amount)`; input is clamped to [-1, 1] like a WaveShaper table.
#[inline]
pub fn shape(kind: Curve, amount: f32, x: f32) -> f32 {
    let x = clamp(x, -1.0, 1.0);
    let y = match kind {
        Curve::Hard => clamp(x * (1.0 + amount * 2.0), -0.8, 0.8) / 0.8,
        Curve::Fuzz => { let k = 2.0 + amount * 10.0; if x >= 0.0 { 1.0 - (-k * x).exp() } else { -(1.0 - (k * x * 0.6).exp()) * 0.85 } }
        Curve::Diode => { let k = 2.0 + amount * 4.0; (k * x).tanh() * if x > 0.0 { 1.0 } else { 0.92 } }
        Curve::Asym => { let k = 1.5 + amount * 3.0; let b = 0.18; ((k * (x + b)).tanh() - (k * b).tanh()) / (1.0 - (k * b).tanh() * 0.6) }
        Curve::Tube => { let k = 1.2 + amount * 2.5; if x >= 0.0 { (k * x).tanh() } else { (k * x * 0.85).tanh() / 0.95 } }
        Curve::Soft => { let k = 1.0 + amount * 3.0; (k * x).tanh() / k.tanh() }
    };
    clamp(y, -1.0, 1.0)
}

const TAPS: usize = 48; // FIR length at the 4× rate (12 per phase)
fn design_lowpass() -> [f32; TAPS] {
    // windowed-sinc lowpass at the original Nyquist (0.125 of the 4× rate), Blackman window, unity DC gain
    let mut h = [0f32; TAPS];
    let fc = 0.11f64; let m = (TAPS - 1) as f64; let mut sum = 0.0;
    for (i, v) in h.iter_mut().enumerate() {
        let n = i as f64 - m / 2.0;
        let sinc = if n == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n) };
        let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / m).cos() + 0.08 * (4.0 * std::f64::consts::PI * i as f64 / m).cos();
        *v = (sinc * w) as f32; sum += *v as f64;
    }
    for v in h.iter_mut() { *v = (*v as f64 / sum) as f32; }
    h
}

/// Oversampled waveshaper stage: up ×4 (polyphase) → curve → anti-alias FIR → down ×4.
pub struct Shaper {
    pub kind: Curve, pub amount: f32,
    h: [f32; TAPS],
    up: [f32; TAPS / 4], up_pos: usize,
    down: [f32; TAPS], down_pos: usize,
}
impl Shaper {
    pub fn new(kind: Curve, amount: f32) -> Self { Self { kind, amount, h: design_lowpass(), up: [0.0; TAPS / 4], up_pos: 0, down: [0.0; TAPS], down_pos: 0 } }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let ph = TAPS / 4;
        self.up_pos = (self.up_pos + ph - 1) % ph; self.up[self.up_pos] = x;
        let mut out = 0.0;
        for k in 0..4 {
            // polyphase interpolation: phase k uses taps k, k+4, k+8, …
            let mut s = 0.0;
            for j in 0..ph { s += self.h[k + 4 * j] * self.up[(self.up_pos + j) % ph]; }
            let y = shape(self.kind, self.amount, s * 4.0);
            self.down_pos = (self.down_pos + TAPS - 1) % TAPS; self.down[self.down_pos] = y;
            if k == 3 {
                for j in 0..TAPS { out += self.h[j] * self.down[(self.down_pos + j) % TAPS]; }
            }
        }
        out
    }
    pub fn reset(&mut self) { self.up = [0.0; TAPS / 4]; self.down = [0.0; TAPS]; }
}
