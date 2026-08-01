import type { CoreClientError } from "./CoreClient";
import type { CoreErrorDto } from "./generated/CoreErrorDto";

const coreErrorTypes = new Set<CoreErrorDto["type"]>([
    "invalidCommand",
    "executionFailed",
    "navigationFailed",
    "stalePlaybackSession",
    "playlistNotFound",
    "invalidPlaylistMutation",
    "protectedPlaylist",
    "playlistVersionConflict",
    "remotePermissionDenied",
]);

const errorMessage = (error: unknown, fallback: string) => {
    if (error instanceof Error && error.message.trim()) {
        return error.message;
    }
    if (typeof error === "string" && error.trim()) {
        return error;
    }
    return fallback;
};

export const isCoreErrorDto = (error: unknown): error is CoreErrorDto => {
    if (!error || typeof error !== "object") return false;
    const type = (error as { type?: unknown }).type;
    return typeof type === "string" && coreErrorTypes.has(type as CoreErrorDto["type"]);
};

export const toCoreClientTransportError = (
    error: unknown,
    fallback: string,
): CoreClientError =>
    isCoreErrorDto(error)
        ? { type: "core", error }
        : { type: "transport", message: errorMessage(error, fallback) };
