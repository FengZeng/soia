import type { CoreClient, CoreClientError } from "../../core-client/CoreClient";
import type { CommandResultDto } from "../../core-client/generated/CommandResultDto";
import type { PlaybackCommandDto } from "../../core-client/generated/PlaybackCommandDto";
import type { PlaybackSnapshotDto } from "../../core-client/generated/PlaybackSnapshotDto";

export type PlaybackSnapshotListener = (snapshot: PlaybackSnapshotDto) => void;
export type PlaybackErrorListener = (error: CoreClientError) => void;

export type PlaybackController = {
    start: (
        onSnapshot: PlaybackSnapshotListener,
        onError?: PlaybackErrorListener,
    ) => () => void;
    execute: (command: PlaybackCommandDto) => Promise<CommandResultDto>;
    dispose: () => void;
};

/**
 * Transport-neutral playback subscription and command workflow.
 * Presentation state, platform effects, and connection UI remain client-owned.
 */
export const createPlaybackController = (client: CoreClient): PlaybackController => {
    let latestRevision = -1;
    let unsubscribe: (() => void) | null = null;
    let disposed = false;

    const dispose = () => {
        if (disposed) return;
        disposed = true;
        unsubscribe?.();
        unsubscribe = null;
    };

    return {
        start: (onSnapshot, onError) => {
            if (disposed) return () => {};
            const applySnapshot = (snapshot: PlaybackSnapshotDto) => {
                if (disposed || snapshot.revision < latestRevision) return;
                latestRevision = snapshot.revision;
                onSnapshot(snapshot);
            };
            const handleError = (error: CoreClientError) => {
                if (!disposed) onError?.(error);
            };
            unsubscribe?.();
            unsubscribe = client.subscribe(applySnapshot, handleError);
            void client.getSnapshot().then(applySnapshot).catch(handleError);
            return dispose;
        },
        execute: (command) => client.execute(command),
        dispose,
    };
};
