import { isCoreErrorDto } from "./coreClientError";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { CoreErrorDto } from "./generated/CoreErrorDto";
import type { PlaybackSnapshotDto } from "./generated/PlaybackSnapshotDto";
import type { PlaylistSnapshotDto } from "./generated/PlaylistSnapshotDto";
import type { PlaylistEntriesPageDto } from "./generated/PlaylistEntriesPageDto";
import type { PlaylistSummaryDto } from "./generated/PlaylistSummaryDto";
import type { NetworkBrowseResultDto } from "./generated/NetworkBrowseResultDto";
import type { NetworkConnectionSummaryDto } from "./generated/NetworkConnectionSummaryDto";

export const WEBSOCKET_PROTOCOL_VERSION = 6;

export type WebSocketServerMessage =
    | { type: "hello"; protocolVersion: number }
    | { type: "state"; state: PlaybackSnapshotDto }
    | { type: "playlistSnapshot"; id?: string | null; snapshot: PlaylistSnapshotDto }
    | { type: "playlistSummaries"; id?: string | null; playlists: PlaylistSummaryDto[] }
    | { type: "playlistEntriesPage"; id?: string | null; page: PlaylistEntriesPageDto }
    | { type: "playlistDeleted"; id?: string | null; playlistId: string; collectionRevision: number }
    | { type: "playlistImported"; id?: string | null; playlist: PlaylistSummaryDto; collectionRevision: number }
    | { type: "networkConnections"; id?: string | null; connections: NetworkConnectionSummaryDto[] }
    | { type: "networkBrowseResult"; id?: string | null; result: NetworkBrowseResultDto }
    | { type: "pong"; id?: string | null }
    | { type: "commandResult"; result: CommandResultDto }
    | { type: "navigationResult"; id?: string | null; ok: boolean }
    | { type: "error"; id?: string | null; error: CoreErrorDto };

const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === "object" ? (value as Record<string, unknown>) : null;

export const parseWebSocketServerMessage = (
    raw: string,
): WebSocketServerMessage => {
    let parsed: unknown;
    try {
        parsed = JSON.parse(raw);
    } catch {
        throw new Error("received invalid WebSocket JSON");
    }

    const message = asRecord(parsed);
    const type = message?.type;
    if (!message || typeof type !== "string") {
        throw new Error("received WebSocket message without a type");
    }

    switch (type) {
        case "hello": {
            const protocolVersion = message.protocol_version;
            if (typeof protocolVersion !== "number") {
                throw new Error("received hello without a protocol version");
            }
            return { type, protocolVersion };
        }
        case "state": {
            const state = asRecord(message.state);
            if (!state || typeof state.revision !== "number") {
                throw new Error("received invalid playback snapshot");
            }
            return {
                type,
                state: {
                    ...state,
                    downloadSpeedBps:
                        typeof state.downloadSpeedBps === "number" &&
                        Number.isFinite(state.downloadSpeedBps)
                            ? Math.max(0, state.downloadSpeedBps)
                            : 0,
                } as PlaybackSnapshotDto,
            };
        }
        case "playlistSnapshot": {
            const snapshot = asRecord(message.snapshot);
            if (!snapshot || typeof snapshot.collectionRevision !== "number") {
                throw new Error("received invalid playlist snapshot");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                snapshot: snapshot as PlaylistSnapshotDto,
            };
        }
        case "playlistSummaries": {
            if (!Array.isArray(message.playlists)) {
                throw new Error("received invalid playlist summaries");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                playlists: message.playlists as PlaylistSummaryDto[],
            };
        }
        case "playlistEntriesPage": {
            const page = asRecord(message.page);
            if (!page || typeof page.playlistId !== "string" || typeof page.playlistRevision !== "number") {
                throw new Error("received invalid playlist entries page");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                page: page as PlaylistEntriesPageDto,
            };
        }
        case "playlistDeleted":
            if (typeof message.playlist_id !== "string" || typeof message.collection_revision !== "number") {
                throw new Error("received invalid playlist deletion result");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                playlistId: message.playlist_id,
                collectionRevision: message.collection_revision,
            };
        case "playlistImported": {
            const playlist = asRecord(message.playlist);
            if (!playlist || typeof playlist.id !== "string" || typeof message.collection_revision !== "number") {
                throw new Error("received invalid playlist import result");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                playlist: playlist as PlaylistSummaryDto,
                collectionRevision: message.collection_revision,
            };
        }
        case "networkConnections":
            if (!Array.isArray(message.connections)) throw new Error("received invalid network connections");
            return { type, id: typeof message.id === "string" ? message.id : null, connections: message.connections as NetworkConnectionSummaryDto[] };
        case "networkBrowseResult": {
            const result = asRecord(message.result);
            if (!result || typeof result.path !== "string" || !Array.isArray(result.entries)) throw new Error("received invalid network browse result");
            return { type, id: typeof message.id === "string" ? message.id : null, result: result as NetworkBrowseResultDto };
        }
        case "pong":
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
            };
        case "commandResult": {
            const result = asRecord(message.result);
            if (!result || typeof result.commandId !== "string") {
                throw new Error("received invalid command result");
            }
            return { type, result: result as CommandResultDto };
        }
        case "navigationResult":
            if (typeof message.ok !== "boolean") {
                throw new Error("received invalid navigation result");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                ok: message.ok,
            };
        case "error":
            if (!isCoreErrorDto(message.error)) {
                throw new Error("received invalid Core error");
            }
            return {
                type,
                id: typeof message.id === "string" ? message.id : null,
                error: message.error,
            };
        default:
            throw new Error(`received unsupported WebSocket message type: ${type}`);
    }
};

export const isNewerSnapshot = (
    candidate: PlaybackSnapshotDto,
    current: PlaybackSnapshotDto | null,
) => !current || candidate.revision >= current.revision;
