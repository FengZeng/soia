import type { CommandResultDto } from "./generated/CommandResultDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";
import { TauriCoreClient } from "./tauriCoreClient";

export const tauriCoreClient = new TauriCoreClient();

export const updatePlaybackSessionId = (sessionId: string | null) => {
    tauriCoreClient.updatePlaybackSessionId(sessionId);
};

export const executePlaybackCommand = async (
    command: PlaybackCommandDto,
): Promise<CommandResultDto> => {
    return tauriCoreClient.execute(command);
};
