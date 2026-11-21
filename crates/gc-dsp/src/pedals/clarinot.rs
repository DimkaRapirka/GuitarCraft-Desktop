use crate::biquad::{Biquad, Kind};
use crate::blocks::{num, Processor};
use crate::conv::DelayLine;
use crate::dynamics::Envelope;
use crate::util::{lerp, Lfo, Smooth, Wave};
use serde_json::Value;

/// Clean envelope-controlled delay. A stronger pick attack shortens the delay,
/// so the read head speeds up. As the note decays the pitch falls back.
pub(super) struct Clarinot {
    line: DelayLine,
    envelope: Envelope,
    delay: Smooth,
    depth: Smooth,
    blend: Smooth,
    feedback: Smooth,
    wow: Lfo,
    flutter: Lfo,
    repeats: Biquad,
    tap: f32,
    sr: f32,
}

impl Clarinot {
    pub(super) fn new(sr: f32) -> Self {
        let delay = 0.055 * sr;
        Self {
            line: DelayLine::new((sr * 0.64) as usize),
            envelope: Envelope::new(sr),
            delay: Smooth::new(delay, sr, 0.035),
            depth: Smooth::new(0.4, sr, 0.015),
            blend: Smooth::new(0.5, sr, 0.015),
            feedback: Smooth::new(0.15, sr, 0.02),
            wow: Lfo::new(sr, 0.83, Wave::Sine),
            flutter: Lfo::new(sr, 6.7, Wave::Sine),
            repeats: Biquad::new(sr, Kind::Lowpass, 4400.0, 0.7, 0.0),
            tap: delay,
            sr,
        }
    }
}

impl Processor for Clarinot {
    fn set(&mut self, params: &Value) {
        self.delay
            .set(num(params, "delay", 55.0).clamp(6.0, 400.0) * 0.001 * self.sr);
        self.depth.set(num(params, "depth", 0.4).clamp(0.0, 1.0));
        self.blend.set(num(params, "blend", 0.5).clamp(0.0, 1.0));
        self.feedback
            .set(0.84 * num(params, "feedback", 0.18).clamp(0.0, 1.0));
        let tracking = num(params, "tracking", 0.45).clamp(0.0, 1.0);
        self.envelope.sensitivity = 0.32 * tracking * tracking;
        self.envelope.attack_ms = lerp(18.0, 5.0, tracking);
        self.envelope.release_ms = lerp(170.0, 85.0, tracking);
    }

    fn process(&mut self, buf: &mut [f32]) {
        for sample in buf {
            let dry = *sample;
            let env = self.envelope.tick(dry);
            let depth = self.depth.next();
            let drift = 0.002 * self.wow.next() + 0.0004 * self.flutter.next();
            let sweep = 0.17 * depth * depth * (1.0 - env);
            let target = self.delay.next() + self.sr * (sweep + drift * depth * env);
            // Read speed = 1 - change in delay. Permit octave-up bends at
            // extreme settings while keeping the read head moving forward.
            self.tap += (target - self.tap).clamp(-1.45, 0.85);
            let wet = self.line.read(self.tap);
            self.line
                .write(dry + self.repeats.tick(wet) * self.feedback.next());
            *sample = lerp(dry, wet, self.blend.next());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clean_delay_has_a_dry_endpoint_and_timed_repeats() {
        let sr = 48000.0;
        for blend in [0.0, 1.0] {
            let mut pedal = Clarinot::new(sr);
            pedal.set(&json!({"delay":55, "depth":0, "blend":blend, "feedback":0.8}));
            pedal.process(&mut vec![0.0; sr as usize]);
            let mut signal = vec![0.0; 6000];
            signal[0] = 0.5;
            pedal.process(&mut signal);
            if blend == 0.0 {
                assert!((signal[0] - 0.5).abs() < 0.0001);
                assert!(signal[1..].iter().all(|x| x.abs() < 0.0001));
            } else {
                assert!(signal[..2639].iter().all(|x| x.abs() < 0.0001));
                assert!(signal[2640] > 0.49);
                assert!(signal[5280..5300].iter().any(|x| x.abs() > 0.01));
            }
        }
    }

    #[test]
    fn tracking_changes_pick_response_and_allows_octave_bends() {
        fn fastest_read(level: f32, tracking: f32) -> f32 {
            let mut pedal = Clarinot::new(48000.0);
            pedal.set(&json!({"depth":1, "tracking":tracking}));
            pedal.process(&mut vec![0.0; 48000]);
            let mut speed = 0.0_f32;
            for i in 0..3000 {
                let old = pedal.tap;
                let sample = level * (i as f32 * 0.04).sin();
                pedal.process(&mut [sample]);
                speed = speed.max(1.0 + old - pedal.tap);
            }
            speed
        }
        let soft = fastest_read(0.01, 0.4);
        assert!(fastest_read(0.1, 0.4) > soft + 0.1);
        assert!(fastest_read(0.05, 1.0) > fastest_read(0.05, 0.0) + 0.1);
        assert!(fastest_read(0.7, 1.0) > 2.0);
    }
}
