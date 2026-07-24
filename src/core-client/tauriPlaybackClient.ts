import { invoke } from "@tauri-apps/api/core";
import type { CommandEnvelopeDto } from "./generated/CommandEnvelopeDto";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";

const clientId = "desktop";
let commandSequence = 0;
let playbackSessionId: string | null = null;

export const updatePlaybackSessionId = (sessionId: string | null) => {
    playbackSessionId = sessionId;
};

export const executePlaybackCommand = async (
    command: PlaybackCommandDto,
): Promise<CommandResultDto> => {
    commandSequence += 1;
    const envelope: CommandEnvelopeDto = {
        commandId: `${clientId}:${commandSequence}`,
        clientId,
        playbackSessionId,
        command,
    };
    return invoke<CommandResultDto>("execute_playback_command", { envelope });
};
