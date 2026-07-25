import type { CoreClient } from "../core-client/CoreClient";
import type { PlaybackDirection } from "../utils/playbackNavigation";

type UsePlaybackNavigationOptions = {
    coreClient: CoreClient;
};

export const usePlaybackNavigation = ({ coreClient }: UsePlaybackNavigationOptions) => {
    const playTrack = async (direction: PlaybackDirection) => {
        await coreClient.execute(
            direction === 1 ? { type: "next" } : { type: "previous" },
        ).catch(() => {
            // Navigation failed (no adjacent media, etc.) — silently ignore.
        });
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
