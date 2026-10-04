// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// A client on the official TypeScript SDK that exercises the common surface of
// whichever server it reaches: tools, resources, prompts, logging, and an
// error. Usage:
//   node ts-client.mjs stdio <command> [args...]   (launches the command)
//   node ts-client.mjs http <url>                  (streamable HTTP)
// Exits non-zero if any call the server advertises fails.
// INTEROP_SDK selects the SDK package; run.sh points it at an older release
// to record a session at an older protocol revision.
const sdk = process.env.INTEROP_SDK ?? "@modelcontextprotocol/sdk";
const { Client } = await import(`${sdk}/client/index.js`);
const { StdioClientTransport } = await import(`${sdk}/client/stdio.js`);
const { StreamableHTTPClientTransport } = await import(`${sdk}/client/streamableHttp.js`);

const [mode, ...rest] = process.argv.slice(2);
const transport =
  mode === "http"
    ? new StreamableHTTPClientTransport(new URL(rest[0]))
    : new StdioClientTransport({ command: rest[0], args: rest.slice(1), stderr: "inherit" });
const client = new Client({ name: "interop-ts-client", version: "1.0.0" }, { capabilities: {} });

await client.connect(transport);
const caps = client.getServerCapabilities() ?? {};
if (caps.tools) {
  const { tools } = await client.listTools();
  if (tools.some((t) => t.name === "echo")) {
    await client.callTool({ name: "echo", arguments: { message: "interop" } });
  }
  // An unknown tool: the server answers with an error the trace must carry.
  await client.callTool({ name: "no-such-tool", arguments: {} }).catch(() => {});
}
if (caps.resources) {
  const { resources } = await client.listResources();
  if (resources[0]) await client.readResource({ uri: resources[0].uri });
}
if (caps.prompts) {
  const { prompts } = await client.listPrompts();
  const simple = prompts.find((p) => !p.arguments || p.arguments.every((a) => !a.required));
  if (simple) await client.getPrompt({ name: simple.name });
}
if (caps.logging) await client.setLoggingLevel("info");
await client.close();
