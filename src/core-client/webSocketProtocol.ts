import { isCoreErrorDto } from "./coreClientError";
import type { CommandResultDto } from "./generated/CommandResultDto";
import type { CoreErrorDto } from "./generated/CoreErrorDto";
import type { PlaybackSnapshotDto } from "./generated/PlaybackSnapshotDto";

export const WEBSOCKET_PROTOCOL_VERSION = 3;

export type WebSocketServerMessage =
    | { type: "hello"; protocolVersion: number }
    | { type: "state"; state: PlaybackSnapshotDto }
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
