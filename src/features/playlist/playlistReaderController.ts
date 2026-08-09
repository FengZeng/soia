import type { PlaylistReader } from "../../core-client/PlaylistClient";
import type { PlaylistEntriesPageDto } from "../../core-client/generated/PlaylistEntriesPageDto";
import type { PlaylistSnapshotDto } from "../../core-client/generated/PlaylistSnapshotDto";
import type { CommandResultDto } from "../../core-client/generated/CommandResultDto";

const createId = (prefix: string) => {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
        return `${prefix}-${crypto.randomUUID()}`;
    }
    return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
};

export type PlaylistReaderController = {
    getSnapshot: () => Promise<PlaylistSnapshotDto>;
    subscribe: (listener: (snapshot: PlaylistSnapshotDto) => void) => () => void;
    getEntriesPage: (playlistId: string, offset: number, limit: number) => Promise<PlaylistEntriesPageDto>;
    playEntry: (playlistId: string, entryId: string) => Promise<CommandResultDto>;
    dispose: () => void;
};

/**
 * Transport-neutral read/play workflow shared by Desktop and Remote playlist presentation.
 * Editing remains client-specific because the Desktop and Remote capability sets differ.
 */
export const createPlaylistReaderController = (
    client: PlaylistReader,
    clientId = createId("playlist-client"),
): PlaylistReaderController => {
    const unsubscribers = new Set<() => void>();

    return {
        getSnapshot: () => client.getSnapshot(),
        subscribe: (listener) => {
            const unsubscribe = client.subscribe(listener);
            unsubscribers.add(unsubscribe);
            return () => {
                if (!unsubscribers.delete(unsubscribe)) return;
                unsubscribe();
            };
        },
        getEntriesPage: (playlistId, offset, limit) =>
            client.getEntriesPage({ playlistId, offset, limit }),
        playEntry: (playlistId, entryId) => client.playEntry({
            commandId: createId("playlist-play"),
            clientId,
            playlistId,
            entryId,
        }),
        dispose: () => {
            for (const unsubscribe of unsubscribers) unsubscribe();
            unsubscribers.clear();
        },
    };
};
