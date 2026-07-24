import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const tempDir = await mkdtemp(path.join(tmpdir(), "soia-playlist-source-workflows-"));
const tempModulePath = path.join(tempDir, "playlistSourceWorkflow.mjs");

try {
  const sourcePath = new URL(
    "../src/utils/playlistSourceWorkflow.ts",
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

  const workflow = await import(pathToFileURL(tempModulePath).href);

  assert.equal(workflow.isPlaylistSource("/media/channels.M3U"), true);
  assert.equal(
    workflow.isPlaylistSource("https://media.example/live/list.m3u8?token=abc"),
    true,
  );
  assert.equal(workflow.isPlaylistSource("https://media.example/video.mp4"), false);

  assert.equal(
    workflow.isYoutubePlaylistUrl("https://www.youtube.com/playlist?list=PL1"),
    true,
  );
  assert.equal(
    workflow.isYoutubePlaylistUrl("https://music.youtube.com/show/example"),
    true,
  );
  assert.equal(
    workflow.isYoutubePlaylistUrl("https://www.youtube.com/watch?v=video&list=PL1"),
    false,
  );

  assert.equal(
    workflow.isParsedPlaylistLiveCandidate({
      hasEndList: false,
      playlistType: null,
      hasHlsTags: false,
    }),
    true,
  );
  assert.equal(
    workflow.isParsedPlaylistLiveCandidate({
      hasEndList: false,
      playlistType: null,
      hasHlsTags: true,
    }),
    true,
  );
  assert.equal(
    workflow.isParsedPlaylistLiveCandidate({
      hasEndList: true,
      playlistType: null,
      hasHlsTags: true,
    }),
    false,
  );

  assert.deepEqual(
    workflow.getParsedPlaylistWorkflow(
      { hasEndList: false, playlistType: null, hasHlsTags: true },
      ["segment.ts"],
    ),
    { type: "playSource", isLivePlayback: true },
  );
  assert.deepEqual(
    workflow.getParsedPlaylistWorkflow(
      { hasEndList: true, playlistType: null, hasHlsTags: true },
      ["segment.ts"],
    ),
    { type: "playSource", isLivePlayback: false },
  );
  assert.deepEqual(
    workflow.getParsedPlaylistWorkflow(
      { hasEndList: false, playlistType: null, hasHlsTags: false },
      [],
    ),
    { type: "fallbackToOriginalSource" },
  );
  assert.deepEqual(
    workflow.getParsedPlaylistWorkflow(
      { hasEndList: false, playlistType: null, hasHlsTags: false },
      [" first.mp4 "],
    ),
    {
      type: "playFirstEntry",
      paths: ["first.mp4"],
      shouldConfirmPlaylistCreation: false,
      isLivePlayback: true,
    },
  );
  assert.deepEqual(
    workflow.getParsedPlaylistWorkflow(
      { hasEndList: false, playlistType: null, hasHlsTags: false },
      ["first.mp4", "second.mp4"],
    ),
    {
      type: "playFirstEntry",
      paths: ["first.mp4", "second.mp4"],
      shouldConfirmPlaylistCreation: true,
      isLivePlayback: true,
    },
  );

  assert.equal(workflow.shouldConfirmYoutubePlaylistCreation(0), false);
  assert.equal(workflow.shouldConfirmYoutubePlaylistCreation(1), true);
  assert.equal(workflow.shouldConfirmMultiPathPlaylistCreation(1), false);
  assert.equal(workflow.shouldConfirmMultiPathPlaylistCreation(2), true);
  assert.equal(
    workflow.isParsedPlaylistLiveCandidate({
      hasEndList: false,
      playlistType: "vod",
      hasHlsTags: true,
    }),
    false,
  );

  assert.equal(
    workflow.getPlaylistNameFromSource("https://media.example/lists/News.m3u8"),
    "News",
  );
  assert.equal(
    workflow.getPlaylistNameFromSource("C:\\Media\\Channels.m3u"),
    "\\Media\\Channels",
  );
  assert.equal(workflow.getPlaylistNameFromSource("https://media.example/", "IPTV"), "IPTV");

  assert.equal(workflow.getUniquePathCount([" a ", "a", "", "b"]), 2);
  assert.equal(
    workflow.getPlaylistSourceLabel("/Users/feng/Media/list.m3u"),
    "~/Media/list.m3u",
  );
  assert.equal(
    workflow.getCommonSelectionSourceLabel([
      "/Users/feng/Media/a.mkv",
      "/Users/feng/Media/b.mkv",
    ]),
    "~/Media",
  );
  assert.equal(
    workflow.getCommonSelectionSourceLabel([
      "/Users/feng/Media/a.mkv",
      "/Volumes/share/b.mkv",
    ]),
    "~/Media/a.mkv",
  );
} finally {
  await rm(tempDir, { recursive: true, force: true });
}
