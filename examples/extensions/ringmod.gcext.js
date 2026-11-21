// Example GuitarCraft extension: a ring modulator pedal.
// Copy to ~/Documents/GuitarCraft/Extensions/ and press "Перезагрузить расширения".
export default function register(api) {
  const { gain, setP, mixer, expMap } = api.dsp;
  api.registerPedal({
    id: 'ringmod', name: 'Ring Planet', type: 'Ring Modulator', category: 'modulation', color: '#5b3fa0',
    character: ['ринг-модулятор', 'ring mod', 'металлический', 'колокольный', 'синт', 'экспериментальный'],
    desc: 'Пример расширения: кольцевая модуляция, металлические колокольные обертоны.',
    params: [{ id: 'freq', name: 'Freq', min: 0, max: 1, def: 0.4 }, { id: 'mix', name: 'Mix', min: 0, max: 1, def: 0.5 }],
    create(ctx) {
      const m = mixer(ctx); const vca = gain(ctx, 0); const osc = ctx.createOscillator(); osc.start(); osc.connect(vca.gain);
      m.wetIn.connect(vca).connect(m.wetOut);
      return { input: m.input, output: m.output, apply(p) { setP(osc.frequency, expMap(p.freq ?? 0.4, 30, 2000), ctx); m.setMix(p.mix ?? 0.5); } };
    },
  });
}
