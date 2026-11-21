//! Native audio engine: CoreAudio (cpal) streams, the real-time callback around `gc_dsp::chain::Rack`,
//! analysis/tuner/recorder in a background thread, and the control side that turns rigs into chains.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat};
use crossbeam_channel::{bounded, Receiver, Sender};
use gc_dsp::chain::{globals, kernel_for, make, node_params, plan, Chain, Garbage, Msg, Node, Rack};
use gc_dsp::conv::Convolver;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

fn f2u(v: f32) -> u32 { v.to_bits() }
fn u2f(v: u32) -> f32 { f32::from_bits(v) }

/// Lock-free state shared with the audio callback.
#[derive(Default)]
pub struct Shared {
    pub meter_raw: AtomicU32, pub meter_in: AtomicU32, pub meter_out: AtomicU32, pub gate_open: AtomicBool,
    pub di_playing: AtomicBool, pub di_restart: AtomicBool, pub live: AtomicBool,
    pub metro_on: AtomicBool, pub bpm: AtomicU32, pub metro_level: AtomicU32, pub test_tone: AtomicU32,
    pub recording: AtomicBool, pub out_latency_us: AtomicU32, pub underruns: AtomicU64,
}

/// Messages to the audio callback.
pub enum Cb { Rack(Msg), Di(Box<Vec<f32>>) }

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AudioConfig { pub mode: String, pub input: Option<String>, pub channel: u16, pub output: Option<String>, #[serde(default)] pub output_pair: u16, #[serde(default)] pub mono: bool, pub buffer: u32, pub sample_rate: Option<u32> }
#[derive(Clone, Debug, Serialize, Default)]
pub struct StreamInfo { pub sample_rate: u32, pub buffer: u32, pub output: String, pub input: Option<String>, pub in_channels: u16, pub channel: u16, pub out_channels: u16, pub sample_rates: Vec<u32> }

enum AudioCmd { Configure(AudioConfig, Sender<Result<StreamInfo, String>>) }

pub struct Analysis { pub out: Vec<f32>, pub out_pos: usize, pub inp: Vec<f32>, pub inp_pos: usize, writer: Option<(hound::WavWriter<std::io::BufWriter<std::fs::File>>, PathBuf)> }
impl Analysis {
    fn last(buf: &[f32], pos: usize, n: usize) -> Vec<f32> { let len = buf.len(); (0..n).map(|i| buf[(pos + len - n + i) % len]).collect() }
    pub fn last_out(&self, n: usize) -> Vec<f32> { Self::last(&self.out, self.out_pos, n) }
    pub fn last_in(&self, n: usize) -> Vec<f32> { Self::last(&self.inp, self.inp_pos, n) }
}

struct Control { rig: Value, sig: String, keys: Vec<Option<String>>, user: HashMap<String, Value>, kernels: HashMap<String, Arc<Vec<f32>>>, ir_files: HashMap<String, Vec<f32>>, di_src: Option<(Vec<f32>, u32)> }

pub struct Engine {
    pub shared: Arc<Shared>,
    pub analysis: Arc<Mutex<Analysis>>,
    tx: Mutex<Option<Sender<Cb>>>,
    ctl: Mutex<Control>,
    audio: Sender<AudioCmd>,
    pub info: Mutex<StreamInfo>,
    pub di_name: Mutex<String>,
}

impl Engine {
    pub fn new() -> Arc<Self> {
        let (atx, arx) = bounded::<AudioCmd>(8);
        let shared = Arc::new(Shared::default());
        shared.bpm.store(f2u(120.0), Relaxed); shared.metro_level.store(f2u(0.5), Relaxed);
        let analysis = Arc::new(Mutex::new(Analysis { out: vec![0.0; 1 << 16], out_pos: 0, inp: vec![0.0; 1 << 14], inp_pos: 0, writer: None }));
        let e = Arc::new(Engine { shared, analysis, tx: Mutex::new(None), ctl: Mutex::new(Control { rig: Value::Null, sig: String::new(), keys: vec![], user: HashMap::new(), kernels: HashMap::new(), ir_files: HashMap::new(), di_src: None }),
            audio: atx, info: Mutex::new(StreamInfo::default()), di_name: Mutex::new(String::new()) });
        let e2 = e.clone();
        std::thread::Builder::new().name("gc-audio-ctl".into()).spawn(move || audio_thread(e2, arx)).unwrap();
        e
    }
    pub fn sr(&self) -> f32 { self.info.lock().sample_rate.max(8000) as f32 }
    fn send(&self, m: Cb) { if let Some(tx) = self.tx.lock().as_ref() { let _ = tx.try_send(m); } }

    /// (Re)open audio devices. Rebuilds the chain at the new sample rate.
    pub fn configure(&self, cfg: AudioConfig) -> Result<StreamInfo, String> {
        let (rtx, rrx) = bounded(1);
        self.audio.send(AudioCmd::Configure(cfg, rtx)).map_err(|e| e.to_string())?;
        let info = rrx.recv().map_err(|e| e.to_string())??;
        // fresh rack: resend everything
        { let mut c = self.ctl.lock(); c.sig.clear(); c.keys.clear(); c.kernels.clear(); c.ir_files.clear(); }
        let rig = self.ctl.lock().rig.clone();
        if !rig.is_null() { self.set_rig(rig); }
        let di = self.ctl.lock().di_src.clone();
        if let Some((d, sr)) = di { self.send(Cb::Di(Box::new(resample(&d, sr as f32, self.sr())))); }
        Ok(info)
    }

    /// New rig state from the UI: cheap parameter updates when the structure is unchanged, crossfaded rebuild otherwise.
    pub fn set_rig(&self, rig: Value) {
        let sr = self.sr();
        let mut c = self.ctl.lock();
        let p = plan(&rig);
        self.send(Cb::Rack(Msg::Globals(globals(&rig))));
        let ir_cache = c.ir_files.clone();
        let ir_file = |path: &str| -> Option<Vec<f32>> { ir_cache.get(path).cloned().or_else(|| decode_file(path).ok().map(|(d, s)| resample(&d, s as f32, sr))) };
        if p.sig != c.sig {
            let mut nodes = vec![]; let mut keys = vec![];
            for n in &p.nodes {
                let Some(mut proc) = make(n, sr, &c.user) else { continue };
                let mut key = None;
                if let Some((k, build)) = kernel_for(n, sr, &ir_file) {
                    let ir = c.kernels.entry(k.clone()).or_insert_with(|| Arc::new(build())).clone();
                    proc.set_kernel(Box::new(Convolver::new(&ir))); key = Some(k);
                }
                keys.push(key.clone());
                nodes.push(Node { uid: n.uid.clone(), kind: n.kind.clone(), key, proc });
            }
            c.keys = keys; c.sig = p.sig.clone();
            self.send(Cb::Rack(Msg::Swap(Box::new(Chain { sig: p.sig, nodes }))));
        } else {
            let list: Vec<(usize, Value)> = p.nodes.iter().enumerate().map(|(i, n)| (i, node_params(n))).collect();
            self.send(Cb::Rack(Msg::Params(list)));
            for (i, n) in p.nodes.iter().enumerate() {
                if let Some((k, build)) = kernel_for(n, sr, &ir_file) {
                    if c.keys.get(i).cloned().flatten().as_deref() != Some(k.as_str()) {
                        let ir = c.kernels.entry(k.clone()).or_insert_with(|| Arc::new(build())).clone();
                        if c.kernels.len() > 96 { c.kernels.clear(); }
                        self.send(Cb::Rack(Msg::Kernel(i, k.clone(), Box::new(Convolver::new(&ir)))));
                        if let Some(slot) = c.keys.get_mut(i) { *slot = Some(k); }
                    }
                }
            }
        }
        // remember decoded IR files
        if let Some(path) = rig.get("cab").and_then(|x| x.get("irPath")).and_then(|x| x.as_str()) {
            if !c.ir_files.contains_key(path) { if let Ok((d, s)) = decode_file(path) { c.ir_files.insert(path.into(), resample(&d, s as f32, sr)); } }
        }
        c.rig = rig;
    }
    pub fn set_user_pedals(&self, defs: Vec<Value>) {
        let mut c = self.ctl.lock();
        c.user = defs.into_iter().filter_map(|d| { let id = d.get("id")?.as_str()?.to_string(); Some((if id.starts_with("user.") { id } else { format!("user.{id}") }, d)) }).collect();
        c.sig.clear();
        let rig = c.rig.clone(); drop(c);
        if !rig.is_null() { self.set_rig(rig); }
    }
    pub fn mute(&self, m: bool) { self.send(Cb::Rack(Msg::Mute(m))); }

    // ---------- DI player ----------
    pub fn load_di(&self, data: Vec<f32>, sr: u32, name: &str) {
        *self.di_name.lock() = name.to_string();
        self.ctl.lock().di_src = Some((data.clone(), sr));
        self.send(Cb::Di(Box::new(resample(&data, sr as f32, self.sr()))));
        self.shared.di_restart.store(true, Relaxed);
    }

    // ---------- analysis ----------
    pub fn spectrum(&self, n: usize) -> Vec<f32> { let x = self.analysis.lock().last_out(n); gc_dsp::analysis::spectrum_db(&x) }
    pub fn analyze(&self, seconds: f32) -> Value {
        let sr = self.sr(); let fft = 8192usize; let mut acc = vec![0f32; fft / 2 + 1]; let mut frames = 0;
        let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(seconds.clamp(0.3, 10.0));
        while std::time::Instant::now() < end {
            let x = self.analysis.lock().last_out(fft);
            for (a, p) in acc.iter_mut().zip(gc_dsp::analysis::power_spectrum(&x)) { *a += p; } frames += 1;
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        acc.iter_mut().for_each(|v| *v /= frames.max(1) as f32);
        let (b, total) = gc_dsp::analysis::bands(&acc, sr, fft);
        let names = gc_dsp::analysis::BANDS.map(|x| x.0);
        let r1 = |v: f32| (v * 10.0).round() / 10.0;
        let abs: serde_json::Map<String, Value> = names.iter().zip(b.iter()).map(|(k, v)| (k.to_string(), json!(r1(*v)))).collect();
        let rel: serde_json::Map<String, Value> = names.iter().zip(b.iter()).map(|(k, v)| (k.to_string(), json!(r1(v - total)))).collect();
        let reference: serde_json::Map<String, Value> = names.iter().zip(gc_dsp::analysis::REFERENCE.iter()).map(|(k, v)| (k.to_string(), json!(v))).collect();
        let dev: serde_json::Map<String, Value> = names.iter().zip(b.iter().zip(gc_dsp::analysis::REFERENCE.iter())).map(|(k, (v, r))| (k.to_string(), json!(r1(v - total - r)))).collect();
        let d = |k: &str| dev.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let mut notes = vec![];
        if d("sub") > 4.0 || d("low") > 3.0 { notes.push(format!("лишний низ: sub {:+.1} дБ, low {:+.1} дБ к эталону. Бубнение 120–250 Гц снимается eq.low (полка 100 Гц, но широкая) и eq.lowmid (400 Гц) или low cut 100–160 Гц", d("sub"), d("low"))); }
        if d("highmid") > 4.0 || d("high") > 5.0 { notes.push("резкий верх: уберите eq.highmid / eq.high или highcut".into()); }
        if d("mid") < -4.0 && d("lowmid") < -4.0 { notes.push("провал середины".into()); }
        let off: Vec<String> = names.iter().filter(|k| d(k).abs() > 3.0).map(|k| format!("{k} {:+.1}", d(k))).collect();
        if notes.is_empty() && off.is_empty() { notes.push("все полосы в пределах ±3 дБ от эталона".into()); }
        else if !off.is_empty() { notes.push(format!("вне ±3 дБ: {}", off.join(", "))); }
        json!({ "bands_dbfs": abs, "bands_relative_db": rel, "reference_relative_db": reference, "deviation_from_reference_db": dev, "assessment": notes.join("; "), "total_dbfs": r1(total), "seconds": seconds, "engine": "rust" })
    }
    pub fn tuner(&self, a4: f32) -> Value {
        let x = self.analysis.lock().last_in(4096);
        let f = gc_dsp::analysis::yin(&x, self.sr(), 0.12);
        if f <= 0.0 { return Value::Null; }
        const NOTES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        let n = 12.0 * (f / a4).log2() + 69.0; let ni = n.round() as i32;
        json!({ "freq": (f * 10.0).round() / 10.0, "note": NOTES[ni.rem_euclid(12) as usize], "octave": ni.div_euclid(12) - 1, "cents": ((n - ni as f32) * 100.0).round() as i32 })
    }
    pub fn meters(&self) -> Value {
        let s = &self.shared;
        json!({ "raw": u2f(s.meter_raw.load(Relaxed)), "input": u2f(s.meter_in.load(Relaxed)), "output": u2f(s.meter_out.load(Relaxed)), "gateOpen": s.gate_open.load(Relaxed) })
    }

    // ---------- recording (app output → WAV) ----------
    pub fn record_start(&self, path: PathBuf) -> Result<(), String> {
        let spec = hound::WavSpec { channels: 2, sample_rate: self.sr() as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let w = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        self.analysis.lock().writer = Some((w, path));
        self.shared.recording.store(true, Relaxed);
        Ok(())
    }
    pub fn record_stop(&self) -> Option<(PathBuf, u64)> {
        self.shared.recording.store(false, Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(80));
        let (w, path) = self.analysis.lock().writer.take()?;
        let _ = w.finalize();
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Some((path, bytes))
    }
}

/// Linear resampler (DI files and IRs to the engine rate, live input when rates differ).
pub fn resample(x: &[f32], from: f32, to: f32) -> Vec<f32> {
    if (from - to).abs() < 1.0 || x.is_empty() { return x.to_vec(); }
    let ratio = from / to; let n = (x.len() as f32 / ratio) as usize;
    (0..n).map(|i| { let p = i as f32 * ratio; let i0 = p.floor() as usize; let fr = p - p.floor(); x[i0.min(x.len() - 1)] * (1.0 - fr) + x[(i0 + 1).min(x.len() - 1)] * fr }).collect()
}

/// Decode an audio file (wav/flac/mp3/ogg) to mono f32.
pub fn decode_file(path: &str) -> Result<(Vec<f32>, u32), String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    decode_stream(Box::new(f), std::path::Path::new(path).extension().and_then(|x| x.to_str()).map(|s| s.to_string()))
}
pub fn decode_bytes(bytes: Vec<u8>, ext: Option<String>) -> Result<(Vec<f32>, u32), String> { decode_stream(Box::new(std::io::Cursor::new(bytes)), ext) }
fn decode_stream(src: Box<dyn symphonia::core::io::MediaSource>, ext: Option<String>) -> Result<(Vec<f32>, u32), String> {
    use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};
    let mss = MediaSourceStream::new(src, Default::default());
    let mut hint = Hint::new(); if let Some(e) = ext.as_deref() { hint.with_extension(e); }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(|e| e.to_string())?;
    let mut fmt = probed.format;
    let track = fmt.default_track().ok_or("no audio track")?.clone();
    let sr = track.codec_params.sample_rate.unwrap_or(48000);
    let mut dec = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let mut out = vec![];
    while let Ok(pkt) = fmt.next_packet() {
        if pkt.track_id() != track.id { continue; }
        let Ok(buf) = dec.decode(&pkt) else { continue };
        let spec = *buf.spec(); let ch = spec.channels.count().max(1);
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec); sb.copy_interleaved_ref(buf);
        for fr in sb.samples().chunks(ch) { out.push(fr.iter().sum::<f32>() / ch as f32); }
    }
    Ok((out, sr))
}

