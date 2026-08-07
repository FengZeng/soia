import type { ContinuePlaylistSourceOperationDto } from "./generated/ContinuePlaylistSourceOperationDto";
import type { PlaylistSourceContinuationResultDto } from "./generated/PlaylistSourceContinuationResultDto";
import type { PreparePlaylistSourceOperationDto } from "./generated/PreparePlaylistSourceOperationDto";
import type { PreparePlaylistSourceOperationResultDto } from "./generated/PreparePlaylistSourceOperationResultDto";

/** Client boundary for Core-owned playlist-source operations. */
export interface PlaylistSourceClient {
    prepare(
        request: PreparePlaylistSourceOperationDto,
    ): Promise<PreparePlaylistSourceOperationResultDto>;
    continue(
        request: ContinuePlaylistSourceOperationDto,
    ): Promise<PlaylistSourceContinuationResultDto>;
}
