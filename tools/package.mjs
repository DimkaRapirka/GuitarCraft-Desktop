// Build the app and its stdio MCP sidecar for the current desktop host.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';

const windows = process.platform === 'win32';
const bundles = { win32: 'nsis', darwin: 'app,dmg', linux: 'deb' }[process.platform];
if (!bundles) throw new Error('Unsupported desktop platform: '+process.platform);
const host = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)[1].trim();
const ext = windows ? '.exe' : '';
const run = (cmd, args) => execFileSync(cmd, args, { stdio: 'inherit' });
run('cargo', ['build', '--locked', '--release', '-p', 'gc-mcp']);
mkdirSync('src-tauri/binaries', { recursive: true });
copyFileSync(`target/release/guitarcraft-mcp${ext}`, `src-tauri/binaries/guitarcraft-mcp-${host}${ext}`);
const config = JSON.stringify({ bundle: { externalBin: ['binaries/guitarcraft-mcp'] } });
run(process.execPath, [resolve('node_modules/@tauri-apps/cli/tauri.js'), 'build', '--config', config, '--bundles', bundles, '--', '--locked']);
