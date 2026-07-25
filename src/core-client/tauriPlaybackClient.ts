import { invoke } from "@tauri-apps/api/core";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";
import { PlaybackCommandContext } from "./playbackCommandContext";

const commandContext = new PlaybackCommandContext("desktop");

export const updatePlaybackSessionId = (sessionId: string | null) => {
    commandContext.updatePlaybackSessionId(sessionId);
};

export const executePlaybackCommand = async (
    command: PlaybackCommandDto,
): Promise<CommandResultDto> => {
    const envelope = commandContext.createEnvelope(command);
    return invoke<CommandResultDto>("execute_playback_command", { envelope });
};
