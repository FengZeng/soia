import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const tempDir = await mkdtemp(path.join(tmpdir(), "soia-websocket-protocol-"));
const tempModulePath = path.join(tempDir, "webSocketProtocol.mjs");
const tempErrorModulePath = path.join(tempDir, "coreClientError.mjs");

try {
  const compile = (source) => ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2020,
      module: ts.ModuleKind.ES2020,
    },
  });
  const sourcePath = new URL("../src/core-client/webSocketProtocol.ts", import.meta.url);
  const source = await readFile(sourcePath, "utf8");
  const compiled = compile(source);
  await writeFile(
    tempModulePath,
    compiled.outputText.replace(
      'from "./coreClientError"',
      'from "./coreClientError.mjs"',
    ),
  );
  const errorSourcePath = new URL("../src/core-client/coreClientError.ts", import.meta.url);
  const errorSource = await readFile(errorSourcePath, "utf8");
  await writeFile(tempErrorModulePath, compile(errorSource).outputText);

  const protocol = await import(pathToFileURL(tempModulePath).href);

  assert.deepEqual(
    protocol.parseWebSocketServerMessage(
      JSON.stringify({ type: "hello", protocol_version: 3 }),
    ),
    { type: "hello", protocolVersion: 3 },
  );
  assert.equal(
    protocol.parseWebSocketServerMessage(
      JSON.stringify({ type: "state", state: { revision: 1 } }),
    ).state.downloadSpeedBps,
    0,
  );
  assert.equal(
    protocol.parseWebSocketServerMessage(
      JSON.stringify({
        type: "state",
        state: { revision: 2, downloadSpeedBps: 1_234_567.5 },
      }),
    ).state.downloadSpeedBps,
    1_234_567.5,
  );
  assert.deepEqual(
    protocol.parseWebSocketServerMessage(
      JSON.stringify({
        type: "error",
        id: "command-7",
        error: {
          type: "stalePlaybackSession",
          message: "playback session has changed",
          requestedPlaybackSessionId: "session-a",
          currentPlaybackSessionId: "session-b",
        },
      }),
    ),
    {
      type: "error",
      id: "command-7",
      error: {
        type: "stalePlaybackSession",
        message: "playback session has changed",
        requestedPlaybackSessionId: "session-a",
        currentPlaybackSessionId: "session-b",
      },
    },
  );
  assert.equal(
    protocol.isNewerSnapshot({ revision: 7 }, { revision: 6 }),
    true,
  );
  assert.equal(
    protocol.isNewerSnapshot({ revision: 5 }, { revision: 6 }),
    false,
  );
  assert.throws(
    () => protocol.parseWebSocketServerMessage('{"type":"hello"}'),
    /protocol version/,
  );
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
