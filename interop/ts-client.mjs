// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// A client on the official TypeScript SDK that exercises the common surface of
// whichever server it reaches: tools, resources, prompts, logging, and an
// error. Usage:
//   node ts-client.mjs stdio <command> [args...]   (launches the command)
//   node ts-client.mjs http <url>                  (streamable HTTP)
// Exits non-zero if any call the server advertises fails — unless
// INTEROP_ERRORS=record, which matrix.sh sets: there the session is the
// evidence, so an error answer is logged and the session carries on, and the
// validator judges what the server said rather than the client stopping at it.
// INTEROP_SDK selects the SDK package; run.sh points it at an older release
// to record a session at an older protocol revision.
const sdk = process.env.INTEROP_SDK ?? "@modelcontextprotocol/sdk";
const recordErrors = process.env.INTEROP_ERRORS === "record";
const { Client } = await import(`${sdk}/client/index.js`);
const { StdioClientTransport } = await import(`${sdk}/client/stdio.js`);
const { StreamableHTTPClientTransport } = await import(`${sdk}/client/streamableHttp.js`);

// One advertised call: its result, or — when errors are being recorded — the
// error logged and `undefined` returned.
async function call(what, promise) {
  try {
    return await promise;
  } catch (error) {
    if (!recordErrors) throw error;
    console.error(`ts-client: ${what} answered with an error: ${error.message}`);
    return undefined;
  }
}

// Settles once the standalone GET stream the SDK opens after the handshake
// has been answered, whatever the answer. Until then that GET and the next
// POST are both awaiting a status, and a trace cannot say which status is
// whose — so the validator, rightly, attributes neither, and the session's
// report would depend on which reply won the race.
let standaloneAnswered;
const standalone = new Promise((resolve) => {
  standaloneAnswered = resolve;
});
async function recordingFetch(url, init) {
  const isStandalone = init?.method === "GET";
  try {
    return await fetch(url, init);
  } finally {
    if (isStandalone) standaloneAnswered();
  }
}

const [mode, ...rest] = process.argv.slice(2);
const transport =
  mode === "http"
    ? new StreamableHTTPClientTransport(new URL(rest[0]), { fetch: recordingFetch })
    : new StdioClientTransport({ command: rest[0], args: rest.slice(1), stderr: "inherit" });
const client = new Client({ name: "interop-ts-client", version: "1.0.0" }, { capabilities: {} });

await client.connect(transport);
if (mode === "http") {
  // Bounded, in case a server's answer to `initialized` means no GET is
  // opened at all; the wait ends as soon as the GET is answered.
  await Promise.race([standalone, new Promise((resolve) => setTimeout(resolve, 5000))]);
}
const caps = client.getServerCapabilities() ?? {};
if (caps.tools) {
  const { tools = [] } = (await call("tools/list", client.listTools())) ?? {};
  if (tools.some((t) => t.name === "echo")) {
    await call("tools/call echo", client.callTool({ name: "echo", arguments: { message: "interop" } }));
  }
  // An unknown tool: the server answers with an error the trace must carry.
  await client.callTool({ name: "no-such-tool", arguments: {} }).catch(() => {});
}
if (caps.resources) {
  const { resources = [] } = (await call("resources/list", client.listResources())) ?? {};
  if (resources[0]) await call("resources/read", client.readResource({ uri: resources[0].uri }));
}
if (caps.prompts) {
  const { prompts = [] } = (await call("prompts/list", client.listPrompts())) ?? {};
  const simple = prompts.find((p) => !p.arguments || p.arguments.every((a) => !a.required));
  if (simple) await call("prompts/get", client.getPrompt({ name: simple.name }));
}
if (caps.logging) await call("logging/setLevel", client.setLoggingLevel("info"));
// Over HTTP, end the session explicitly: the DELETE teardown is part of what
// the recording must judge correctly (the TS SDK answers it 200, no body).
if (mode === "http") await transport.terminateSession();
await client.close();