pub fn list_devices() -> Value {
    let host = cpal::default_host();
    let names = |it: Option<Vec<cpal::Device>>| it.unwrap_or_default().iter().filter_map(|d| d.name().ok()).collect::<Vec<_>>();
    let inputs = names(host.input_devices().ok().map(|i| i.collect()));
    let outputs = names(host.output_devices().ok().map(|i| i.collect()));
    let di = host.default_input_device().and_then(|d| d.name().ok()); let dout = host.default_output_device().and_then(|d| d.name().ok());
    json!({ "inputs": inputs, "outputs": outputs, "defaultInput": di, "defaultOutput": dout })
}

// ------------------------------------------------------------------ audio thread
fn audio_thread(e: Arc<Engine>, rx: Receiver<AudioCmd>) {
    let mut streams: Vec<cpal::Stream> = vec![];
    // garbage disposal for objects coming back from the callback
    let (gtx, grx) = bounded::<Garbage>(256);
    std::thread::Builder::new().name("gc-garbage".into()).spawn(move || { for g in grx { drop(g); } }).unwrap();
    while let Ok(AudioCmd::Configure(cfg, reply)) = rx.recv() {
        for s in streams.drain(..) { drop(s); }
        e.shared.live.store(false, Relaxed);
        *e.tx.lock() = None;
        let result = open_streams(&e, &cfg, gtx.clone(), None);
        #[cfg(windows)]
        let result = retry_wasapi_format(result, &e, &cfg, &gtx);
        let r = result.map(|(s, info)| { streams = s; info });
        *e.info.lock() = r.as_ref().cloned().unwrap_or_default();
        let _ = reply.send(r);
    }
}

