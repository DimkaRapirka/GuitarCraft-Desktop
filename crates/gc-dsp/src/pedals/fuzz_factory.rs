use crate::biquad::{Biquad, Kind};
use crate::blocks::{num, Processor};
use crate::conv::DelayLine;
use crate::shaper::{Curve, Shaper};
use crate::util::{lerp, Lcg, Smooth};
use serde_json::Value;

/// Behavioral gated fuzz. A short inverting feedback loop shares the guitar's
/// clipping stage, so a held note can pull or interrupt a free-running squeal.
pub(super) struct FuzzFactory {
    sr: f32,
    volume: Smooth,
    gate: Smooth,
    comp: Smooth,
    drive: Smooth,
    stab: Smooth,
    input: Biquad,
    clip: Shaper,
    dc_block: Biquad,
    feedback: DelayLine,
    noise: Lcg,
    envelope: f32,
    release: f32,
}

impl FuzzFactory {
    pub(super) fn new(sr: f32) -> Self {
        let knob = |v| Smooth::new(v, sr, 0.009);
        Self {
            sr,
            volume: knob(0.35),
            gate: knob(0.1),
            comp: knob(0.35),
            drive: knob(0.6),
            stab: knob(1.0),
            input: Biquad::new(sr, Kind::Highpass, 55.0, -3.0, 0.0),
            clip: Shaper::new(Curve::Fuzz, 0.7),
            dc_block: Biquad::new(sr, Kind::Highpass, 22.0, -3.0, 0.0),
            feedback: DelayLine::new((sr * 0.01) as usize),
            noise: Lcg(27),
            envelope: 0.0,
            release: (-1.0 / (sr * 0.015)).exp(),
        }
    }
}

impl Processor for FuzzFactory {
    fn set(&mut self, params: &Value) {
        let knob = |id, default| num(params, id, default).clamp(0.0, 1.0);
        self.volume.set(knob("volume", 0.35));
        self.gate.set(knob("gate", 0.1));
        self.comp.set(knob("comp", 0.35));
        self.drive.set(knob("drive", 0.6));
        self.stab.set(knob("stab", 1.0));
    }

    fn process(&mut self, buf: &mut [f32]) {
        for sample in buf {
            let gate = self.gate.next();
            let comp = self.comp.next();
            let stab = self.stab.next();
            let volume = self.volume.next();
            let pinch = ((comp - 0.8) * 5.0).clamp(0.0, 1.0);
            let drive = lerp(self.drive.next(), 0.68, pinch);
            let x = self.input.tick(*sample);
            self.envelope = x.abs().max(self.envelope * self.release);

            // Gate chokes the tail. At the far end of Comp the bias also
            // cuts out parts of each cycle, giving a torn, pulse-like decay.
            let threshold = 0.065 * gate * gate + 0.010 * pinch;
            let open = ((self.envelope - threshold) / (0.0002 + 0.18 * threshold)).clamp(0.0, 1.0);
            let clipped_input = x.signum() * (x.abs() - 0.035 * pinch).max(0.0);
            let octave = gate * gate * (1.0 - comp) * (1.0 - drive);
            let guitar = lerp(clipped_input, clipped_input.abs(), octave) * open;
            let gain = (1.1 + 8.0 * drive * drive) * (1.0 + comp);
            let attack = guitar * gain / (1.0 + 9.0 * comp * self.envelope);

            let frequency =
                95.0 + 1100.0 * stab * stab + 280.0 * drive + 210.0 * comp + 150.0 * gate;
            let feedback = 0.75 * (1.0 - stab) * (1.0 - gate).powi(2) * (1.0 - 0.92 * pinch);
            let tap = self.feedback.read(self.sr / (2.0 * frequency));
            // Tiny deterministic noise seeds an unstable loop even after digital silence.
            let wet = self
                .clip
                .tick(attack + feedback * (self.noise.next() * 1e-6 - tap));
            self.feedback.write(wet);
            *sample = self.dc_block.tick(wet) * volume * volume * lerp(0.65, 1.15, stab);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render(sr: u32, params: Value, guitar_level: f32) -> Vec<f32> {
        let mut pedal = crate::pedals::builtin("gc.fuzz-factory", sr as f32).unwrap();
        pedal.set(&params);
        let mut signal: Vec<_> = (0..sr / 2)
            .map(|i| guitar_level * (i as f32 * std::f32::consts::TAU * 110.0 / sr as f32).sin())
            .collect();
        for block in signal.chunks_mut(128) {
            pedal.process(block);
        }
        assert!(signal.iter().all(|s| s.is_finite() && s.abs() < 2.5));
        signal.split_off(signal.len() / 2)
    }

    fn rms(signal: &[f32]) -> f32 {
        (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt()
    }

    #[test]
    fn stability_and_gate_control_self_oscillation() {
        for sr in [44100, 48000, 96000] {
            let stable = render(sr, json!({}), 0.0);
            let radio = render(sr, json!({"gate":0,"stab":0.25}), 0.0);
            let squelched = render(sr, json!({"gate":1,"stab":0.25}), 0.0);
            assert!(rms(&stable) < 1e-6);
            assert!(rms(&radio) > 0.01);
            assert!(rms(&squelched) < 1e-6);
        }
    }

    #[test]
    fn gate_chokes_soft_notes_and_maximum_compression_overrides_drive() {
        let sr = 48000;
        let open = render(sr, json!({"gate":0}), 0.02);
        let gated = render(sr, json!({"gate":1}), 0.02);
        let quiet = render(sr, json!({"volume":0}), 0.2);
        assert!(rms(&open) > 0.01 && rms(&gated) < 1e-6 && rms(&quiet) < 1e-6);
        for comp in [0.2, 1.0] {
            let low = render(sr, json!({"drive":0,"comp":comp}), 0.18);
            let high = render(sr, json!({"drive":1,"comp":comp}), 0.18);
            let difference = low
                .iter()
                .zip(high)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            if comp == 1.0 {
                assert!(difference < 0.001);
            } else {
                assert!(difference > 0.02);
            }
        }
    }
}
