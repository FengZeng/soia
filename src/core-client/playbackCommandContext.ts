import type { CommandEnvelopeDto } from "./generated/CommandEnvelopeDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";
import type { PlaybackSnapshotDto } from "./generated/PlaybackSnapshotDto";

/**
 * Shared per-client command identity and current playback-session state.
 * Adapters own the context for their lifetime and reuse one envelope when they
 * retry an in-flight command.
 */
export class PlaybackCommandContext {
    private commandSequence = 0;
    private playbackSessionId: string | null = null;

    constructor(private readonly clientId: string) {
        if (!clientId.trim()) {
            throw new Error("Core client ID is required");
        }
    }

    updateSnapshot(snapshot: PlaybackSnapshotDto) {
        this.playbackSessionId = snapshot.playbackSessionId;
    }

    updatePlaybackSessionId(sessionId: string | null) {
        this.playbackSessionId = sessionId;
    }

    createEnvelope(command: PlaybackCommandDto): CommandEnvelopeDto {
        this.commandSequence += 1;
        return {
            commandId: `${this.clientId}:${this.commandSequence}`,
            clientId: this.clientId,
            playbackSessionId: this.playbackSessionId,
            command,
        };
    }
}
