// Smoke test: connect to the MCP server like an agent would, list tools, run a few calls.
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport, getDefaultEnvironment } from '@modelcontextprotocol/sdk/client/stdio.js';
const c = new Client({ name: 'smoke', version: '1' });
await c.connect(new StdioClientTransport({ command: process.env.MCP_CMD || './target/release/guitarcraft-mcp', args: [], env: { ...getDefaultEnvironment(), ...(process.env.GC_CONTROL_FILE && { GC_CONTROL_FILE: process.env.GC_CONTROL_FILE }) } }));
const { tools } = await c.listTools();
console.log('tools:', tools.map((t) => t.name).join(', '));
const show = async (name, args = {}) => { const r = await c.callTool({ name, arguments: args }); if (r.isError) process.exitCode = 1; console.log(`\n# ${name} ${JSON.stringify(args)}${r.isError ? ' ERROR' : ''}\n` + r.content[0].text.slice(0, 600)); };
for (const [n, a] of JSON.parse(process.argv[2] || '[]')) await show(n, a);
await c.close();
