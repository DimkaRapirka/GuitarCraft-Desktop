#!/usr/bin/env node
// Tiny client for the GuitarCraft control API: node tools/gc.mjs <method> '<json params>'
import { pathToFileURL } from 'node:url';
import fs from 'node:fs'; import os from 'node:os'; import path from 'node:path';
export function control() {
  const base = {
    win32: process.env.APPDATA,
    darwin: path.join(os.homedir(), 'Library/Application Support'),
    linux: process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local', 'share'),
  }[process.platform];
  const f = process.env.GC_CONTROL_FILE || path.join(base, 'GuitarCraft', 'control.json');
  return JSON.parse(fs.readFileSync(f, 'utf8'));
}
export async function call(method, params = {}, source = 'api') {
  const { port, token } = control();
  const req = () => fetch(`http://127.0.0.1:${port}/rpc`, { method: 'POST', headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json', connection: 'close' }, body: JSON.stringify({ method, params, source }) });
  // a pooled keep-alive socket may already be closed by the server after a long idle period → retry once
  let r; try { r = await req(); } catch (e) { if (e.cause?.code !== 'ECONNRESET') throw e; r = await req(); }
  const j = await r.json(); if (j.error) throw new Error(j.error); return j.result;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [method, p] = process.argv.slice(2);
  call(method, p ? JSON.parse(p) : {}, process.env.GC_SOURCE || 'api').then((r) => console.log(JSON.stringify(r, null, 2))).catch((e) => { console.error('ERR', e.message); process.exit(1); });
}
