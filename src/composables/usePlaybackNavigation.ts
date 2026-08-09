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

    return {
        playPreviousTrack: () => playTrack(-1),
        playNextTrack: () => playTrack(1),
    };
};