fn find_device(host: &cpal::Host, name: Option<&str>, input: bool) -> Option<cpal::Device> {
    let list: Vec<cpal::Device> = if input { host.input_devices().ok()?.collect() } else { host.output_devices().ok()?.collect() };
    if let Some(n) = name.filter(|n| *n != "default") { if let Some(d) = list.into_iter().find(|d| d.name().ok().as_deref() == Some(n)) { return Some(d); } }
    if input { host.default_input_device() } else { host.default_output_device() }
}

#[cfg(windows)]
fn retry_wasapi_format(
    mut result: Result<(Vec<cpal::Stream>, StreamInfo), String>,
    engine: &Arc<Engine>,
    config: &AudioConfig,
    garbage: &Sender<Garbage>,
) -> Result<(Vec<cpal::Stream>, StreamInfo), String> {
    let rejected_output = |reason: &String| {
        let reason = reason.to_ascii_lowercase();
        reason.starts_with("выход:") && (reason.contains("0x88890008")
            || reason.contains("stream configuration is not supported"))
    };
    let mut native = config.clone();
    native.sample_rate = None;
    if config.sample_rate.is_some() && result.as_ref().is_err_and(rejected_output) {
        result = open_streams(engine, &native, garbage.clone(), None);
    }
    if result.as_ref().is_err_and(rejected_output) {
        // Some USB drivers reject float during Initialize despite advertising it.
        result = open_streams(engine, &native, garbage.clone(), Some(SampleFormat::I16));
    }
    result
}

