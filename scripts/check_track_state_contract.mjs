import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const readSource = (relativePath) =>
  readFile(new URL(`../${relativePath}`, import.meta.url), "utf8");

const [eventBindings, mediaTracks, subtitleState, eventLoop] = await Promise.all([
  readSource("src/composables/useAppEventBindings.ts"),
  readSource("src/composables/useMediaTracks.ts"),
  readSource("src/composables/useSubtitleState.ts"),
  readSource("src-tauri/src/mpv/event_loop.rs"),
]);

for (const source of [eventBindings, eventLoop]) {
  assert.equal(
    source.includes("mpv-tracks-update"),
    false,
    "track state must not use the legacy Desktop-only event",
  );
}

assert.match(
  eventBindings,
  /tracks\.handleTracksSnapshot\(snapshot\.tracks\)/,
  "Desktop must consume tracks from the authoritative playback snapshot",
);

assert.match(
  mediaTracks,
  /type: "selectAudioTrack"/,
  "Desktop audio selection must execute through CoreClient",
);
assert.match(
  subtitleState,
  /type: "selectSubtitleTrack"/,
  "Desktop primary subtitle selection must execute through CoreClient",
);
assert.match(
  subtitleState,
  /type: "disableSubtitles"/,
  "Desktop subtitle disabling must execute through CoreClient",
);
