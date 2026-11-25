//! Turning a rig (preset JSON) into a processing chain, and the real-time rack around it.
use crate::blocks::{num, txt, Amp, Cab, DelayFx, Eq, Processor, ReverbFx};
use crate::conv::{Convolver, DelayLine};
use crate::dynamics::{Gate, Limiter, PitchShift};
use crate::pedals::{builtin, kernel_spec, BlockPedal};
use crate::util::*;
use serde_json::Value;
use std::collections::HashMap;

pub const CORE: [&str; 5] = ["amp", "cab", "eq", "delay", "reverb"];

/// One enabled block of the planned chain.
#[derive(Clone, Debug)]
pub struct PlanNode { pub uid: String, pub kind: String, pub json: Value }
#[derive(Clone, Debug)]
pub struct Plan { pub sig: String, pub nodes: Vec<PlanNode> }

/// Order of enabled blocks for a rig, including a temporary audition pedal (A/B from the catalog).
pub fn plan(rig: &Value) -> Plan {
    let mut nodes: Vec<PlanNode> = vec![];
    let chain = rig.get("chain").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let on_core = |k: &str| rig.get(k).and_then(|x| x.get("on")).and_then(|x| x.as_bool()).unwrap_or(true);
    let mut enabled_before: Vec<usize> = vec![]; // enabled count before chain index i
    for b in chain.iter() {
        enabled_before.push(nodes.len());
        let kind = txt(b, "kind", "");
        let uid = txt(b, "uid", kind).to_string();
        if kind == "pedal" {
            if b.get("on").and_then(|x| x.as_bool()).unwrap_or(true) {
                nodes.push(PlanNode { uid, kind: txt(b, "pedal", "").to_string(), json: b.get("params").cloned().unwrap_or(Value::Null) });
            }
        } else if CORE.contains(&kind) && on_core(kind) {
            nodes.push(PlanNode { uid, kind: kind.to_string(), json: rig.get(kind).cloned().unwrap_or(Value::Null) });
        }
    }
    enabled_before.push(nodes.len());
    if let Some(a) = rig.get("audition").filter(|a| !a.is_null()) {
        if a.get("active").and_then(|x| x.as_bool()).unwrap_or(false) {
            let idx = a.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
            let mut li = enabled_before[idx.min(enabled_before.len() - 1)];
            if let Some(r) = a.get("replaceUid").and_then(|x| x.as_str()) {
                if let Some(ri) = nodes.iter().position(|n| n.uid == r) { nodes.remove(ri); if ri < li { li -= 1; } }
            }
            let mut json = a.get("params").cloned().unwrap_or(Value::Null);
            if let Some(def) = a.get("def").filter(|d| !d.is_null()) { json = serde_json::json!({ "__def": def, "params": json }); }
            nodes.insert(li.min(nodes.len()), PlanNode { uid: "__audition".into(), kind: txt(a, "pedalId", "").to_string(), json });
        }
    }
    let sig = nodes.iter().map(|n| format!("{}:{}", n.uid, n.kind)).collect::<Vec<_>>().join("|");
    Plan { sig, nodes }
}

/// Construct a processor for a planned node. `user` holds builder pedal definitions by id.
pub fn make(node: &PlanNode, sr: f32, user: &HashMap<String, Value>) -> Option<Box<dyn Processor>> {
    let (params, def) = match node.json.get("__def") { Some(d) => (node.json.get("params").cloned().unwrap_or(Value::Null), Some(d.clone())), None => (node.json.clone(), None) };
    let mut p: Box<dyn Processor> = match node.kind.as_str() {
        "amp" => Box::new(Amp::new(sr)), "cab" => Box::new(Cab::new(sr)), "eq" => Box::new(Eq::new(sr)),
        "delay" => Box::new(DelayFx::new(sr)), "reverb" => Box::new(ReverbFx::new(sr)),
        k => match builtin(k, sr) {
            Some(p) => p,
            None => { let d = def.or_else(|| user.get(k).cloned())?; Box::new(BlockPedal::new(d, sr)) }
        },
    };
    p.set(&params);
    Some(p)
}
/// Params JSON a node's processor expects (strips the audition definition wrapper).
pub fn node_params(n: &PlanNode) -> Value { n.json.get("__def").map(|_| n.json.get("params").cloned().unwrap_or(Value::Null)).unwrap_or_else(|| n.json.clone()) }