fn capture_samples<T: Sample>(
    data: &[T],
    channels: usize,
    channel: usize,
    mut send: impl FnMut(f32),
) where f32: FromSample<T> {
    for frame in data.chunks_exact(channels) {
        send(frame[channel].to_sample::<f32>());
    }
}

fn render_samples<T: Sample + FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    pair: usize,
    mono: bool,
    left: &[f32],
    right: &[f32],
) {
    data.fill(T::EQUILIBRIUM);
    for ((frame, &a), &b) in data.chunks_exact_mut(channels).zip(left).zip(right) {
        let first = if mono { (a + b) * 0.5 } else { a };
        frame[pair] = T::from_sample(first.clamp(-1., 1.));
        if let Some(next) = frame.get_mut(pair + 1) {
            *next = T::from_sample(if mono { first } else { b }.clamp(-1., 1.));
        }
    }
}

macro_rules! sample_types {
    ($format:expr, $sample:ident, $body:expr) => {{
        match $format {
            SampleFormat::F32 => { type $sample = f32; $body }
            SampleFormat::F64 => { type $sample = f64; $body }
            SampleFormat::I8 => { type $sample = i8; $body }
            SampleFormat::I16 => { type $sample = i16; $body }
            SampleFormat::I32 => { type $sample = i32; $body }
            SampleFormat::I64 => { type $sample = i64; $body }
            SampleFormat::U8 => { type $sample = u8; $body }
            SampleFormat::U16 => { type $sample = u16; $body }
            SampleFormat::U32 => { type $sample = u32; $body }
            SampleFormat::U64 => { type $sample = u64; $body }
            _ => {}
        }
    }};
}

