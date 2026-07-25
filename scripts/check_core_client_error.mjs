import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const tempDir = await mkdtemp(path.join(tmpdir(), "soia-core-client-error-"));
const tempModulePath = path.join(tempDir, "coreClientError.mjs");

try {
  const sourcePath = new URL("../src/core-client/coreClientError.ts", import.meta.url);
  const source = await readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2020,
      module: ts.ModuleKind.ES2020,
    },
  });
  await writeFile(tempModulePath, compiled.outputText);

  const { toCoreClientTransportError } = await import(
    pathToFileURL(tempModulePath).href,
  );

  const coreError = {
    type: "stalePlaybackSession",
    message: "playback session has changed",
    requestedPlaybackSessionId: "session-a",
    currentPlaybackSessionId: "session-b",
  };
  assert.deepEqual(
    toCoreClientTransportError(coreError, "fallback"),
    { type: "core", error: coreError },
  );
  assert.deepEqual(
    toCoreClientTransportError(new Error("Tauri unavailable"), "fallback"),
    { type: "transport", message: "Tauri unavailable" },
  );
  assert.deepEqual(toCoreClientTransportError(null, "fallback"), {
    type: "transport",
    message: "fallback",
  });
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
