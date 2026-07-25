import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const tempDir = await mkdtemp(path.join(tmpdir(), "soia-core-client-contract-"));
const tempModulePath = path.join(tempDir, "playbackCommandContext.mjs");

try {
  const sourcePath = new URL(
    "../src/core-client/playbackCommandContext.ts",
    import.meta.url,
  );
  const source = await readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2020,
      module: ts.ModuleKind.ES2020,
    },
  });
  await writeFile(tempModulePath, compiled.outputText);

  const { PlaybackCommandContext } = await import(
    pathToFileURL(tempModulePath).href,
  );

  assert.throws(() => new PlaybackCommandContext("  "), /client ID is required/);

  const context = new PlaybackCommandContext("browser-42");
  assert.deepEqual(context.createEnvelope({ type: "setMuted", muted: true }), {
    commandId: "browser-42:1",
    clientId: "browser-42",
    playbackSessionId: null,
    command: { type: "setMuted", muted: true },
  });

  context.updateSnapshot({ playbackSessionId: "session-9" });
  assert.deepEqual(context.createEnvelope({ type: "seekAbsolute", position: 15 }), {
    commandId: "browser-42:2",
    clientId: "browser-42",
    playbackSessionId: "session-9",
    command: { type: "seekAbsolute", position: 15 },
  });
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
