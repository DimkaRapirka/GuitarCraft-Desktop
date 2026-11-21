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

/// Roughly 110 ms gives the 25 Hz lower limit more than two periods to compare.
pub const TUNER_WINDOW_MS: usize = 110;

/// YIN pitch detection on a single dry note. Returns Hz or −1.
pub fn yin(buf: &[f32], sr: f32, threshold: f32) -> f32 {
    if sr <= 0.0 || !sr.is_finite() || buf.len() < 8 {
        return -1.0;
    }

    // Average adjacent samples before decimating to keep the search off the audio thread cheap.
    let stride = (sr / 12000.0).max(1.0) as usize;
    let samples: Vec<_> = buf
        .chunks_exact(stride)
        .map(|group| group.iter().sum::<f32>() / stride as f32)
        .collect();
    let sr = sr / stride as f32;
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    let rms = (samples.iter().map(|x| (x - mean).powi(2)).sum::<f32>()
        / samples.len() as f32)
        .sqrt();
    if !rms.is_finite() || rms < 0.0015 {
        return -1.0;
    }

    let half = samples.len() / 2;
    let min_lag = ((sr / 1400.0).floor() as usize).max(2);
    let max_lag = ((sr / 25.0).ceil() as usize).min(half.saturating_sub(1));
    if min_lag > max_lag {
        return -1.0;
    }
    let mut difference = vec![1.0; max_lag + 2];
    let mut running_sum = 0.0;
    for lag in 1..difference.len() {
        let sum = samples[..half]
            .iter()
            .zip(&samples[lag..lag + half])
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>();
        running_sum += sum;
        difference[lag] = if running_sum > 0.0 {
            sum * lag as f32 / running_sum
        } else {
            1.0
        };
    }

    let is_minimum = |lag: usize| {
        difference[lag] < difference[lag - 1] && difference[lag] <= difference[lag + 1]
    };
    let Some(first) = (min_lag..=max_lag)
        .find(|&lag| difference[lag] < threshold && is_minimum(lag))
    else {
        return -1.0;
    };
    let mut lag = first;
    // A loud overtone can give a good match at half or a third of the true period.
    // Prefer a multiple only if it gives a clearly better match, even on quiet notes.
    for multiple in [2, 3] {
        let expected = first * multiple;
        let tolerance = (expected / 50).max(2);
        let start = expected - tolerance;
        let end = (expected + tolerance).min(max_lag);
        if let Some(candidate) = (start..=end).filter(|&i| is_minimum(i))
            .min_by(|&a, &b| difference[a].total_cmp(&difference[b]))
        {
            if difference[candidate] + 0.008 < difference[lag] * 0.5 {
                lag = candidate;
            }
        }
    }
    // Refine near the selected lag at the native rate for accurate cents on higher notes.
    let lag = lag * stride;
    let start = lag - stride - 1;
    let end = (lag + stride + 1).min(buf.len() - 1);
    let window = buf.len() - end;
    let local: Vec<_> = (start..=end)
        .map(|lag| {
            buf[..window].iter().zip(&buf[lag..])
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
        })
        .collect();
    let index = (1..local.len() - 1)
        .min_by(|&a, &b| local[a].total_cmp(&local[b]))
        .unwrap();
    let (left, center, right) = (local[index - 1], local[index], local[index + 1]);
    let curvature = left - 2.0 * center + right;
    let offset = if curvature > 0.0 {
        (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    sr * stride as f32 / ((start + index) as f32 + offset)
}

#[cfg(test)]
mod tuner_tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn identifies_guitar_fundamentals_at_different_device_rates() {
        for sr in [44100.0, 48000.0, 96000.0, 192000.0] {
            for midi in [24, 38, 40, 45, 50, 55, 59, 64, 88] {
                let freq = 440.0 * 2_f32.powf((midi as f32 - 69.0 + 0.17) / 12.0);
                let signal: Vec<_> = (0..(sr as usize * TUNER_WINDOW_MS / 1000))
                    .map(|i| {
                        let angle = TAU * freq * i as f32 / sr;
                        let envelope = (-2.0 * i as f32 / sr).exp();
                        0.08 + envelope * (angle.sin() * 0.014 + (2.0 * angle).sin() * 0.08)
                    })
                    .collect();
                let detected = yin(&signal, sr, 0.12);
                let error = 1200.0 * (detected / freq).log2();
                assert!(error.abs() < 3.0, "{sr} Hz, MIDI {midi}: {detected} vs {freq}, {error} cents");
            }
        }
    }

    #[test]
    fn rejects_silence_dc_and_noise() {
        let noise: Vec<_> = (0..5280_u32)
            .map(|i| ((i.wrapping_mul(2654435761) ^ i.wrapping_mul(134775813)) >> 16) as f32 / 65536.0 - 0.5)
            .collect();
        for signal in [&[0.0; 5280][..], &[0.1; 5280][..], &noise[..]] {
            assert_eq!(yin(signal, 48000.0, 0.12), -1.0);
        }
    }
}
