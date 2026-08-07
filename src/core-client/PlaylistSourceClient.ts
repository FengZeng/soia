import type { ContinuePlaylistSourceOperationDto } from "./generated/ContinuePlaylistSourceOperationDto";
import type { PlaylistSourceClientActionDto } from "./generated/PlaylistSourceClientActionDto";
import type { PlaylistSourceContinuationResultDto } from "./generated/PlaylistSourceContinuationResultDto";
import type { PreparePlaylistSourceOperationDto } from "./generated/PreparePlaylistSourceOperationDto";

/** Client boundary for Core-owned playlist-source operations. */
export interface PlaylistSourceClient {
    prepare(
        request: PreparePlaylistSourceOperationDto,
    ): Promise<PlaylistSourceClientActionDto>;
    continue(
        request: ContinuePlaylistSourceOperationDto,
    ): Promise<PlaylistSourceContinuationResultDto>;
}
