// Run on the signing Mac after collecting the installers for a release.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const {version} = JSON.parse(readFileSync(path.join(root, 'package.json')));
const directory = process.argv[2];
if (!directory || !process.env.TAURI_SIGNING_PRIVATE_KEY_PATH) {
  throw Error('Pass the release directory and set TAURI_SIGNING_PRIVATE_KEY_PATH.');
}
const prefix = 'GuitarCraft_' + version;
const archive = prefix + '_aarch64.app.tar.gz';
const bundle = path.join(root, 'target/release/bundle/macos');
const appVersion = execFileSync('/usr/libexec/PlistBuddy', ['-c', 'Print CFBundleShortVersionString', path.join(bundle, 'GuitarCraft.app/Contents/Info.plist')], {encoding:'utf8'}).trim();
if (appVersion !== version) throw Error('Build the matching macOS application before signing.');
execFileSync('tar', ['-czf', path.resolve(directory, archive), '-C', bundle, 'GuitarCraft.app'], {
  env: {...process.env, COPYFILE_DISABLE:'1'},
});
const platforms = {};
for (const [target, name] of Object.entries({
  'darwin-aarch64': archive,
  'windows-x86_64': prefix + '_x64-setup.exe',
  'linux-x86_64-deb': prefix + '_amd64.deb',
})) {
  const file = path.resolve(directory, name);
  execFileSync(process.execPath, [path.join(root,'node_modules/@tauri-apps/cli/tauri.js'), 'signer', 'sign', '--app-version', version, file]);
  platforms[target] = {
    url: 'https://github.com/DimkaRapirka/GuitarCraft-Desktop/releases/download/v' + version + '/' + name,
    signature: readFileSync(file + '.sig','utf8').trim(),
  };
}
writeFileSync(path.join(directory,'latest.json'), JSON.stringify({version, platforms}, null, 2) + '\n');
