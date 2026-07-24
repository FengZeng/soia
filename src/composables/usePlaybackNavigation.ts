import { invoke } from "@tauri-apps/api/core";
import type { PlayerApi } from "./usePlaybackController";
import { resolveAdjacentPathInSameDirectory } from "./usePlaybackAdjacency";
import {
    resolvePlaybackNavigationPath,
    type PlaybackDirection,
} from "../utils/playbackNavigation";
import type { CommandEnvelopeDto } from "../core-client/generated/CommandEnvelopeDto";
import type { CommandResultDto } from "../core-client/generated/CommandResultDto";

type PlaylistApi = {
    getPathForEnd: (currentPath: string) => string | null;
    getTitleForPath: (path: string) => string | undefined;
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

export const usePlaybackNavigation = ({
    player,
    playlistState,
    playPath,
}: UsePlaybackNavigationOptions) => {
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

    const playNextAfterEnd = async () => {
        const currentPath = player.state.media.url;
        const nextPath = await resolvePlaybackNavigationPath({
            currentPath,
            direction: 1,
            resolvePlaylistPath: () => playlistState.getPathForEnd(currentPath),
            resolveDirectoryPath: resolveAdjacentPathInSameDirectory,
        });
        if (!nextPath) return;
        await playPath(nextPath, playlistState.getTitleForPath(nextPath));
    };

    return {
        playPreviousTrack: () => playTrack(-1),
        playNextTrack: () => playTrack(1),
        playNextAfterEnd,
    };
};
