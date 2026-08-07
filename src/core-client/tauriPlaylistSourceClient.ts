import { invoke } from "@tauri-apps/api/core";
import { toCoreClientTransportError } from "./coreClientError";
import type { PlaylistSourceClient } from "./PlaylistSourceClient";
import type { ContinuePlaylistSourceOperationDto } from "./generated/ContinuePlaylistSourceOperationDto";
import type { PlaylistSourceClientActionDto } from "./generated/PlaylistSourceClientActionDto";
import type { PlaylistSourceContinuationResultDto } from "./generated/PlaylistSourceContinuationResultDto";
import type { PreparePlaylistSourceOperationDto } from "./generated/PreparePlaylistSourceOperationDto";

/** Tauri transport adapter for Core-owned playlist-source operations. */
export class TauriPlaylistSourceClient implements PlaylistSourceClient {
    async prepare(
        request: PreparePlaylistSourceOperationDto,
    ): Promise<PlaylistSourceClientActionDto> {
        try {
            return await invoke<PlaylistSourceClientActionDto>(
                "prepare_playlist_source_operation",
                { request },
            );
        } catch (error) {
            throw toCoreClientTransportError(
                error,
                "failed to prepare playlist source operation",
            );
        }
    }

    async continue(
        request: ContinuePlaylistSourceOperationDto,
    ): Promise<PlaylistSourceContinuationResultDto> {
        try {
            return await invoke<PlaylistSourceContinuationResultDto>(
                "continue_playlist_source_operation",
                { request },
            );
        } catch (error) {
            throw toCoreClientTransportError(
                error,
                "failed to continue playlist source operation",
            );
        }
    }
}

export const tauriPlaylistSourceClient = new TauriPlaylistSourceClient();
