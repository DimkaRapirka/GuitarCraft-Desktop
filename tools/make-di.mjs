// Generates DI guitar clips (dry, as from an instrument cable) with an extended Karplus-Strong string model.
// node tools/make-di.mjs  → app/assets/di/*.wav
import fs from 'node:fs';
import path from 'node:path';

const SR = 48000;
const OUT = path.resolve(path.dirname(new URL(import.meta.url).pathname), '../app/assets/di');
fs.mkdirSync(OUT, { recursive: true });

let seed = 12345;
const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296 * 2 - 1; };
const midiHz = (m) => 440 * Math.pow(2, (m - 69) / 12);

/** One plucked string note rendered into buffer `out` at sample offset `t0`. */
function pluck(out, t0, { midi, dur, vel = 0.8, mute = 0, pickPos = 0.18, bright = 0.6, vibrato = 0, bendTo = null, bendAt = 0.15, slideFrom = null }) {
  const f0 = midiHz(midi);
  const maxN = Math.ceil(SR / midiHz(Math.min(midi, slideFrom ?? midi, bendTo ?? midi) - 1)) + 4;
  const line = new Float32Array(maxN * 2);
  let N = SR / f0;
  // excitation: noise burst, lowpassed by pick hardness, comb-filtered at pick position
  const exc = new Float32Array(Math.ceil(N));
  let lp = 0; const k = 0.12 + bright * 0.35; // softer pick: less high-frequency energy in the excitation
  for (let i = 0; i < exc.length; i++) { lp += k * (rnd() - lp); exc[i] = lp; }
  const pp = Math.max(1, Math.round(exc.length * pickPos));
  for (let i = exc.length - 1; i >= pp; i--) exc[i] -= exc[i - pp];
  let w = 0; for (let i = 0; i < exc.length; i++) line[i] = exc[i] * vel;
  const len = Math.floor(dur * SR);
  // loop parameters
  const decaySec = mute ? 0.12 + (1 - mute) * 0.3 : 2.2 + (60 - Math.min(60, midi - 40)) * 0.04;
  const rho = Math.pow(0.001, 1 / (decaySec * f0));
  const S = mute ? 0.5 + mute * 0.45 : 0.5 - bright * 0.1; // loop lowpass coefficient (higher = darker)
  let prev = 0, read = 0, writeIdx = Math.ceil(N);
  const L = line.length;
  let lpOut = 0;
  for (let i = 0; i < len && t0 + i < out.length; i++) {
    const t = i / SR;
    let m = midi;
    if (slideFrom != null) m = slideFrom + (midi - slideFrom) * Math.min(1, t / 0.06);
    if (bendTo != null && t > bendAt) m += (bendTo - midi) * Math.min(1, (t - bendAt) / 0.12);
    if (vibrato && t > 0.25) m += vibrato * Math.sin(2 * Math.PI * 5.5 * (t - 0.25)) * Math.min(1, (t - 0.25) / 0.3);
    N = SR / midiHz(m) - 0.5; // compensate 0.5-sample loop filter delay
    let rp = writeIdx - N; while (rp < 0) rp += L;
    const i0 = Math.floor(rp), fr = rp - i0;
    const s = line[i0 % L] * (1 - fr) + line[(i0 + 1) % L] * fr;
    const y = rho * ((1 - S) * s + S * prev);
    prev = s;
    line[writeIdx % L] = y; writeIdx++;
    // release / note-off damping at the end
    const rel = i > len - SR * 0.03 ? (len - i) / (SR * 0.03) : 1;
    lpOut += 0.6 * (s - lpOut);
    out[t0 + i] += lpOut * rel;
    w++;
  }
}

function pickup(buf) {
  // magnetic pickup: comb at pickup position + resonant lowpass ~ 3.5 kHz, slight HPF
  const d = Math.round(SR / 82 * 0.08);
  const y = new Float32Array(buf.length);
  for (let i = 0; i < buf.length; i++) y[i] = buf[i] - 0.35 * (i >= d ? buf[i - d] : 0);
  // biquad lowpass resonant
  const f = 2800, Q = 1.3, w0 = 2 * Math.PI * f / SR, al = Math.sin(w0) / (2 * Q), cs = Math.cos(w0);
  const b0 = (1 - cs) / 2, b1 = 1 - cs, b2 = b0, a0 = 1 + al, a1 = -2 * cs, a2 = 1 - al;
  let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
  for (let i = 0; i < y.length; i++) { const x = y[i]; const o = (b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0; x2 = x1; x1 = x; y2 = y1; y1 = o; y[i] = o; }
  let l1 = 0; for (let i = 0; i < y.length; i++) { l1 += 0.45 * (y[i] - l1); y[i] = l1; } // gentle extra roll-off
  let hp = 0, px = 0; for (let i = 0; i < y.length; i++) { hp = 0.995 * (hp + y[i] - px); px = y[i]; y[i] = hp; }
  // normalise to a typical DI level (peaks around -6 dBFS)
  let pk = 0; for (const v of y) pk = Math.max(pk, Math.abs(v));
  for (let i = 0; i < y.length; i++) y[i] = y[i] / pk * 0.5 + rnd() * 0.0004; // tiny hum/noise floor like a real DI
  return y;
}

function wav(file, data) {
  const b = Buffer.alloc(44 + data.length * 2);
  b.write('RIFF', 0); b.writeUInt32LE(36 + data.length * 2, 4); b.write('WAVEfmt ', 8); b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22);
  b.writeUInt32LE(SR, 24); b.writeUInt32LE(SR * 2, 28); b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34); b.write('data', 36); b.writeUInt32LE(data.length * 2, 40);
  for (let i = 0; i < data.length; i++) b.writeInt16LE(Math.max(-32767, Math.min(32767, Math.round(data[i] * 32767))), 44 + i * 2);
  fs.writeFileSync(path.join(OUT, file), b);
  console.log(file, (data.length / SR).toFixed(2) + 's');
}

