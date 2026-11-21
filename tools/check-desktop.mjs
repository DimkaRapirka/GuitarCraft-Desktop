// Exercise the installed WebView through the desktop control port, using a fresh data directory.
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { once } from 'node:events';
import { call } from './gc.mjs';

const exe = process.argv[2];
assert(exe, 'Pass the path to the built or installed GuitarCraft executable');
const temp = await mkdtemp(path.join(os.tmpdir(), 'guitarcraft-desktop-'));
const data = path.join(temp, 'data');
const probe = net.createServer().listen(0, '127.0.0.1');
await once(probe, 'listening');
const { port } = probe.address();
await new Promise(resolve => probe.close(resolve));
process.env.GC_CONTROL_FILE = path.join(data, 'control.json');
const launch = () => spawn(path.resolve(exe), [], {
  env: { ...process.env, GC_DATA_DIR: data, GC_DOCUMENTS_DIR: path.join(temp, 'docs'), GC_CONTROL_PORT: String(port) },
  stdio: ['ignore', 'ignore', 'inherit'],
});
let child = launch();
let exit = once(child, 'exit');
async function restart() {
  child.kill();
  await exit;
  child = launch();
  exit = once(child, 'exit');
  await until(async () => (await dom('#btn-settings'))?.text, 90000);
}
const dom = selector => call('eval.dom', { selector });
const demo = (...steps) => call('demo.run', { steps });
const screenshotDir = process.env.GC_SCREENSHOT_DIR;
async function screenshot(name) {
  if (!screenshotDir) return;
  await mkdir(screenshotDir, { recursive: true });
  await delay(300);
  const ids = execFileSync('xdotool', ['search', '--onlyvisible', '--name', '^GuitarCraft$'], {encoding:'utf8'});
  const window = ids.trim().split(/\s+/).at(-1);
  execFileSync('import', ['-window', window, '-resize', '1440x', '-quality', '89', path.join(screenshotDir, name+'.jpg')]);
}
async function until(test, ms = 10000) {
  const deadline = Date.now() + ms;
  let last;
  while (Date.now() < deadline) {
    assert.equal(child.exitCode, null, 'GuitarCraft exited');
    try {
      const ok = await test();
      if (ok) return ok;
    } catch (err) { last = err; }
    await delay(150);
  }
  throw last || new Error('Timed out waiting for WebView');
}
try {
  await until(() => call('state.get', {timeoutMs: 1000}), 90000);
  await until(() => dom('[data-ctl="channel"]'));
  assert(await dom('html[lang="en"]'));
  assert((await dom('#btn-settings')).text.includes('SETTINGS'));
  await call('ui.openSettings',{tab:'language'});
  await until(() => dom('#set-language option[value="en"]'));
  assert.equal((await dom('#set-language option[value="ru"]')).text, 'Русский');
  await call('ui.closeModals');
  await call('ui.openSettings',{tab:'audio'});
  await until(async () => (await dom('.lat-box b'))?.text.includes('Estimated output latency'));
  assert((await dom('.lat-box span')).text.includes('Buffer'));
  await call('ui.closeModals');
  assert((await dom('#btn-latency')).text.includes('kHz'));
  assert.equal((await call('engine.status')).diPlaying, false);
  assert.equal((await dom('#btn-play')).text, '▶');
  await demo({click:'#btn-play'});
  await until(async () => (await call('engine.status')).diPlaying);
  await demo({click:'#btn-play'});
  await until(async () => !(await call('engine.status')).diPlaying);
  assert(await dom('.brand img.logo[src="wasm/app-icon.png"]'));
  await screenshot('amplifier');
  assert.equal((await call('catalog.search', {query:'clarinot'}))[0].id, 'gc.clarinot-clean');
  const { added: uid } = await call('chain.add', {pedal:'gc.clarinot-clean',index:0});
  const artPedal = (await call('chain.add', {pedal:'gc.pulse',index:1})).added;
  const art = await until(() => dom('[data-pedal-vis="'+ artPedal +'"] img.pr-print[src$="night-moth.png"]'));
  assert(art.w > 15 && art.h > 15);
  await demo({click:'.board-remove'});
  assert.equal(await dom('[data-board-pedal="'+artPedal+'"]'),null);
  assert(await dom('.scene-board [data-board-pedal="'+uid+'"].sel'));
  await demo({click:'.board-remove'});
  assert.equal(await dom('.scene-board [data-board-pedal]'),null);
  assert(await dom('.scene-board [data-board-add="0"]'));
  await demo({click:'.scene-board [data-board-add="0"]'});
  await until(() => dom('#cat-search'));
  await call('ui.closeModals');
  await call('history.undo');
  assert(await dom('.scene-board [data-board-pedal="'+uid+'"]'));
  await call('history.redo');
  assert(await dom('.scene-board [data-board-add="0"]'));
  await call('history.undo');
  const chainIds = async () => (await call('state.get')).chain.map(block => block.uid);
  const initialOrder = await chainIds();
  const lastSlot = '[data-chain-slot="' + initialOrder.length + '"]';
  await demo({drag:'[data-block="cab"]',dropOn:lastSlot,dur:180});
  assert.equal((await chainIds()).at(-1), 'cab');
  await call('history.undo');
  assert.deepEqual(await chainIds(), initialOrder);
  await call('history.redo');
  assert.equal((await chainIds()).at(-1), 'cab');
  await call('chain.move', {target:'eq',before:'amp'});
  assert.equal((await chainIds()).indexOf('eq') + 1, (await chainIds()).indexOf('amp'));
  await call('ui.select', {target:uid});
  assert(await dom('[data-pedal-vis="'+uid+'"] .pr-wear[style*="data:image/png"]'));
  assert(await dom('[data-knob="'+uid+'.depth"] .knob-cap:not([style*="transform"]) > .knob-rotor[style*="rotate"]'));
  const depth = async () => (await call('state.get')).chain.find(b => b.uid === uid).params.depth.value;
  const before = await depth();
  await demo({drag:'[data-knob="'+uid+'.depth"]',dy:-30,dur:180});
  assert((await depth()) > before);
  await call('history.undo');
  assert.equal(await depth(),before);
  await call('history.redo');
  await call('midi.learn', {id:uid+'.depth',type:'param'});
  await call('midi.simulate', {from:'QA controller',data:[176,23,64]});
  await call('midi.simulate', {from:'QA controller',data:[176,23,127]});
  assert.equal(await depth(), 1);
  await call('midi.simulate', {from:'other controller',data:[176,23,0]});
  assert.equal(await depth(), 1);
  await call('ui.select',{target:'delay'});
  await call('settings.set',{bpm:100});
  await demo({click:'[data-sync="1/8."]'});
  assert((await dom('[data-rack=delay] .lcd-text b')).text.includes('450 ms'));
  await call('ui.select',{target:'cab'});
  await screenshot('cabinet');
  await demo({click:'[data-cab="1x10-tiny"]'},{click:'#btn-closeup'});
  assert(await dom('.cab-pic.close'));
  assert(!(await call('skin.list')).some(skin => ['tokyo-ghoul', 'tokyo-ghoul-white'].includes(skin.id)));
  const skinFile = path.resolve('app/assets/skins/skin_ember.png');
  const importedSkin = await call('skin.import',{file:skinFile,name:'Imported combo',edit:false});
  assert((await call('skin.list')).some(skin => skin.id === importedSkin.skin && skin.user));
  await until(() => dom('.skin-frame img[alt="Imported combo"]'));
  await until(() => dom('[data-knob="amp.gain"]'));
  const source = path.resolve('app/assets/gear/bg_studio.png');
  const uploaded = await call('settings.background.import',{scene:'studio',file:source});
  assert((await readFile(uploaded.path)).subarray(0,8).equals(Buffer.from('89504e470d0a1a0a','hex')));
  assert(await dom('.app[style*="--gc-background-studio"] .scene-studio'));
  await call('settings.background.select',{scene:'board',id:uploaded.id});
  await call('ui.openSettings',{tab:'appearance'});
  await until(() => dom('#background-board option[value="' + uploaded.id + '"]'));
  await call('ui.closeModals');
  await call('settings.background.select',{scene:'studio'});
  assert.equal(await dom('.app[style*="--gc-background-studio"]'),null);
  assert(await dom('.app[style*="--gc-background-board"]'));
  await assert.rejects(call('settings.background.select',{scene:'rack',id:'missing.png'}),/отсутствует/);
  await call('preset.save',{name:'WASM QA',bank:'QA'});
  await call('preset.load',{name:'WASM QA'});
  assert.equal((await call('state.get')).preset,'WASM QA');
  await demo({click:'[data-plus="0"]'},{click:'#cat-build'},{click:'[data-add-block="gain"]'},{type:'#builder-name',text:' QA',cps:100},{click:'#builder-save'});
  await until(async () => (await call('catalog.search',{query:'My pedal QA'})).some(d=>d.source==='user'));
  await assert.rejects(call('state.get',{},'mcp'), /выключен/);
  await call('settings.set',{mcpEnabled:true});
  assert.equal((await call('state.get',{},'mcp')).preset,'WASM QA');
  await assert.rejects(call('settings.set',{bpm:200},'mcp'),/недоступны/);
  await call('ui.openSettings',{tab:'agents'});
  await until(() => dom('#set-mcp'));
  await call('ui.closeModals');
  const [factory] = await call('catalog.search',{query:'ZVEX'},'mcp');
  assert.equal(factory.id, 'gc.fuzz-factory');
  assert.equal(factory.source, 'builtin');
  const {added: fuzz} = await call('chain.add',{pedal:factory.id,position:'pre-amp'},'mcp');
  for (const knob of ['volume','gate','comp','drive','stab']) {
    assert(await dom('[data-knob="'+fuzz+'.'+knob+'"]'));
  }
  assert(await dom('[data-board-pedal="'+fuzz+'"] img[src$="factory-storm.png"]'));
  if (screenshotDir) {
    await call('chain.add',{pedal:'gc.crown',before:'amp'});
    await call('chain.add',{pedal:'gc.spring',after:'cab'});
    await call('ui.select',{target:fuzz});
    await screenshot('pedalboard');
    await demo({click:'[data-plus="0"]'},{type:'#cat-search',text:'Fuzz Factory',cps:80},{hideCursor:true});
    await until(() => dom('.cat-detail [data-model="gc.fuzz-factory"]'));
    await screenshot('catalog');
    await call('ui.closeModals');
  }
  await call('param.set',{target:fuzz,param:'stab',value:0.42},'mcp');
  await call('preset.save',{name:'Fuzz Factory QA',bank:'QA'});
  await call('preset.load',{name:'Fuzz Factory QA'});
  assert.equal((await call('state.get')).chain.find(block => block.uid === fuzz).params.stab.value,0.42);
  const ctl = JSON.parse(await readFile(process.env.GC_CONTROL_FILE,'utf8'));
  assert.equal(ctl.pid,child.pid);
  for (const [language, settingsLabel, latencyLabel] of [
    ['ru', 'НАСТРОЙКИ', 'Оценка задержки вывода'],
    ['en', 'SETTINGS', 'Estimated output latency'],
  ]) {
    await call('settings.set', {language});
    await restart();
    assert(await dom('html[lang="' + language + '"]'));
    assert((await dom('#btn-settings')).text.includes(settingsLabel));
    await call('ui.openSettings',{tab:'audio'});
    await until(async () => (await dom('.lat-box b'))?.text.includes(latencyLabel));
    await call('ui.closeModals');
  }
  console.log('Desktop UI, chain reordering, backgrounds, amp skin, catalog, MIDI, history, presets and MCP passed.');
} finally {
  if (child.exitCode === null && child.signalCode === null) child.kill();
  await exit;
  await rm(temp,{recursive:true,force:true,maxRetries:5,retryDelay:200});
}
