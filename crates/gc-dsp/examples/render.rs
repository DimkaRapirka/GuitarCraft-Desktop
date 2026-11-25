//! Offline render: cargo run --release -p gc-dsp --example render -- <di.wav> <rig.json> <out.wav>
use gc_dsp::analysis::{bands, power_spectrum, REFERENCE};
use gc_dsp::chain::{build_chain, globals, kernel_for, plan, Msg, Rack};
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut rd = hound::WavReader::open(&a[1]).unwrap();
    let sr = rd.spec().sample_rate as f32;
    let input: Vec<f32> = rd.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
    let rig: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let p = plan(&rig);
    let user = HashMap::new();
    let (chain, _) = build_chain(&p, sr, &user, &mut |n| kernel_for(n, sr, &|_| None).map(|(k, b)| (k, std::sync::Arc::new(b()))));
    let mut rack = Rack::new(sr);
    let mut trash = |_g| {};
    rack.handle(Msg::Globals(globals(&rig)), &mut trash);
    rack.handle(Msg::Swap(Box::new(chain)), &mut trash);
    let t0 = std::time::Instant::now();
    let (mut l, mut r) = (vec![0f32; input.len()], vec![0f32; input.len()]);
    for (i, chunk) in input.chunks(128).enumerate() {
        let o = i * 128; let n = chunk.len();
        let (lo, ro) = (&mut l[o..o + n], &mut r[o..o + n]);
        rack.process(chunk, lo, ro, &mut trash);
    }
    let el = t0.elapsed().as_secs_f32();
    let fft = 8192; let mut acc = vec![0f32; fft / 2 + 1]; let mut cnt = 0;
    let mut i = (sr * 0.5) as usize; while i + fft < l.len() { for (a, p) in acc.iter_mut().zip(power_spectrum(&l[i..i + fft])) { *a += p; } cnt += 1; i += fft / 2; }
    acc.iter_mut().for_each(|v| *v /= cnt as f32);
    let (b, total) = bands(&acc, sr, fft);
    let rel: Vec<String> = b.iter().zip(REFERENCE.iter()).map(|(x, r)| format!("{:.1}(ref {:.1})", x - total, r)).collect();
    let peak = l.iter().fold(0f32, |m, v| m.max(v.abs()));
    let rms = (l.iter().map(|v| v * v).sum::<f32>() / l.len() as f32).sqrt();
    println!("chain: {}\nrealtime x{:.0} | peak {:.2} | rms {:.1} dBFS\nbands rel: {}", p.sig, input.len() as f32 / sr / el, peak, 20.0 * rms.log10(), rel.join(" "));
    let spec = hound::WavSpec { channels: 2, sample_rate: sr as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&a[3], spec).unwrap();
    for (x, y) in l.iter().zip(r.iter()) { w.write_sample((x * 32767.0) as i16).unwrap(); w.write_sample((y * 32767.0) as i16).unwrap(); }
}
