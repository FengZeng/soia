import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
    CoreClient,
    CoreClientErrorListener,
    CoreClientSnapshotListener,
    CoreClientUnsubscribe,
} from "./CoreClient";
import { toCoreClientTransportError } from "./coreClientError";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";
import type { PlaybackSnapshotDto } from "./generated/PlaybackSnapshotDto";
import { PlaybackCommandContext } from "./playbackCommandContext";

const PLAYBACK_SNAPSHOT_EVENT = "playback-snapshot";

const tauriCommandFor = (command: PlaybackCommandDto) =>
    command.type === "previous" ||
    command.type === "next" ||
    command.type === "playSource"
        ? "execute_navigation_command"
        : "execute_playback_command";

export class TauriCoreClient implements CoreClient {
    private readonly commandContext: PlaybackCommandContext;

    constructor(clientId = "desktop") {
        this.commandContext = new PlaybackCommandContext(clientId);
    }

    async getSnapshot(): Promise<PlaybackSnapshotDto> {
        try {
            const snapshot = await invoke<PlaybackSnapshotDto>(
                "get_playback_snapshot",
            );
            this.commandContext.updateSnapshot(snapshot);
            return snapshot;
        } catch (error) {
            throw toCoreClientTransportError(
                error,
                "failed to retrieve playback snapshot",
            );
        }
    }

    subscribe(
        listener: CoreClientSnapshotListener,
        onError?: CoreClientErrorListener,
    ): CoreClientUnsubscribe {
        let disposed = false;
        let unlisten: UnlistenFn | null = null;

        void listen<PlaybackSnapshotDto>(
            PLAYBACK_SNAPSHOT_EVENT,
            (event) => {
                if (disposed) return;
                this.commandContext.updateSnapshot(event.payload);
                listener(event.payload);
            },
        )
            .then((nextUnlisten) => {
                if (disposed) {
                    nextUnlisten();
                    return;
                }
                unlisten = nextUnlisten;
            })
            .catch((error) => {
                if (disposed) return;
                onError?.(
                    toCoreClientTransportError(
                        error,
                        "failed to subscribe to playback snapshots",
                    ),
                );
            });

        return () => {
            if (disposed) return;
            disposed = true;
            unlisten?.();
        };
    }

    async execute(command: PlaybackCommandDto): Promise<CommandResultDto> {
        const envelope = this.commandContext.createEnvelope(command);
        try {
            return await invoke<CommandResultDto>(tauriCommandFor(command), {
                envelope,
            });
        } catch (error) {
            throw toCoreClientTransportError(
                error,
                "failed to execute playback command",
            );
        }
    }
}
