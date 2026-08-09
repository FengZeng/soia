import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const tempDir = await mkdtemp(path.join(tmpdir(), "soia-playlist-feature-"));
const tempModulePath = path.join(tempDir, "playlistReaderController.mjs");

try {
  const [source, desktopPlaylistState, remotePlaylistPanel] = await Promise.all([
    readFile(
      new URL("../src/features/playlist/playlistReaderController.ts", import.meta.url),
      "utf8",
    ),
    readFile(new URL("../src/composables/usePlaylistState.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/remote/RemotePlaylistPanel.vue", import.meta.url), "utf8"),
  ]);
  const compiled = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ES2020 },
  });
  await writeFile(tempModulePath, compiled.outputText);

  const { createPlaylistReaderController } = await import(pathToFileURL(tempModulePath).href);
  const calls = [];
  let listener;
  let unsubscribeCount = 0;
  const client = {
    getSnapshot: async () => ({ playlists: [] }),
    subscribe: (nextListener) => {
      listener = nextListener;
      return () => { unsubscribeCount += 1; };
    },
    getEntriesPage: async (request) => {
      calls.push({ type: "page", request });
      return { entries: [], total: 0 };
    },
    playEntry: async (request) => {
      calls.push({ type: "play", request });
      return { accepted: true };
    },
  };
  const controller = createPlaylistReaderController(client, "playlist-ui");
  const snapshots = [];
  controller.subscribe((snapshot) => snapshots.push(snapshot));
  listener({ playlists: [{ id: "favorites" }] });
  await controller.getEntriesPage("favorites", 100, 50);
  await controller.playEntry("favorites", "entry-9");
  controller.dispose();

  assert.deepEqual(snapshots, [{ playlists: [{ id: "favorites" }] }]);
  assert.deepEqual(calls[0], {
    type: "page",
    request: { playlistId: "favorites", offset: 100, limit: 50 },
  });
  assert.equal(calls[1].type, "play");
  assert.equal(calls[1].request.clientId, "playlist-ui");
  assert.equal(calls[1].request.playlistId, "favorites");
  assert.equal(calls[1].request.entryId, "entry-9");
  assert.match(calls[1].request.commandId, /^playlist-play-/);
  assert.equal(unsubscribeCount, 1);

  for (const [name, featureSource] of [
    ["Desktop playlist state", desktopPlaylistState],
    ["Remote playlist panel", remotePlaylistPanel],
  ]) {
    assert.match(
      featureSource,
      /createPlaylistReaderController/,
      `${name} must use the shared headless playlist reader`,
    );
  }
  assert.doesNotMatch(
    remotePlaylistPanel,
    /remotePlaylistClient\.playEntry\(/,
    "Remote playlist UI must not construct play-entry requests directly",
  );
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