fn open_streams(
    e: &Arc<Engine>,
    cfg: &AudioConfig,
    gtx: Sender<Garbage>,
    output_format: Option<SampleFormat>,
) -> Result<(Vec<cpal::Stream>, StreamInfo), String> {
    let host = cpal::default_host();
    let out_dev = find_device(&host, cfg.output.as_deref(), false).ok_or("нет устройства вывода")?;
    let def = out_dev.default_output_config().map_err(|e| e.to_string())?;
    let format = output_format.unwrap_or(def.sample_format());
    let mut rates = vec![def.sample_rate().0];
    if let Ok(configs) = out_dev.supported_output_configs() {
        for config in configs.filter(|c| c.channels() == def.channels() && c.sample_format() == format) {
            for rate in [44100, 48000, 88200, 96000] {
                if (config.min_sample_rate().0..=config.max_sample_rate().0).contains(&rate) {
                    rates.push(rate);
                }
            }
        }
    }
    rates.sort_unstable();
    rates.dedup();
    let sr = cfg.sample_rate.filter(|r| rates.contains(r)).unwrap_or(def.sample_rate().0);
    let out_ch = def.channels();
    let buffer = cfg.buffer.clamp(32, 4096);
    let out_cfg = cpal::StreamConfig { channels: out_ch, sample_rate: cpal::SampleRate(sr), buffer_size: cpal::BufferSize::Fixed(buffer) };

    // input (live guitar)
    let live = cfg.mode == "live";
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new((sr as usize).max(4096));
    let mut streams = vec![]; let mut in_name = None; let mut in_ch = 0u16; let mut channel = cfg.channel;
    if live {
        let in_dev = find_device(&host, cfg.input.as_deref(), true).ok_or("нет устройства входа")?;
        let icfg = in_dev.default_input_config().map_err(|e| e.to_string())?;
        in_ch = icfg.channels(); channel = channel.min(in_ch.saturating_sub(1));
        let in_sr = icfg.sample_rate().0;
        in_name = in_dev.name().ok();
        let ratio = in_sr as f32 / sr as f32; let mut phase = 0f32; let mut prev = 0f32;
        let ch = channel as usize; let nch = in_ch as usize;
        let format = icfg.sample_format();
        let s = in_dev.build_input_stream_raw(&cpal::StreamConfig { channels: in_ch, sample_rate: cpal::SampleRate(in_sr), buffer_size: cpal::BufferSize::Fixed(buffer) },
            format, move |data, _| {
                sample_types!(format, T, if let Some(data) = data.as_slice::<T>() {
                    capture_samples(data, nch, ch, |x| {
                        if (ratio - 1.0).abs() < 1e-4 { let _ = prod.push(x); return; }
                        while phase < 1.0 { let _ = prod.push(prev + (x - prev) * phase); phase += ratio; }
                        phase -= 1.0; prev = x;
                    });
                });
            }, |err| eprintln!("[gc] input stream error: {err}"), None).map_err(|e| format!("вход: {e}"))?;
        s.play().map_err(|e| e.to_string())?; streams.push(s);
    }

    // output + processing
    let (tx, rx) = bounded::<Cb>(512);
    let mut rack = Rack::new(sr as f32);
    let shared = e.shared.clone();
    let (mut aprod, mut acons) = rtrb::RingBuffer::<f32>::new(1 << 17);
    let (mut iprod, mut icons) = rtrb::RingBuffer::<f32>::new(1 << 16);
    let (mut rprod, mut rcons) = rtrb::RingBuffer::<f32>::new(1 << 18);
    let mut di: Box<Vec<f32>> = Box::new(vec![]); let mut di_pos = 0usize;
    let (mut inp, mut l, mut r) = (vec![0f32; 4096], vec![0f32; 4096], vec![0f32; 4096]);
    let mut metro_n: u64 = 0; let mut tone_seen = 0u32; let mut tone_left = 0usize; let mut tone_ph = 0f32;
    let srf = sr as f32;
    let mut trash = move |g: Garbage| { let _ = gtx.try_send(g); };
    let nch = out_ch as usize;
    let pair_start = (cfg.output_pair as usize * 2).min(nch.saturating_sub(1) / 2 * 2);
    let mono = cfg.mono || pair_start + 1 == nch;
    let out_stream = out_dev.build_output_stream_raw(&out_cfg, format, move |data, info| {
        while let Ok(m) = rx.try_recv() {
            match m { Cb::Rack(m) => rack.handle(m, &mut trash), Cb::Di(d) => { let old = std::mem::replace(&mut di, d); trash(Garbage::Samples(old)); di_pos = 0; } }
        }
        if let Some(d) = info.timestamp().playback.duration_since(&info.timestamp().callback) { shared.out_latency_us.store(d.as_micros() as u32, Relaxed); }
        if shared.di_restart.swap(false, Relaxed) { di_pos = 0; }
        let frames = data.len() / nch;
        let mut done = 0;
        let (mut mr, mut mi, mut mo) = (0f32, 0f32, 0f32);
        let metro = shared.metro_on.load(Relaxed); let bpm = u2f(shared.bpm.load(Relaxed)).max(20.0); let mlev = u2f(shared.metro_level.load(Relaxed));
        let tt = shared.test_tone.load(Relaxed); if tt != tone_seen { tone_seen = tt; tone_left = (srf * 0.7) as usize; }
        if live { // keep the live input latency bounded
            let avail = cons.slots(); let keep = frames * 2 + 64;
            if avail > keep * 2 { for _ in 0..(avail - keep) { let _ = cons.pop(); } }
        }
        while done < frames {
            let n = (frames - done).min(inp.len());
            let input = &mut inp[..n];
            if live { for s in input.iter_mut() { *s = cons.pop().unwrap_or(0.0); } }
            else if shared.di_playing.load(Relaxed) && !di.is_empty() { for s in input.iter_mut() { *s = di[di_pos]; di_pos = (di_pos + 1) % di.len(); } }
            else { input.fill(0.0); }
            rack.process(input, &mut l[..n], &mut r[..n], &mut trash);
            mr = mr.max(rack.meters.raw); mi = mi.max(rack.meters.input); mo = mo.max(rack.meters.output);
            let rec = shared.recording.load(Relaxed);
            for i in 0..n {
                let (mut a, mut b) = (l[i], r[i]);
                if rec { let _ = rprod.push(a); let _ = rprod.push(b); }
                let _ = aprod.push(a); let _ = iprod.push(input[i]);
                if metro { // click on every beat, accent on the first of four
                    let period = (srf * 60.0 / bpm) as u64; let ph = metro_n % period; let beat = (metro_n / period) % 4;
                    if (ph as f32) < srf * 0.05 { let t = ph as f32 / srf; let f = if beat == 0 { 1760.0 } else { 1320.0 }; let c = (t * f * std::f32::consts::TAU).sin() * (-t * 90.0).exp() * mlev * 0.5; a += c; b += c; }
                    metro_n += 1;
                }
                if tone_left > 0 { tone_ph += 440.0 / srf; let env = (tone_left as f32 / (srf * 0.05)).min(1.0); let t = (tone_ph * std::f32::consts::TAU).sin() * 0.15 * env; a += t; b += t; tone_left -= 1; }
                l[i] = a;
                r[i] = b;
            }
            sample_types!(format, T, if let Some(data) = data.as_slice_mut::<T>() {
                render_samples(&mut data[done * nch..(done + n) * nch], nch, pair_start, mono, &l[..n], &r[..n]);
            });
            done += n;
        }
        shared.meter_raw.store(f2u(mr), Relaxed); shared.meter_in.store(f2u(mi), Relaxed); shared.meter_out.store(f2u(mo), Relaxed); shared.gate_open.store(rack.meters.gate_open, Relaxed);
    }, |err| eprintln!("[gc] output stream error: {err}"), None).map_err(|e| format!("выход: {e}"))?;
    out_stream.play().map_err(|e| e.to_string())?;
    streams.push(out_stream);
    *e.tx.lock() = Some(tx);
    e.shared.live.store(live, Relaxed);

    // analysis / recorder thread for this stream generation
    let an = e.analysis.clone(); let shared2 = e.shared.clone();
    std::thread::Builder::new().name("gc-analysis".into()).spawn(move || loop {
        let mut got = false;
        { let mut a = an.lock();
          while let Ok(x) = acons.pop() { let p = a.out_pos; a.out[p] = x; a.out_pos = (p + 1) % a.out.len(); got = true; }
          while let Ok(x) = icons.pop() { let p = a.inp_pos; a.inp[p] = x; a.inp_pos = (p + 1) % a.inp.len(); }
          if let Some((w, _)) = a.writer.as_mut() { while let Ok(x) = rcons.pop() { let _ = w.write_sample((x.clamp(-1.0, 1.0) * 32767.0) as i16); } } else { while rcons.pop().is_ok() {} }
        }
        if acons.is_abandoned() && !got { break; }
        let _ = &shared2;
        std::thread::sleep(std::time::Duration::from_millis(15));
    }).unwrap();

    let info = StreamInfo { sample_rate: sr, buffer, output: out_dev.name().unwrap_or_default(), input: in_name, in_channels: in_ch, channel, out_channels: out_ch, sample_rates: rates };
    Ok((streams, info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_audio_uses_the_selected_inputs_and_output_pair() {
        let mut captured = Vec::new();
        capture_samples(&[0i16, 16384, 0, -16384], 2, 1, |sample| captured.push(sample));
        assert_eq!(captured, [0.5, -0.5]);

        let mut output = [0u16; 4];
        render_samples(&mut output, 4, 2, false, &[0.5], &[-0.5]);
        assert_eq!(output, [32768, 32768, 49152, 16384]);
        render_samples(&mut output, 4, 2, true, &[0.75], &[0.25]);
        assert_eq!(output, [32768, 32768, 49152, 49152]);
    }
}
