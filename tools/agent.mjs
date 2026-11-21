#!/usr/bin/env node
// Runs a real coding agent (headless) against the GuitarCraft MCP server and mirrors its messages
// into the app's agent panel. Tool calls are shown by the app itself (they arrive with source "mcp").
//   node tools/agent.mjs "убери лишний низ и сохрани пресет"
//   AGENT=claude node tools/agent.mjs "..."   (Claude Code; needs `claude` logged in)
//   AGENT=codex  node tools/agent.mjs "..."   (default; Codex CLI with the same MCP server)
import { spawn } from 'node:child_process';
import path from 'node:path';
import { call } from './gc.mjs';

const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const prompt = process.argv.slice(2).join(' ');
const sys = 'Ты звукорежиссёр-ассистент в приложении GuitarCraft. Работай только через MCP-инструменты guitarcraft. Отвечай по-русски, коротко (1–2 предложения на шаг). Перед изменениями посмотри цепочку (chain_get) и послушай звук (tone_analyze); после изменений проверь результат повторным tone_analyze.';
const agent = process.env.AGENT || 'codex';
const say = (t) => { console.log('🤖', t); return call('agent.say', { text: '🤖 ' + t }).catch(() => {}); };

let cmd, args;
if (agent === 'claude') {
  cmd = 'claude';
  args = ['-p', prompt, '--mcp-config', path.join(ROOT, '.mcp.json'), '--strict-mcp-config', '--allowedTools', 'mcp__guitarcraft__*', '--append-system-prompt', sys, '--output-format', 'stream-json', '--verbose'];
} else {
  cmd = 'codex';
  args = ['exec', '--skip-git-repo-check', '-s', 'read-only', '--json', '-m', process.env.CODEX_MODEL || 'gpt-6.1-sol',
    '-c', `mcp_servers.guitarcraft.command="${path.join(ROOT, 'target/release/guitarcraft-mcp')}"`, '-c', 'mcp_servers.guitarcraft.args=[]',
    '-c', 'mcp_servers.guitarcraft.default_tools_approval_mode="approve"', '-c', `model_reasoning_effort="${process.env.EFFORT || 'low'}"`,
    `${sys}\n\nЗадача пользователя: ${prompt}`];
}
const p = spawn(cmd, args, { stdio: ['ignore', 'pipe', 'ignore'], cwd: ROOT });
let buf = ''; const queue = [];
p.stdout.on('data', (d) => {
  buf += d; let i;
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i); buf = buf.slice(i + 1);
    let ev; try { ev = JSON.parse(line); } catch { continue; }
    if (agent === 'claude' && ev.type === 'assistant') for (const c of ev.message.content || []) { if (c.type === 'text' && c.text.trim()) queue.push(say(c.text.trim())); }
    if (agent !== 'claude' && ev.type === 'item.completed' && ev.item?.type === 'agent_message' && ev.item.text?.trim()) queue.push(say(ev.item.text.trim()));
    if (agent !== 'claude' && ev.type === 'item.completed' && ev.item?.type === 'mcp_tool_call') console.log('  →', ev.item.tool, ev.item.status);
    if (ev.type === 'result' || ev.type === 'turn.completed') console.log('[done]');
  }
});
p.on('close', async (c) => { await Promise.all(queue); process.exit(c ?? 0); });
