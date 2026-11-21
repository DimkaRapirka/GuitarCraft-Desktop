//! Impulse responses: algorithmic reverb and cabinet + microphone models (rendered through biquads).
use crate::biquad::{Biquad, Kind};
use crate::util::{clamp, lerp, Lcg};

#[derive(Clone, Copy)]
pub enum ReverbKind { Hall, Spring, Room }

/// Stereo reverb IR, port of the JS `reverbIR` (returned as mono average for the mono chain + right channel).
pub fn reverb_ir(sr: f32, decay: f32, pre_delay: f32, tone: f32, kind: ReverbKind) -> (Vec<f32>, Vec<f32>) {
    let len = (sr * (decay * 1.2 + pre_delay + 0.05).min(8.0)) as usize;
    let pd = (pre_delay * sr) as usize;
    let damp = 0.15 + tone * 0.8;
    let mut ch = [vec![0f32; len], vec![0f32; len]];
    for (c, d) in ch.iter_mut().enumerate() {
        let mut lp = 0f32; let mut rnd = Lcg(if c == 1 { 1234567 } else { 7654321 });
        for i in pd..len {
            let t = (i - pd) as f32 / sr;
            let env = (-6.9 * t / decay).exp();
            let k = damp * (-t * (1.5 - tone)).exp();
            lp += clamp(k, 0.02, 1.0) * (rnd.next() - lp);
            d[i] = lp * env;
        }
        if let ReverbKind::Spring = kind {
            let step = (sr * 0.029) as usize; let mut i = pd;
            while i < len { d[i] += 0.5 * (-((i - pd) as f32) / sr * 3.0).exp(); i += step.max(1); }
        }
        let taps: &[f32] = if let ReverbKind::Room = kind { &[7.0, 11.0, 17.0, 23.0, 31.0] } else { &[13.0, 19.0, 29.0, 37.0, 47.0, 59.0] };
        for (j, ms) in taps.iter().enumerate() {
            let i = pd + ((ms + c as f32 * 1.7) * sr / 1000.0) as usize;
            if i < len { d[i] += (0.6 - j as f32 * 0.07) * if c == 1 { -1.0 } else { 1.0 }; }
        }
    }
    let e: f64 = ch.iter().flat_map(|d| d.iter()).map(|v| (*v as f64) * (*v as f64)).sum();
    let norm = (1.0 / (e / 2.0 + 1e-9).sqrt() * 0.9) as f32;
    for d in ch.iter_mut() { d.iter_mut().for_each(|v| *v *= norm); }
    let [l, r] = ch; (l, r)
}

