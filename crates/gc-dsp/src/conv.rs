//! Delay lines and convolution.
//! `Convolver`: zero-latency convolution = direct FIR for the first `B` taps + uniformly partitioned
//! FFT convolution (overlap-save) for the rest, so cab IRs add no latency and long reverbs stay cheap.
use realfft::{num_complex::Complex32, ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

pub struct DelayLine { buf: Vec<f32>, w: usize }
impl DelayLine {
    pub fn new(max_samples: usize) -> Self { Self { buf: vec![0.0; max_samples.max(4) + 2], w: 0 } }
    #[inline] pub fn write(&mut self, x: f32) { self.buf[self.w] = x; self.w = (self.w + 1) % self.buf.len(); }
    /// Read `d` samples behind the last write (fractional, linear interpolation). d ≥ 1.
    #[inline] pub fn read(&self, d: f32) -> f32 {
        let n = self.buf.len(); let d = d.clamp(1.0, (n - 2) as f32);
        let r = self.w as f32 - d; let r = if r < 0.0 { r + n as f32 } else { r };
        let i0 = r.floor() as usize % n; let fr = r - r.floor();
        self.buf[i0] * (1.0 - fr) + self.buf[(i0 + 1) % n] * fr
    }
    pub fn reset(&mut self) { self.buf.iter_mut().for_each(|v| *v = 0.0); }
}

const B: usize = 256;

pub struct Convolver {
    head: Vec<f32>, hist: Vec<f32>, hpos: usize,
    // FFT tail
    parts: Vec<Vec<Complex32>>, fdl: Vec<Vec<Complex32>>, fdl_pos: usize,
    fwd: Arc<dyn RealToComplex<f32>>, inv: Arc<dyn ComplexToReal<f32>>,
    inbuf: Vec<f32>, fill: usize, outq: Vec<f32>, scratch_t: Vec<f32>, scratch_f: Vec<Complex32>, acc: Vec<Complex32>,
}

impl Convolver {
    pub fn new(ir: &[f32]) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(2 * B); let inv = planner.plan_fft_inverse(2 * B);
        let head: Vec<f32> = ir.iter().take(B).copied().collect();
        let tail = if ir.len() > B { &ir[B..] } else { &[][..] };
        let nparts = (tail.len() + B - 1) / B;
        let mut parts = Vec::with_capacity(nparts);
        for p in 0..nparts {
            let mut t = vec![0f32; 2 * B];
            for i in 0..B { if let Some(v) = tail.get(p * B + i) { t[i] = *v; } }
            let mut f = fwd.make_output_vec(); fwd.process(&mut t, &mut f).ok();
            parts.push(f);
        }
        let fdl = (0..nparts).map(|_| fwd.make_output_vec()).collect();
        Self { hist: vec![0.0; head.len().max(1)], head, hpos: 0, parts, fdl, fdl_pos: 0, scratch_f: fwd.make_output_vec(), acc: fwd.make_output_vec(),
            fwd, inv, inbuf: vec![0.0; 2 * B], fill: 0, outq: vec![0.0; B], scratch_t: vec![0.0; 2 * B] }
    }
    pub fn len_taps(&self) -> usize { self.head.len() + self.parts.len() * B }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        // direct head
        let n = self.hist.len();
        self.hpos = (self.hpos + n - 1) % n; self.hist[self.hpos] = x;
        let mut y = 0.0;
        let (a, b) = self.hist.split_at(self.hpos);
        let hl = self.head.len();
        let first = b.len().min(hl);
        for i in 0..first { y += self.head[i] * b[i]; }
        for i in first..hl { y += self.head[i] * a[i - first]; }
        if self.parts.is_empty() { return y; }
        // tail: the partitioned result has latency B, which is exactly the tail's offset
        y += self.outq[self.fill];
        self.inbuf[B + self.fill] = x;
        self.fill += 1;
        if self.fill == B { self.block(); self.fill = 0; }
        y
    }
    fn block(&mut self) {
        let np = self.parts.len();
        self.scratch_t.copy_from_slice(&self.inbuf);
        self.fwd.process(&mut self.scratch_t, &mut self.scratch_f).ok();
        self.fdl_pos = (self.fdl_pos + np - 1) % np;
        self.fdl[self.fdl_pos].copy_from_slice(&self.scratch_f);
        self.acc.iter_mut().for_each(|c| *c = Complex32::new(0.0, 0.0));
        for k in 0..np {
            let x = &self.fdl[(self.fdl_pos + k) % np]; let h = &self.parts[k];
            for (a, (xi, hi)) in self.acc.iter_mut().zip(x.iter().zip(h.iter())) { *a += xi * hi; }
        }
        // real FFT requires zero imaginary parts at DC/Nyquist
        self.acc[0].im = 0.0; let last = self.acc.len() - 1; self.acc[last].im = 0.0;
        self.inv.process(&mut self.acc, &mut self.scratch_t).ok();
        let norm = 1.0 / (2 * B) as f32;
        for i in 0..B { self.outq[i] = self.scratch_t[B + i] * norm; }
        self.inbuf.copy_within(B.., 0);
    }
    pub fn process(&mut self, buf: &mut [f32]) { for s in buf.iter_mut() { *s = self.tick(*s); } }
}
