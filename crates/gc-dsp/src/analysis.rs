//! Tone analysis (band energies vs a reference balance), spectrum for the EQ display, YIN tuner.
use realfft::RealFftPlanner;

pub const BANDS: [(&str, f32, f32); 6] = [("sub", 20.0, 80.0), ("low", 80.0, 250.0), ("lowmid", 250.0, 800.0), ("mid", 800.0, 2000.0), ("highmid", 2000.0, 5000.0), ("high", 5000.0, 12000.0)];
/// Reference balance of a well-balanced distorted rhythm guitar (measured on the factory "Rhythm — Tight").
pub const REFERENCE: [f32; 6] = [-26.0, -15.5, -6.5, -5.0, -4.5, -16.0];

/// Power spectrum (linear power per bin) of the last `n` samples with a Hann window.
pub fn power_spectrum(x: &[f32]) -> Vec<f32> {
    let n = x.len();
    let mut planner = RealFftPlanner::<f32>::new(); let fft = planner.plan_fft_forward(n);
    let mut buf: Vec<f32> = x.iter().enumerate().map(|(i, v)| v * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())).collect();
    let mut out = fft.make_output_vec(); fft.process(&mut buf, &mut out).ok();
    let norm = 4.0 / (n as f32 * n as f32);
    out.iter().map(|c| (c.re * c.re + c.im * c.im) * norm).collect()
}

/// Spectrum in dB for display (Web Audio AnalyserNode-like scale).
pub fn spectrum_db(x: &[f32]) -> Vec<f32> { power_spectrum(x).iter().map(|p| 10.0 * (p + 1e-12).log10()).collect() }

/// Sum of averaged power spectra into bands; returns (absolute dBFS per band, total dBFS).
pub fn bands(avg_power: &[f32], sr: f32, fft_n: usize) -> ([f32; 6], f32) {
    let mut out = [0f32; 6];
    for (bi, (_, lo, hi)) in BANDS.iter().enumerate() {
        let mut s = 0f64;
        for (i, p) in avg_power.iter().enumerate().skip(1) { let f = i as f32 * sr / fft_n as f32; if f >= *lo && f < *hi { s += *p as f64; } }
        out[bi] = (10.0 * (s + 1e-12).log10()) as f32;
    }
    let total = 10.0 * out.iter().map(|v| 10f64.powf(*v as f64 / 10.0)).sum::<f64>().log10();
    (out, total as f32)
}

/// YIN pitch detection (same as the JS tuner). Returns Hz or −1.
pub fn yin(buf: &[f32], sr: f32, threshold: f32) -> f32 {
    let half = buf.len() / 2;
    let rms = (buf.iter().map(|v| v * v).sum::<f32>() / buf.len() as f32).sqrt();
    if rms < 0.004 { return -1.0; }
    let mut d = vec![0f32; half];
    for tau in 1..half { let mut s = 0.0; for i in 0..half { let x = buf[i] - buf[i + tau]; s += x * x; } d[tau] = s; }
    let mut run = 0.0; d[0] = 1.0;
    for tau in 1..half { run += d[tau]; d[tau] = d[tau] * tau as f32 / if run == 0.0 { 1.0 } else { run }; }
    let min_tau = (sr / 1400.0) as usize; let max_tau = ((sr / 25.0) as usize).min(half - 1);
    let mut tau = 0usize; let mut t = min_tau;
    while t < max_tau { if d[t] < threshold { while t + 1 < max_tau && d[t + 1] < d[t] { t += 1; } tau = t; break; } t += 1; }
    if tau == 0 { return -1.0; }
    let (x0, x1, x2) = (d[tau - 1], d[tau], *d.get(tau + 1).unwrap_or(&d[tau]));
    let den = 2.0 * (2.0 * x1 - x2 - x0);
    let better = tau as f32 + if den != 0.0 { (x2 - x0) / den } else { 0.0 };
    sr / better
}
