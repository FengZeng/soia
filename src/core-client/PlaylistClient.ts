import type { CommandResultDto } from "./generated/CommandResultDto";
import type { CreatePlaylistDto } from "./generated/CreatePlaylistDto";
import type { DeletePlaylistDto } from "./generated/DeletePlaylistDto";
import type { GetPlaylistEntriesPageDto } from "./generated/GetPlaylistEntriesPageDto";
import type { ImportPlaylistFromSourceDto } from "./generated/ImportPlaylistFromSourceDto";
import type { PlayPlaylistEntryDto } from "./generated/PlayPlaylistEntryDto";
import type { PlaylistEntriesPageDto } from "./generated/PlaylistEntriesPageDto";
import type { PlaylistMutationDto } from "./generated/PlaylistMutationDto";
import type { PlaylistMutationResultDto } from "./generated/PlaylistMutationResultDto";
import type { PlaylistSnapshotDto } from "./generated/PlaylistSnapshotDto";

export type PlaylistSnapshotListener = (snapshot: PlaylistSnapshotDto) => void;
export type PlaylistUnsubscribe = () => void;

/** Read/play capability shared by Desktop and Remote playlist clients. */
export interface PlaylistReader {
    getSnapshot(): Promise<PlaylistSnapshotDto>;
    subscribe(listener: PlaylistSnapshotListener): PlaylistUnsubscribe;
    getEntriesPage(request: GetPlaylistEntriesPageDto): Promise<PlaylistEntriesPageDto>;
    playEntry(request: PlayPlaylistEntryDto): Promise<CommandResultDto>;
}

/** Full playlist editing capability available only through the Desktop adapter. */
export interface DesktopPlaylistEditor extends PlaylistReader {
    create(request: CreatePlaylistDto): Promise<PlaylistMutationResultDto>;
    mutate(mutation: PlaylistMutationDto): Promise<PlaylistMutationResultDto>;
}

/** Deliberately narrow Remote playlist capability surface. */
export interface RemotePlaylistClient extends PlaylistReader {
    delete(request: DeletePlaylistDto): Promise<PlaylistMutationResultDto>;
    importFromSource(
        request: ImportPlaylistFromSourceDto,
    ): Promise<PlaylistMutationResultDto>;
}
