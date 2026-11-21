//! Small helpers shared by all processors.

pub fn db_to_gain(db: f32) -> f32 { 10f32.powf(db / 20.0) }
pub fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }
pub fn clamp(v: f32, a: f32, b: f32) -> f32 { v.max(a).min(b) }
/// Exponential mapping 0..1 → a..b (a, b > 0), same as the JS `expMap`.
pub fn exp_map(t: f32, a: f32, b: f32) -> f32 { a * (b / a).powf(clamp(t, 0.0, 1.0)) }

/// One-pole parameter smoother, equivalent to Web Audio `setTargetAtTime(v, t, 0.02)`.
#[derive(Clone, Copy, Debug)]
pub struct Smooth { pub value: f32, pub target: f32, coef: f32 }
impl Smooth {
    pub fn new(v: f32, sr: f32, time: f32) -> Self { Self { value: v, target: v, coef: (-1.0 / (sr * time)).exp() } }
    pub fn set(&mut self, t: f32) { self.target = t; }
    pub fn snap(&mut self, t: f32) { self.target = t; self.value = t; }
    #[inline] pub fn next(&mut self) -> f32 { self.value = self.target + (self.value - self.target) * self.coef; self.value }
    pub fn settled(&self) -> bool { (self.value - self.target).abs() < 1e-5 }
}

/// Parameters of a pedal as `id → value`.
pub type Params = std::collections::HashMap<String, f32>;
pub fn p(params: &Params, id: &str, def: f32) -> f32 { params.get(id).copied().unwrap_or(def) }

/// Deterministic PRNG (same LCG as the JS reverb so IRs match).
pub struct Lcg(pub u32);
impl Lcg {
    pub fn next(&mut self) -> f32 { self.0 = self.0.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff; self.0 as f32 / 0x7fff_ffff as f32 * 2.0 - 1.0 }
}

/// Low-frequency oscillator.
#[derive(Clone, Copy, PartialEq)]
pub enum Wave { Sine, Triangle, Square }
pub struct Lfo { phase: f32, pub freq: Smooth, pub wave: Wave, sr: f32 }
impl Lfo {
    pub fn new(sr: f32, f: f32, wave: Wave) -> Self { Self { phase: 0.0, freq: Smooth::new(f, sr, 0.02), wave, sr } }
    #[inline] pub fn next(&mut self) -> f32 {
        let f = self.freq.next();
        self.phase += f / self.sr; if self.phase >= 1.0 { self.phase -= 1.0; }
        match self.wave {
            Wave::Sine => (self.phase * std::f32::consts::TAU).sin(),
            Wave::Triangle => 1.0 - 4.0 * (self.phase - 0.5).abs(),
            Wave::Square => if self.phase < 0.5 { 1.0 } else { -1.0 },
        }
    }
}
