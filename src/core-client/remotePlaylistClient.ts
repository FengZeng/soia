import type { RemotePlaylistClient as RemotePlaylistClientContract } from "./PlaylistClient";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { DeletePlaylistDto } from "./generated/DeletePlaylistDto";
import type { GetPlaylistEntriesPageDto } from "./generated/GetPlaylistEntriesPageDto";
import type { ImportPlaylistFromSourceDto } from "./generated/ImportPlaylistFromSourceDto";
import type { PlayPlaylistEntryDto } from "./generated/PlayPlaylistEntryDto";
import type { PlaylistEntriesPageDto } from "./generated/PlaylistEntriesPageDto";
import type { PlaylistMutationResultDto } from "./generated/PlaylistMutationResultDto";
import type { PlaylistSnapshotDto } from "./generated/PlaylistSnapshotDto";
import type { WebSocketCoreClient } from "./webSocketCoreClient";

/** Remote playlist reader backed by the already-paired Core WebSocket connection. */
export class RemotePlaylistClient implements RemotePlaylistClientContract {
    constructor(private readonly coreClient: WebSocketCoreClient) {}

    getSnapshot(): Promise<PlaylistSnapshotDto> {
        return this.coreClient.getPlaylistSnapshot();
    }

    subscribe(listener: (snapshot: PlaylistSnapshotDto) => void): () => void {
        return this.coreClient.subscribePlaylist(listener);
    }

    getEntriesPage(request: GetPlaylistEntriesPageDto): Promise<PlaylistEntriesPageDto> {
        return this.coreClient.getPlaylistEntriesPage(request);
    }

    playEntry(request: PlayPlaylistEntryDto): Promise<CommandResultDto> {
        return this.coreClient.playPlaylistEntry(request);
    }

    delete(request: DeletePlaylistDto): Promise<PlaylistMutationResultDto> {
        return this.coreClient.deletePlaylist(request);
    }

    importFromSource(
        request: ImportPlaylistFromSourceDto,
    ): Promise<PlaylistMutationResultDto> {
        return this.coreClient.importPlaylistFromSource(request);
    }
}