/// Kernels (IRs) a node needs: cache key + builder. The cab returns four (close A, close B, room L, room R);
/// reverbs return one. User IR files are resolved by the caller via `ir_file`.
pub type KernelBuild = Box<dyn FnOnce() -> Vec<Vec<f32>> + Send>;
pub fn kernel_for(n: &PlanNode, sr: f32, ir_file: &dyn Fn(&str) -> Option<Vec<f32>>) -> Option<(String, KernelBuild)> {
    if n.kind == "cab" {
        let setup = crate::room::cab_setup(&n.json);
        let user = setup.ir.as_deref().and_then(ir_file).map(|ir| crate::ir::normalize_user_ir(ir, 8192));
        let key = format!("{}:{}", crate::room::kernel_key(&setup), if user.is_some() { "file" } else { "model" });
        return Some((key, Box::new(move || crate::room::cab_kernels(sr, &setup, user))));
    }
    kernel_spec(&n.kind, &node_params(n), sr).map(|(k, b)| (k, Box::new(move || vec![b()]) as KernelBuild))
}

/// A block of the running chain. After the cab the chain is stereo: `twin` processes the right channel.
pub struct Node { pub uid: String, pub kind: String, pub key: Option<String>, pub proc: Box<dyn Processor>, pub twin: Option<Box<dyn Processor>> }
pub struct Chain { pub sig: String, pub nodes: Vec<Node> }
impl Chain {
    /// Mono in `mono`; stereo result in `l`/`r` (both copies of mono when nothing splits).
    pub fn process(&mut self, mono: &mut [f32], l: &mut [f32], r: &mut [f32]) {
        let mut stereo = false;
        for n in self.nodes.iter_mut() {
            if !stereo {
                if n.proc.splits() { n.proc.process_split(mono, l, r); stereo = true; } else { n.proc.process(mono); }
            } else {
                n.proc.process(l);
                match n.twin.as_mut() { Some(t) => t.process(r), None => r.copy_from_slice(l) }
            }
        }
        if !stereo { l.copy_from_slice(mono); r.copy_from_slice(mono); }
    }
}

/// Build a chain for a plan. Blocks after the first cab get a right-channel twin.
/// `kernels(node)` returns the node's kernel key and impulse responses (cached by the caller).
pub fn build_chain(p: &Plan, sr: f32, user: &HashMap<String, Value>, kernels: &mut dyn FnMut(&PlanNode) -> Option<(String, std::sync::Arc<Vec<Vec<f32>>>)>) -> (Chain, Vec<Option<String>>) {
    let split = p.nodes.iter().position(|n| n.kind == "cab");
    let mut nodes = vec![]; let mut keys = vec![];
    for (i, n) in p.nodes.iter().enumerate() {
        let Some(mut proc) = make(n, sr, user) else { continue };
        let mut twin = if split.map_or(false, |s| i > s) { make(n, sr, user) } else { None };
        let mut key = None;
        if let Some((k, irs)) = kernels(n) {
            let _ = proc.set_kernels(irs.iter().map(|ir| Box::new(Convolver::new(ir))).collect());
            if let Some(t) = twin.as_mut() { let _ = t.set_kernels(irs.iter().map(|ir| Box::new(Convolver::new(ir))).collect()); }
            key = Some(k);
        }
        keys.push(key.clone());
        nodes.push(Node { uid: n.uid.clone(), kind: n.kind.clone(), key, proc, twin });
    }
    (Chain { sig: p.sig.clone(), nodes }, keys)
}

/// Global, non-chain settings of a rig.
#[derive(Clone, Debug, Default)]
pub struct Globals { pub in_db: f32, pub transpose: i32, pub gate_on: bool, pub gate_thr: f32, pub gate_rel: f32, pub out_db: f32, pub dbl_on: bool, pub dbl_spread: f32 }
pub fn globals(rig: &Value) -> Globals {
    let i = rig.get("input").cloned().unwrap_or(Value::Null); let g = i.get("gate").cloned().unwrap_or(Value::Null);
    let o = rig.get("output").cloned().unwrap_or(Value::Null); let d = o.get("doubler").cloned().unwrap_or(Value::Null);
    Globals { in_db: num(&i, "gain", 0.0), transpose: num(&i, "transpose", 0.0).round() as i32, gate_on: g.get("on").and_then(|x| x.as_bool()).unwrap_or(true),
        gate_thr: num(&g, "threshold", -62.0), gate_rel: num(&g, "release", 80.0), out_db: num(&o, "gain", 0.0),
        dbl_on: d.get("on").and_then(|x| x.as_bool()).unwrap_or(false), dbl_spread: num(&d, "spread", 12.0) }
}

/// Messages from the control thread to the audio thread.
pub enum Msg {
    Globals(Globals),
    /// Same structure: new params for node i.
    Params(Vec<(usize, Value)>),
    /// Kernels for node i and its right-channel twin (same chain signature).
    Kernel(usize, String, Vec<Box<Convolver>>, Vec<Box<Convolver>>),
    /// New chain structure, crossfaded in.
    Swap(Box<Chain>),
    Mute(bool),
}
/// Objects returned from the audio thread so they are freed elsewhere.
pub enum Garbage { Chain(Box<Chain>), Conv(Box<Convolver>), Json(Vec<(usize, Value)>), Samples(Box<Vec<f32>>) }

