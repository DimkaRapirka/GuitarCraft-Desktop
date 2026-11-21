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

/// Kernel (IR) a node needs: cache key + builder. Cab IRs from files are resolved by the caller via `ir_file`.
pub fn kernel_for(n: &PlanNode, sr: f32, ir_file: &dyn Fn(&str) -> Option<Vec<f32>>) -> Option<(String, Box<dyn FnOnce() -> Vec<f32> + Send>)> {
    if n.kind == "cab" {
        let v = &n.json;
        if txt(v, "mode", "model") == "ir" {
            if let Some(path) = v.get("irPath").and_then(|x| x.as_str()) {
                if let Some(ir) = ir_file(path) { let ir = crate::ir::normalize_user_ir(ir, 8192); return Some((format!("irf:{path}"), Box::new(move || ir))); }
            }
        }
        let (cab, mic) = (txt(v, "model", "4x12-v").to_string(), txt(v, "mic", "dyn57").to_string());
        let (pos, dist) = (num(v, "position", 0.3), num(v, "distance", 0.2));
        let second = txt(v, "mic2", "").to_string();
        let blend = num(v, "blend", 0.5).clamp(0.0, 1.0);
        let pos2 = num(v, "position2", 0.65).clamp(0.0, 1.0);
        let dist2 = num(v, "distance2", 0.25).clamp(0.0, 1.0);
        let inverted = v["invert2"].as_bool().unwrap_or(false);
        let key = format!("cab:{cab}:{mic}:{pos:.3}:{dist:.3}:{second}:{pos2:.3}:{dist2:.3}:{blend:.3}:{inverted}");
        return Some((key, Box::new(move || {
            let mut impulse = crate::ir::cab_ir(sr, &cab, &mic, pos, dist);
            if !second.is_empty() && blend > 0.0 {
                let other = crate::ir::cab_ir(sr, &cab, &second, pos2, dist2);
                let gain = if inverted { -blend } else { blend };
                for (a, b) in impulse.iter_mut().zip(other) {
                    *a = *a * (1.0 - blend) + b * gain;
                }
            }
            impulse
        })));
    }
    kernel_spec(&n.kind, &node_params(n), sr)
}

pub struct Node { pub uid: String, pub kind: String, pub key: Option<String>, pub proc: Box<dyn Processor> }
pub struct Chain { pub sig: String, pub nodes: Vec<Node> }
impl Chain { pub fn process(&mut self, buf: &mut [f32]) { for n in self.nodes.iter_mut() { n.proc.process(buf); } } }

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
    /// Kernel for node i (same chain signature).
    Kernel(usize, String, Box<Convolver>),
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
    scratch: Vec<f32>, scratch2: Vec<f32>, pub meters: Meters,
}
impl Rack {
    pub fn new(sr: f32) -> Self {
        Self { sr, chain: Box::new(Chain { sig: String::new(), nodes: vec![] }), old: None, xf: 0, xf_len: (sr * 0.025) as usize,
            in_gain: Smooth::new(1.0, sr, 0.02), pitch: PitchShift::new(sr), gate: Gate::new(sr), out_gain: Smooth::new(db_to_gain(-9.0), sr, 0.02), fader: Smooth::new(1.0, sr, 0.01),
            dbl_line: DelayLine::new((sr * 0.06) as usize), dbl_mix: Smooth::new(0.0, sr, 0.03), dbl_time: Smooth::new(0.012 * sr, sr, 0.05), dbl_lfo: Lfo::new(sr, 0.31, Wave::Sine),
            lim_l: Limiter::new(sr), lim_r: Limiter::new(sr), scratch: vec![0.0; 8192], scratch2: vec![0.0; 8192], meters: Meters { raw: 0.0, input: 0.0, output: 0.0, gate_open: false } }
    }
    pub fn handle(&mut self, m: Msg, trash: &mut dyn FnMut(Garbage)) {
        match m {
            Msg::Globals(g) => {
                self.in_gain.set(db_to_gain(g.in_db)); self.pitch.semitones = g.transpose;
                self.gate.active = g.gate_on; self.gate.threshold_db = g.gate_thr; self.gate.release_ms = g.gate_rel;
                self.out_gain.set(db_to_gain(g.out_db - 9.0)); // −9 dB headroom before the safety limiter
                self.dbl_mix.set(if g.dbl_on { 1.0 } else { 0.0 }); self.dbl_time.set(g.dbl_spread / 1000.0 * self.sr);
            }
            Msg::Params(list) => { for (i, v) in list.iter() { if let Some(n) = self.chain.nodes.get_mut(*i) { n.proc.set(v); } } trash(Garbage::Json(list)); }
            Msg::Kernel(i, key, c) => { if let Some(n) = self.chain.nodes.get_mut(i) { n.key = Some(key); if let Some(old) = n.proc.set_kernel(c) { trash(Garbage::Conv(old)); } } else { trash(Garbage::Conv(c)); } }
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
        if let Some(old) = self.old.as_mut() {
            let b2 = &mut self.scratch2[..n]; b2.copy_from_slice(buf);
            old.process(b2);
            self.chain.process(buf);
            for i in 0..n { let t = ((self.xf + i) as f32 / self.xf_len as f32).min(1.0); buf[i] = buf[i] * t + b2[i] * (1.0 - t); }
            self.xf += n;
            if self.xf >= self.xf_len { if let Some(o) = self.old.take() { trash(Garbage::Chain(o)); } }
        } else { self.chain.process(buf); }
        let mut pout = 0f32;
        for i in 0..n {
            let x = buf[i] * self.fader.next();
            self.dbl_line.write(x);
            let m = self.dbl_mix.next();
            let d = self.dbl_line.read(self.dbl_time.next() + self.dbl_lfo.next() * 0.0006 * self.sr);
            let g = self.out_gain.next();
            let l = self.lim_l.tick(x * g); let r = self.lim_r.tick((x * (1.0 - m) + d * m) * g);
            out_l[i] = l; out_r[i] = r; pout = pout.max(l.abs()).max(r.abs());
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
    fn second_microphone_blends_without_changing_single_mic_presets() {
        let kernel = |params| {
            let node = PlanNode { uid: "cab".into(), kind: "cab".into(), json: params };
            kernel_for(&node, 48000.0, &|_| None).unwrap().1()
        };
        let first = kernel(json!({ "mic": "dyn57" }));
        let second = kernel(json!({ "mic": "ribbon121", "position": 0.65, "distance": 0.25 }));
        assert_eq!(kernel(json!({ "mic2": "ribbon121", "blend": 0 })), first);
        assert_eq!(kernel(json!({ "mic2": "ribbon121", "blend": 1 })), second);
        let mixed = kernel(json!({ "mic2": "ribbon121", "blend": 0.5, "invert2": true }));
        for ((a, b), sum) in first.iter().zip(second).zip(mixed) {
            assert!((sum - 0.5 * (a - b)).abs() < 1e-6);
        }
    }
}
