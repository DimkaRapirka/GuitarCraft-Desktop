#!/bin/zsh
# Restart GuitarCraft (Rust core in a Tauri window) at a fixed position for recordings.
cd "$(dirname $0)/.."
export PATH=/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH
pkill -f "target/debug/guitarcraft" 2>/dev/null; pkill -f "target/release/guitarcraft" 2>/dev/null; pkill -f "GuitarCraft/node_modules/electron" 2>/dev/null; sleep 0.8
if [[ -z "$GC_NO_BUILD" ]]; then cargo build -q -p guitarcraft 2>&1 | grep -E "^error" -A 8; fi
GC_RECORDINGS_DIR="$PWD/recordings" GC_X=${GC_X:-40} GC_Y=${GC_Y:-60} nohup ./target/debug/guitarcraft > "${TMPDIR:-/tmp}/guitarcraft-dev.log" 2>&1 &
for i in {1..160}; do sleep 0.25; node tools/gc.mjs state.get >/dev/null 2>&1 && break; done
sleep 0.5; echo restarted
