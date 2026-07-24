import { invoke } from "@tauri-apps/api/core";
import type { CommandEnvelopeDto } from "../core-client/generated/CommandEnvelopeDto";
import type { CommandResultDto } from "../core-client/generated/CommandResultDto";
import type { PlaybackDirection } from "../utils/playbackNavigation";

type PlaylistApi = {
    getPathForEnd: (currentPath: string) => string | null;
    getTitleForPath: (path: string) => string | undefined;
};

type PlayerApi = {
    state: { media: { url: string } };
};

type UsePlaybackNavigationOptions = {
    player: PlayerApi;
    playlistState: PlaylistApi;
    playPath: (path: string, preferredTitle?: string) => Promise<void>;
};

let navigationCommandCounter = 0;

const createNavigationEnvelope = (
    command: CommandEnvelopeDto["command"],
): CommandEnvelopeDto => ({
    commandId: `nav-desktop-${Date.now()}-${++navigationCommandCounter}`,
    clientId: "desktop",
    playbackSessionId: null,
    command,
});

export const usePlaybackNavigation = (_options: UsePlaybackNavigationOptions) => {
    const playTrack = async (direction: PlaybackDirection) => {
        const envelope = createNavigationEnvelope(
            direction === 1 ? { type: "next" } : { type: "previous" },
        );
        await invoke<CommandResultDto>("execute_navigation_command", { envelope }).catch(
            () => {
                // Navigation failed (no adjacent media, etc.) — silently ignore
            },
        );
    };

    // EOF auto-play is now handled by Core (mpv event loop → handle_end_of_file).
    // This is a no-op retained for interface compatibility.
    const playNextAfterEnd = async () => {
        // Core handles EOF auto-play directly in the Rust event loop.
        // No frontend action needed.
    };

    return {
        playPreviousTrack: () => playTrack(-1),
        playNextTrack: () => playTrack(1),
        playNextAfterEnd,
    };
};
