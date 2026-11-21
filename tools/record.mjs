#!/usr/bin/env node
// Records a clip of the GuitarCraft window with the app's own audio.
//   node tools/record.mjs --name 01_v0.1_first-ui --seconds 20 [--script demo.json] [--region app|screen]
//     [--version v0.1] [--desc "..."] [--best 0:07] [--best-desc "..."]
// Video: ffmpeg avfoundation screen capture cropped to the window. Audio: the app's output tap (exact, no mic).
import { spawn, execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { call } from './gc.mjs';

const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const REC = path.join(ROOT, 'recordings');
const args = Object.fromEntries(process.argv.slice(2).reduce((a, v, i, arr) => (v.startsWith('--') ? [...a, [v.slice(2), arr[i + 1]?.startsWith('--') || arr[i + 1] === undefined ? true : arr[i + 1]]] : a), []));
const name = args.name || `clip-${Date.now()}`;
const seconds = Number(args.seconds || 15);
const tmp = path.join(REC, '.tmp'); fs.mkdirSync(tmp, { recursive: true });
const vid = path.join(tmp, `${name}.video.mp4`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  if (args.script && !fs.existsSync(args.script)) throw new Error('no script ' + args.script);
  // keep the app above every other window while recording, and refuse to record if something still covers it
  const b = await call('window.front', { onTop: true });
  await sleep(600);
  const top = execFileSync(path.join(ROOT, 'tools/winlist'), ['--top', String(b.x + b.width / 2), String(b.y + b.height / 2)]).toString().trim();
  if (!/guitarcraft/i.test(top)) { await call('window.front', { onTop: false }); throw new Error(`окно GuitarCraft не сверху (сверху: ${top}); запись отменена`); }
  const scale = 2; // Retina
  let crop;
  if (args.region === 'screen') crop = null;
  else if (args.region && args.region.includes(',')) { const [x, y, w, h] = args.region.split(',').map(Number); crop = { x, y, w, h }; }
  else crop = { x: b.x, y: b.y, w: b.width, h: b.height };
  const vf = crop ? `crop=${crop.w * scale}:${crop.h * scale}:${crop.x * scale}:${crop.y * scale},scale=1920:-2` : 'scale=1920:-2';

  // audio first
  const a0 = await call('record.start');
  const aStartWall = Date.now();
  const ff = spawn('ffmpeg', ['-hide_banner', '-y', '-f', 'avfoundation', '-capture_cursor', args.cursor ? '1' : '0', '-framerate', '30', '-pixel_format', 'nv12', '-i', '2:none',
    '-vf', vf, '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '20', '-pix_fmt', 'yuv420p', '-r', '30', '-progress', 'pipe:1', '-stats_period', '0.1', vid], { stdio: ['pipe', 'pipe', 'pipe'] });
  let vStartWall = null; let ffErr = '';
  ff.stderr.on('data', (d) => { ffErr += d; });
  ff.stdout.on('data', (d) => {
    const s = String(d); const m = s.match(/out_time_us=(\d+)/g);
    if (m && vStartWall === null) { const us = Number(m[m.length - 1].split('=')[1]); if (us > 0) vStartWall = Date.now() - us / 1000; }
  });
  while (vStartWall === null) { await sleep(20); if (ff.exitCode !== null) throw new Error('ffmpeg died: ' + ffErr.slice(-800)); }
  const t0 = Date.now();
  try {
    if (args.script) await runScript(JSON.parse(fs.readFileSync(args.script, 'utf8')));
  } catch (e) {
    // never leave ffmpeg or the app recorder running
    ff.kill('SIGINT'); await call('record.stop', { name: `${name}.failed.wav` }).catch(() => {}); await call('window.front', { onTop: false }).catch(() => {});
    throw e;
  }
  const remain = seconds * 1000 - (Date.now() - t0);
  if (remain > 0) await sleep(remain);
  ff.stdin.write('q'); await new Promise((r) => ff.on('close', r));
  const { file: wav } = await call('record.stop', { name: `${name}.wav` });
  await call('window.front', { onTop: false });
  const wavTmp = path.join(tmp, `${name}.wav`); fs.renameSync(wav, wavTmp);
  // align: audio started at aStartWall, video at vStartWall
  const skip = Math.max(0, (vStartWall - aStartWall) / 1000);
  const out = path.join(REC, `${name}.mp4`);
  execFileSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', vid, '-ss', skip.toFixed(3), '-i', wavTmp, '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '256k', '-shortest', out]);
  fs.unlinkSync(vid);
  fs.renameSync(wavTmp, path.join(tmp, `${name}.wav`));
  const check = verify(out);
  console.log(JSON.stringify({ out, skip_s: skip, ...check }, null, 2));
  if (args.version) appendIndex({ name, version: args.version, desc: args.desc || '', best: args.best || '', bestDesc: args['best-desc'] || '', check });
}

async function runScript(steps) {
  for (const s of steps) {
    if (s.rpc) await call(s.rpc, s.params || {}, s.source || 'api');
    else if (s.sleep) await sleep(s.sleep);
    else if (s.shell) execFileSync('/bin/zsh', ['-lc', s.shell], { stdio: 'inherit' });
    else await call('demo.run', { steps: [s], timeoutMs: 60000 });
  }
}

export function verify(file) {
  const probe = JSON.parse(execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration:stream=codec_type,codec_name,width,height', '-of', 'json', file]));
  const r =spawnSyncStderr(['-hide_banner', '-i', file, '-map', '0:a', '-af', 'volumedetect', '-f', 'null', '-']);
  const mean = Number((r.match(/mean_volume: (-?[\d.]+)/) || [])[1]);
  const max = Number((r.match(/max_volume: (-?[\d.]+)/) || [])[1]);
  // how much of the clip has audible guitar: count 0.5 s windows above -45 dBFS RMS
  const rr = spawnSyncStderr(['-hide_banner', '-i', file, '-map', '0:a', '-af', 'astats=metadata=1:reset=24,ametadata=print:key=lavfi.astats.Overall.RMS_level', '-f', 'null', '-']);
  const lv = [...rr.matchAll(/RMS_level=(-?[\d.inf]+)/g)].map((m) => Number(m[1])).filter((x) => Number.isFinite(x));
  const audible = lv.length ? Math.round(lv.filter((x) => x > -45).length / lv.length * 100) : 0;
  const v = probe.streams.find((s) => s.codec_type === 'video'); const a = probe.streams.find((s) => s.codec_type === 'audio');
  return { duration_s: Number(Number(probe.format.duration).toFixed(2)), video: v && `${v.codec_name} ${v.width}x${v.height}`, audio: a && a.codec_name, mean_db: mean, max_db: max, audible_pct: audible, ok: !!(v && a && mean > -50 && audible > 30) };
}
function spawnSyncStderr(a) {
  return String(spawnSync('ffmpeg', a, { encoding: 'utf8', maxBuffer: 64e6 }).stderr);
}

function appendIndex({ name, version, desc, best, bestDesc, check }) {
  const idx = path.join(REC, 'README.md');
  if (!fs.existsSync(idx)) fs.writeFileSync(idx, `# Записи для ролика GuitarCraft\n\nКлипы пишутся скриптом \`tools/record.mjs\`. Видео: окно приложения. Звук: выход GuitarCraft без микрофона. Проверка: ffprobe и громкость (mean/max dB, доля времени со слышимой гитарой).\n\n| # | Файл | Версия | Что в клипе | Лучший момент | Длит. | Звук (mean / max, слышно) |\n|---|---|---|---|---|---|---|\n`);
  const n = name.split('_')[0];
  fs.appendFileSync(idx, `| ${n} | [${name}.mp4](${name}.mp4) | ${version} | ${desc} | **${best}** ${bestDesc} | ${check.duration_s} с | ${check.mean_db} / ${check.max_db} дБ, ${check.audible_pct}% ${check.ok ? '✅' : '⚠️'} |\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) main().catch((e) => { console.error(e); process.exit(1); });