const BPM = 120, beat = 60 / BPM, s16 = beat / 4;
const power = (root) => [root, root + 7, root + 12];

// 1) Palm-muted riff (drop-in metal/rock riff in E), 4 bars, loops cleanly
{
  const bars = 4, out = new Float32Array(Math.round(bars * 4 * beat * SR));
  const E = 40, G = 43, A = 45, D = 38 + 12 - 12, C = 48;
  // pattern of 16ths per bar: 'm' muted low E, 'P' power chord accent, '-' rest
  const pat = [
    ['m', 'm', 'm', 'm', 'P:G', '-', 'm', 'm', 'm', 'm', 'P:A', '-', 'm', 'P:G', '-', 'm'],
    ['m', 'm', 'm', 'm', 'P:G', '-', 'm', 'm', 'm', 'm', 'P:A', '-', 'P:C', '-', 'P:A', '-'],
    ['m', 'm', 'm', 'm', 'P:G', '-', 'm', 'm', 'm', 'm', 'P:A', '-', 'm', 'P:G', '-', 'm'],
    ['P:E', '-', '-', '-', 'P:G', '-', '-', 'P:A', '-', '-', 'P:C', '-', 'P:A', '-', 'P:G', '-'],
  ];
  const roots = { E: 40, G: 43, A: 45, C: 48 };
  pat.forEach((bar, bi) => bar.forEach((sym, si) => {
    const t = Math.round(((bi * 16 + si) * s16) * SR);
    if (sym === 'm') pluck(out, t, { midi: E, dur: s16 * 1.05, vel: 0.75 + rnd() * 0.1, mute: 0.85, pickPos: 0.12, bright: 0.7 });
    else if (sym.startsWith('P:')) {
      const r = roots[sym.slice(2)]; let len = 1; while (bar[si + len] === '-') len++;
      power(r).forEach((n, j) => pluck(out, t + Math.round(j * 0.006 * SR), { midi: n, dur: s16 * len, vel: 0.9, pickPos: 0.14, bright: 0.75 }));
    }
  }));
  wav('di_riff.wav', pickup(out));
}

// 2) Open chords, strummed (clean/crunch demo), 4 bars
{
  const out = new Float32Array(Math.round(16 * beat * SR));
  const chords = { E: [40, 47, 52, 56, 59, 64], C: [48, 52, 55, 60, 64], G: [43, 47, 50, 55, 59, 67], D: [50, 57, 62, 66] };
  const prog = ['E', 'C', 'G', 'D'];
  const strum = [[0, 'd', 1.0], [1.5, 'd', 0.7], [2, 'u', 0.55], [2.5, 'd', 0.75], [3.5, 'u', 0.6]];
  prog.forEach((c, bi) => strum.forEach(([b, dir, v]) => {
    const notes = dir === 'd' ? chords[c] : [...chords[c]].reverse().slice(0, 4);
    const t = Math.round((bi * 4 + b) * beat * SR);
    notes.forEach((n, j) => pluck(out, t + Math.round(j * 0.011 * SR), { midi: n, dur: beat * 1.6, vel: v * (0.8 + rnd() * 0.1), pickPos: 0.2, bright: 0.55 }));
  }));
  wav('di_chords.wav', pickup(out));
}

// 3) Lead phrase in E minor pentatonic with bends, slides and vibrato
{
  const out = new Float32Array(Math.round(16 * beat * SR));
  const phrase = [
    [0, 64, 0.5], [0.5, 67, 0.5], [1, 69, 1, { bendTo: 71, bendAt: 0.1 }], [2, 67, 0.5], [2.5, 64, 0.5], [3, 62, 1, { vibrato: 0.25 }],
    [4, 64, 0.25], [4.25, 67, 0.25], [4.5, 69, 0.25], [4.75, 71, 0.25], [5, 74, 1.5, { slideFrom: 72, vibrato: 0.3 }], [6.5, 71, 0.5], [7, 69, 1, { bendTo: 71, bendAt: 0.05 }],
    [8, 76, 0.5], [8.5, 74, 0.5], [9, 71, 0.5], [9.5, 69, 0.5], [10, 67, 0.5], [10.5, 69, 0.5], [11, 71, 1, { vibrato: 0.2 }],
    [12, 64, 0.5], [12.5, 67, 0.5], [13, 69, 0.5], [13.5, 67, 0.5], [14, 64, 2, { vibrato: 0.35 }],
  ];
  for (const [b, m, len, o = {}] of phrase) pluck(out, Math.round(b * beat * SR), { midi: m, dur: len * beat * 1.05, vel: 0.85, pickPos: 0.16, bright: 0.65, ...o });
  wav('di_lead.wav', pickup(out));
}
// 4) Open strings E A D G B E for the tuner
{
  const out = new Float32Array(Math.round(12 * SR));
  [40, 45, 50, 55, 59, 64].forEach((m, i) => pluck(out, Math.round(i * 2 * SR), { midi: m, dur: 1.9, vel: 0.8, pickPos: 0.2, bright: 0.5 }));
  wav('di_open_strings.wav', pickup(out));
}
