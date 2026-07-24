type ParsedPlaylistMetadataLike = {
    hasEndList: boolean;
    playlistType?: string | null;
    hasHlsTags: boolean;
};

export type ParsedPlaylistWorkflow =
    | {
          type: "playSource";
          isLivePlayback: boolean;
      }
    | {
          type: "fallbackToOriginalSource";
      }
    | {
          type: "playFirstEntry";
          paths: string[];
          shouldConfirmPlaylistCreation: boolean;
          isLivePlayback: true;
      };

export const isPlaylistSource = (value: string) => {
    const trimmed = value.trim();
    if (!trimmed) return false;
    try {
        const parsed = new URL(trimmed);
        const pathname = parsed.pathname.toLowerCase();
        return pathname.endsWith(".m3u") || pathname.endsWith(".m3u8");
    } catch {
        const lower = trimmed.toLowerCase();
        return lower.endsWith(".m3u") || lower.endsWith(".m3u8");
    }
};

export const isYoutubePlaylistUrl = (value: string) => {
    const trimmed = value.trim();
    if (!trimmed) return false;
    try {
        const parsed = new URL(trimmed);
        if (
            parsed.hostname !== "www.youtube.com" &&
            parsed.hostname !== "youtube.com" &&
            parsed.hostname !== "music.youtube.com"
        ) {
            return false;
        }
        if (parsed.pathname === "/playlist") return true;
        if (parsed.pathname.startsWith("/show/")) return true;
        return false;
    } catch {
        return false;
    }
};

export const isParsedPlaylistLiveCandidate = (
    metadata: ParsedPlaylistMetadataLike,
) => {
    if (!metadata.hasHlsTags) return true;
    const playlistType = metadata.playlistType?.trim().toUpperCase() ?? "";
    return !metadata.hasEndList && playlistType !== "VOD";
};

export const getParsedPlaylistWorkflow = (
    metadata: ParsedPlaylistMetadataLike,
    entryPaths: string[],
): ParsedPlaylistWorkflow => {
    if (metadata.hasHlsTags) {
        return {
            type: "playSource",
            isLivePlayback: isParsedPlaylistLiveCandidate(metadata),
        };
    }

    const paths = entryPaths.map((path) => path.trim()).filter(Boolean);
    if (!paths.length) {
        return { type: "fallbackToOriginalSource" };
    }

    return {
        type: "playFirstEntry",
        paths,
        shouldConfirmPlaylistCreation: paths.length > 1,
        isLivePlayback: true,
    };
};

export const shouldConfirmYoutubePlaylistCreation = (entryCount: number) =>
    entryCount > 0;

export const shouldConfirmMultiPathPlaylistCreation = (pathCount: number) =>
    pathCount > 1;

export const getPlaylistNameFromSource = (
    source: string,
    fallback?: string,
) => {
    try {
        const parsed = new URL(source);
        const fileName = parsed.pathname.split("/").pop() ?? "";
        const normalized = fileName.replace(/\.(m3u8?|M3U8?)$/, "");
        return normalized.trim() || fallback;
    } catch {
        const fileName = source.split(/[/\\]+/).pop() ?? "";
        const normalized = fileName.replace(/\.(m3u8?|M3U8?)$/, "");
        return normalized.trim() || fallback;
    }
};

export const getUniquePathCount = (paths: string[]) =>
    new Set(paths.map((path) => path.trim()).filter(Boolean)).size;

const collapseHomePath = (path: string) =>
    path.replace(/^\/Users\/[^/]+(?=\/|$)/, "~");

const getParentPath = (path: string) => {
    const normalizedPath = path.trim();
    if (!normalizedPath) return "";
    const separatorIndex = Math.max(
        normalizedPath.lastIndexOf("/"),
        normalizedPath.lastIndexOf("\\"),
    );
    if (separatorIndex <= 0) return normalizedPath;
    return normalizedPath.slice(0, separatorIndex);
};

export const getPlaylistSourceLabel = (source: string) =>
    collapseHomePath(source);

export const getCommonSelectionSourceLabel = (paths: string[]) => {
    const normalizedPaths = paths.map((path) => path.trim()).filter(Boolean);
    if (!normalizedPaths.length) return "";
    if (normalizedPaths.length === 1) {
        return collapseHomePath(normalizedPaths[0]);
    }

    const parentPaths = normalizedPaths.map(getParentPath).filter(Boolean);
    const firstParent = parentPaths[0] ?? "";
    const hasCommonParent = parentPaths.every((path) => path === firstParent);
    return collapseHomePath(hasCommonParent ? firstParent : normalizedPaths[0]);
};