pub struct CabModel { pub id: &'static str, pub name: &'static str, pub res: f32, pub res_gain: f32, pub hp: f32, pub peaks: &'static [(f32, f32, f32)], pub lp: f32, pub lp_q: f32, pub dip: (f32, f32, f32) }
pub struct MicModel { pub id: &'static str, pub name: &'static str, pub hp: f32, pub peak: (f32, f32, f32), pub shelf: (f32, f32), pub prox: f32 }

pub const CABS: &[CabModel] = &[
    CabModel { id: "1x12-open", name: "1×12 Open Back", res: 105.0, res_gain: 2.0, hp: 80.0, peaks: &[(1800.0, 2.0, 1.2), (3400.0, 3.0, 2.0)], lp: 6200.0, lp_q: 0.9, dip: (700.0, -2.0, 1.0) },
    CabModel { id: "2x12-chime", name: "2×12 Chime", res: 100.0, res_gain: 3.0, hp: 75.0, peaks: &[(2600.0, 4.0, 1.6), (4800.0, 2.0, 2.5)], lp: 7000.0, lp_q: 1.1, dip: (500.0, -3.0, 1.0) },
    CabModel { id: "4x12-v", name: "4×12 Vintage 30", res: 92.0, res_gain: 5.0, hp: 70.0, peaks: &[(1400.0, 2.0, 1.0), (2900.0, 5.0, 1.8)], lp: 5400.0, lp_q: 1.4, dip: (380.0, -2.5, 1.2) },
    CabModel { id: "4x12-g", name: "4×12 Greenback", res: 98.0, res_gain: 4.0, hp: 70.0, peaks: &[(1200.0, 3.0, 1.0), (2400.0, 2.0, 1.5)], lp: 4800.0, lp_q: 1.0, dip: (450.0, -1.5, 1.0) },
    CabModel { id: "1x10-tiny", name: "1×10 Tweed", res: 140.0, res_gain: 3.0, hp: 110.0, peaks: &[(1000.0, 4.0, 0.9), (2200.0, 2.0, 1.4)], lp: 4500.0, lp_q: 0.8, dip: (300.0, -3.0, 1.0) },
];
pub const MICS: &[MicModel] = &[
    MicModel { id: "dyn57", name: "Dynamic 57", hp: 90.0, peak: (5200.0, 4.0, 1.4), shelf: (9000.0, -6.0), prox: 3.0 },
    MicModel { id: "ribbon121", name: "Ribbon 121", hp: 40.0, peak: (1200.0, 1.0, 0.7), shelf: (5000.0, -7.0), prox: 6.0 },
    MicModel { id: "cond414", name: "Condenser 414", hp: 50.0, peak: (8000.0, 3.0, 0.8), shelf: (11000.0, 2.0), prox: 2.0 },
    MicModel { id: "dyn421", name: "Dynamic 421", hp: 70.0, peak: (4000.0, 3.0, 1.0), shelf: (8000.0, -4.0), prox: 4.0 },
];

/// Cabinet + mic IR, port of the JS `renderCabIR` (impulse + floor reflection through the same biquad chain).
pub fn cab_ir(sr: f32, cab: &str, mic: &str, position: f32, distance: f32) -> Vec<f32> {
    let c = CABS.iter().find(|x| x.id == cab).unwrap_or(&CABS[2]);
    let m = MICS.iter().find(|x| x.id == mic).unwrap_or(&MICS[0]);
    let len = 2048usize;
    let mut x = vec![0f32; len]; x[0] = 1.0;
    let refl = ((0.6 + distance * 3.2) * sr / 1000.0) as usize;
    if refl < len { x[refl] = 0.18 + distance * 0.25; }
    let mut chain: Vec<Biquad> = vec![
        Biquad::new(sr, Kind::Highpass, c.hp.max(m.hp * 0.8), 0.8, 0.0),
        Biquad::new(sr, Kind::Peaking, c.res, 2.2, c.res_gain),
        Biquad::new(sr, Kind::Lowshelf, 180.0, 0.7, m.prox * (1.0 - distance) - 1.5),
        Biquad::new(sr, Kind::Peaking, c.dip.0, c.dip.2, c.dip.1),
    ];
    for (f, g, q) in c.peaks { chain.push(Biquad::new(sr, Kind::Peaking, *f, *q, g * (1.0 - position * 0.6))); }
    chain.push(Biquad::new(sr, Kind::Peaking, m.peak.0, m.peak.2, m.peak.1));
    chain.push(Biquad::new(sr, Kind::Highshelf, m.shelf.0, 0.7, m.shelf.1));
    chain.push(Biquad::new(sr, Kind::Lowpass, c.lp * lerp(1.25, 0.55, position), c.lp_q, 0.0));
    chain.push(Biquad::new(sr, Kind::Lowpass, c.lp * lerp(1.4, 0.7, position) + 800.0, 0.6, 0.0));
    chain.push(Biquad::new(sr, Kind::Highshelf, 2600.0, 0.6, -4.0)); // real speakers roll off early
    chain.push(Biquad::new(sr, Kind::Peaking, 500.0, 0.8, 2.5)); // cone body
    for b in chain.iter_mut() { b.process(&mut x); }
    let mut e = 0f64;
    for i in 0..len {
        if i as f32 > len as f32 * 0.7 { x[i] *= 0.5 + 0.5 * (std::f32::consts::PI * (i as f32 - len as f32 * 0.7) / (len as f32 * 0.3)).cos(); }
        e += (x[i] as f64).powi(2);
    }
    let norm = (0.5 / (e + 1e-12).sqrt()) as f32;
    x.iter_mut().for_each(|v| *v *= norm);
    x
}

/// Normalise a user IR to the same loudness as model IRs and cap its length.
pub fn normalize_user_ir(mut ir: Vec<f32>, max_len: usize) -> Vec<f32> {
    if ir.len() > max_len { ir.truncate(max_len); let n = ir.len(); let fade = n / 10; for i in 0..fade { ir[n - 1 - i] *= i as f32 / fade as f32; } }
    let e: f64 = ir.iter().map(|v| (*v as f64).powi(2)).sum();
    let k = (0.5 / (e + 1e-12).sqrt()) as f32;
    ir.iter_mut().for_each(|v| *v *= k);
    ir
}