pub struct Meters { pub raw: f32, pub input: f32, pub output: f32, pub gate_open: bool }

/// Everything that runs on the audio thread.
pub struct Rack {
    sr: f32, pub chain: Box<Chain>, old: Option<Box<Chain>>, xf: usize, xf_len: usize,
    in_gain: Smooth, pitch: PitchShift, gate: Gate, out_gain: Smooth, fader: Smooth,
    dbl_line: DelayLine, dbl_mix: Smooth, dbl_time: Smooth, dbl_lfo: Lfo, lim_l: Limiter, lim_r: Limiter,
    scratch: Vec<f32>, scratch2: Vec<f32>, sl: Vec<f32>, sr_: Vec<f32>, ol: Vec<f32>, or: Vec<f32>, pub meters: Meters,
}
impl Rack {
    pub fn new(sr: f32) -> Self {
        Self { sr, chain: Box::new(Chain { sig: String::new(), nodes: vec![] }), old: None, xf: 0, xf_len: (sr * 0.025) as usize,
            in_gain: Smooth::new(1.0, sr, 0.02), pitch: PitchShift::new(sr), gate: Gate::new(sr), out_gain: Smooth::new(db_to_gain(-9.0), sr, 0.02), fader: Smooth::new(1.0, sr, 0.01),
            dbl_line: DelayLine::new((sr * 0.06) as usize), dbl_mix: Smooth::new(0.0, sr, 0.03), dbl_time: Smooth::new(0.012 * sr, sr, 0.05), dbl_lfo: Lfo::new(sr, 0.31, Wave::Sine),
            lim_l: Limiter::new(sr), lim_r: Limiter::new(sr), scratch: vec![0.0; 8192], scratch2: vec![0.0; 8192], sl: vec![0.0; 8192], sr_: vec![0.0; 8192], ol: vec![0.0; 8192], or: vec![0.0; 8192], meters: Meters { raw: 0.0, input: 0.0, output: 0.0, gate_open: false } }
    }
    pub fn handle(&mut self, m: Msg, trash: &mut dyn FnMut(Garbage)) {
        match m {
            Msg::Globals(g) => {
                self.in_gain.set(db_to_gain(g.in_db)); self.pitch.semitones = g.transpose;
                self.gate.active = g.gate_on; self.gate.threshold_db = g.gate_thr; self.gate.release_ms = g.gate_rel;
                self.out_gain.set(db_to_gain(g.out_db - 9.0)); // −9 dB headroom before the safety limiter
                self.dbl_mix.set(if g.dbl_on { 1.0 } else { 0.0 }); self.dbl_time.set(g.dbl_spread / 1000.0 * self.sr);
            }
            Msg::Params(list) => {
                for (i, v) in list.iter() { if let Some(n) = self.chain.nodes.get_mut(*i) { n.proc.set(v); if let Some(t) = n.twin.as_mut() { t.set(v); } } }
                trash(Garbage::Json(list));
            }
            Msg::Kernel(i, key, a, b) => match self.chain.nodes.get_mut(i) {
                Some(n) => {
                    n.key = Some(key);
                    for old in n.proc.set_kernels(a) { trash(Garbage::Conv(old)); }
                    match n.twin.as_mut() { Some(t) => for old in t.set_kernels(b) { trash(Garbage::Conv(old)); }, None => for c in b { trash(Garbage::Conv(c)); } }
                }
                None => { for c in a.into_iter().chain(b) { trash(Garbage::Conv(c)); } }
            },
            Msg::Swap(new) => {
                if let Some(o) = self.old.take() { trash(Garbage::Chain(o)); }
                let prev = std::mem::replace(&mut self.chain, new);
                self.old = Some(prev); self.xf = 0;
            }
            Msg::Mute(m) => self.fader.set(if m { 0.0 } else { 1.0 }),
        }
    }
    /// Process one block: mono `input` → stereo out. `trash` receives a finished crossfade chain.
    pub fn process(&mut self, input: &[f32], out_l: &mut [f32], out_r: &mut [f32], trash: &mut dyn FnMut(Garbage)) {
        let n = input.len().min(self.scratch.len());
        let buf = &mut self.scratch[..n];
        let mut raw = 0f32; let mut pin = 0f32;
        for (b, x) in buf.iter_mut().zip(input.iter()) { raw = raw.max(x.abs()); *b = *x * self.in_gain.next(); pin = pin.max(b.abs()); }
        self.pitch.process(buf);
        self.gate.process(buf);
        let (l, r) = (&mut self.sl[..n], &mut self.sr_[..n]);
        if let Some(old) = self.old.as_mut() {
            let b2 = &mut self.scratch2[..n]; b2.copy_from_slice(buf);
            let (ol, or) = (&mut self.ol[..n], &mut self.or[..n]);
            old.process(b2, ol, or);
            self.chain.process(buf, l, r);
            for i in 0..n { let t = ((self.xf + i) as f32 / self.xf_len as f32).min(1.0); l[i] = l[i] * t + ol[i] * (1.0 - t); r[i] = r[i] * t + or[i] * (1.0 - t); }
            self.xf += n;
            if self.xf >= self.xf_len { if let Some(o) = self.old.take() { trash(Garbage::Chain(o)); } }
        } else { self.chain.process(buf, l, r); }
        let mut pout = 0f32;
        for i in 0..n {
            let f = self.fader.next();
            let (xl, xr) = (l[i] * f, r[i] * f);
            self.dbl_line.write(xl);
            let m = self.dbl_mix.next();
            let d = self.dbl_line.read(self.dbl_time.next() + self.dbl_lfo.next() * 0.0006 * self.sr);
            let g = self.out_gain.next();
            let lo = self.lim_l.tick(xl * g); let ro = self.lim_r.tick((xr * (1.0 - m) + d * m) * g);
            out_l[i] = lo; out_r[i] = ro; pout = pout.max(lo.abs()).max(ro.abs());
        }
        self.meters = Meters { raw, input: pin, output: pout, gate_open: self.gate.is_open() };
    }
    pub fn node_index(&self, uid: &str) -> Option<usize> { self.chain.nodes.iter().position(|n| n.uid == uid) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_two_mic_presets_keep_their_mono_sum() {
        // old presets blended two mics into one IR; the cab room plays them as two centred mics
        let sr = 48000.0;
        let cab = json!({ "mic": "dyn57", "distance": 0.2, "mic2": "ribbon121", "position2": 0.65, "distance2": 0.2, "blend": 0.5, "invert2": true });
        let node = PlanNode { uid: "cab".into(), kind: "cab".into(), json: cab.clone() };
        let (_, build) = kernel_for(&node, sr, &|_| None).unwrap();
        let k = build();
        assert_eq!(k.len(), 4);
        assert!(k[2].is_empty() && k[3].is_empty(), "room is off for old presets");
        let a = crate::ir::cab_ir(sr, "4x12-v", "dyn57", 0.3, 0.2);
        let b = crate::ir::cab_ir(sr, "4x12-v", "ribbon121", 0.65, 0.2);
        let mut p = make(&node, sr, &HashMap::new()).unwrap();
        let _ = p.set_kernels(k.iter().map(|ir| Box::new(Convolver::new(ir))).collect());
        let mut x = vec![0f32; 2048]; x[0] = 1.0;
        let (mut l, mut r) = (vec![0f32; 2048], vec![0f32; 2048]);
        // let the level smoothers settle on a silent block first
        let z = vec![0f32; 48000]; let (mut zl, mut zr) = (vec![0f32; 48000], vec![0f32; 48000]);
        p.process_split(&z, &mut zl, &mut zr);
        p.process_split(&x, &mut l, &mut r);
        let level = 1.0; // level 0.5 → 0 dB
        for i in 0..2000 {
            let want = (0.5 * a[i] - 0.5 * b[i]) * level;
            assert!((l[i] - want).abs() < 2e-3 && (r[i] - want).abs() < 2e-3, "sample {i}: {} vs {want}", l[i]);
        }
    }

    #[test]
    fn chain_turns_stereo_after_the_cab() {
        let rig = json!({ "chain": [{ "uid": "amp", "kind": "amp" }, { "uid": "cab", "kind": "cab" }, { "uid": "delay", "kind": "delay" }],
            "cab": { "mics": { "a": { "pan": -1.0 }, "b": { "on": true, "pan": 1.0, "mic": "ribbon121" } } }, "delay": { "on": true } });
        let p = plan(&rig);
        let (mut chain, _) = build_chain(&p, 48000.0, &HashMap::new(), &mut |n| kernel_for(n, 48000.0, &|_| None).map(|(k, b)| (k, std::sync::Arc::new(b()))));
        assert!(chain.nodes[0].twin.is_none() && chain.nodes[2].twin.is_some());
        let mut mono: Vec<f32> = (0..4096).map(|i| ((i as f32) * 0.05).sin() * 0.3).collect();
        let (mut l, mut r) = (vec![0f32; 4096], vec![0f32; 4096]);
        chain.process(&mut mono, &mut l, &mut r);
        assert!(l.iter().zip(r.iter()).any(|(a, b)| (a - b).abs() > 1e-4), "hard-panned mics must differ");
    }
}
