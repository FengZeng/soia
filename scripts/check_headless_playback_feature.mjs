import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const tempDir = await mkdtemp(path.join(tmpdir(), "soia-playback-feature-"));
const tempModulePath = path.join(tempDir, "playbackController.mjs");
const tempDerivedModulePath = path.join(tempDir, "playbackDerivedState.mjs");

try {
  const [source, derivedSource, desktopBindings, desktopController, desktopCommands, remotePanel] = await Promise.all([
    readFile(new URL("../src/features/playback/playbackController.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/features/playback/playbackDerivedState.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/composables/useAppEventBindings.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/composables/usePlaybackController.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/composables/usePlaybackCommands.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/remote/RemotePlaybackPanel.vue", import.meta.url), "utf8"),
  ]);
  const compiled = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ES2020 },
  });
  await writeFile(tempModulePath, compiled.outputText);
  const compiledDerived = ts.transpileModule(derivedSource, {
    compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ES2020 },
  });
  await writeFile(tempDerivedModulePath, compiledDerived.outputText);

  const { createPlaybackController } = await import(pathToFileURL(tempModulePath).href);
  const {
    displayedPlaybackPosition,
    isSeekPositionConfirmed,
    playbackProgressPercent,
  } = await import(pathToFileURL(tempDerivedModulePath).href);
  let emitSnapshot;
  let releaseInitialSnapshot;
  const receivedCommands = [];
  const client = {
    getSnapshot: () => new Promise((resolve) => { releaseInitialSnapshot = resolve; }),
    subscribe: (listener) => {
      emitSnapshot = listener;
      return () => {};
    },
    execute: async (command) => {
      receivedCommands.push(command);
      return { accepted: true };
    },
  };
  const controller = createPlaybackController(client);
  const snapshots = [];
  const stop = controller.start((snapshot) => snapshots.push(snapshot));
  emitSnapshot({ revision: 2, position: 20 });
  releaseInitialSnapshot({ revision: 1, position: 10 });
  await Promise.resolve();
  await controller.execute({ type: "setMuted", muted: true });
  stop();
  emitSnapshot({ revision: 3, position: 30 });

  assert.deepEqual(snapshots, [{ revision: 2, position: 20 }]);
  assert.deepEqual(receivedCommands, [{ type: "setMuted", muted: true }]);
  assert.equal(playbackProgressPercent(100, 120), 100);
  assert.equal(playbackProgressPercent(0, 20), 0);
  assert.equal(displayedPlaybackPosition({ position: 15 }, 40), 40);
  assert.equal(isSeekPositionConfirmed({ position: 39 }, 40), true);
  assert.equal(isSeekPositionConfirmed({ position: 42 }, 40), false);
  for (const [name, featureSource] of [
    ["Desktop event bindings", desktopBindings],
    ["Remote playback panel", remotePanel],
  ]) {
    assert.match(
      featureSource,
      /createPlaybackController/,
      `${name} must use the shared headless playback controller`,
    );
  }
  assert.doesNotMatch(
    desktopBindings,
    /latestPlaybackSnapshotRevision/,
    "Desktop must not duplicate snapshot revision ordering",
  );
  for (const [name, featureSource] of [
    ["Desktop playback controller", desktopController],
    ["Remote playback panel", remotePanel],
  ]) {
    assert.match(
      featureSource,
      /playbackProgressPercent/,
      `${name} must use shared playback derived state`,
    );
  }
  assert.match(
    desktopCommands,
    /Pick<PlaybackController, "execute">/,
    "Desktop core playback commands must execute through the headless controller",
  );
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
