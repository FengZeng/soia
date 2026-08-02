export type MediaInfo = {
    title: string;
    path: string;
    container: string;
    size: string;
    duration: string;
    badges: string[];
    video: {
        codec: string;
        resolution: string;
        fps: string;
        bitRate: string;
        color: string;
        aspect: string;
    };
    audio: {
        codec: string;
        channels: string;
        sampleRate: string;
        language: string;
    };
    subtitles: string[];
};

type RuntimeMediaInfo = {
    durationSeconds?: number | null;
    fileSizeBytes?: number | null;
};

const fallbackPath = "/Media/Movies/2024/Nature_4K_HDR.mkv";

const baseInfo: Omit<MediaInfo, "title" | "path" | "badges"> = {
    container: "Matroska (MKV)",
    size: "6.4 GB",
    duration: "01:42:19",
    video: {
        codec: "H.264",
        resolution: "1920×1080",
        fps: "24 fps",
        bitRate: "8.1 Mbps",
        color: "SDR (BT.709)",
        aspect: "16:9",
    },
    audio: {
        codec: "AAC",
        channels: "2.0",
        sampleRate: "44.1 kHz",
        language: "English",
    },
    subtitles: ["English (SRT)", "Off"],
};

const resolveTitle = (path: string) => {
    if (!path) return "Media";
    const segment = path.split(/[/\\]/).filter(Boolean).pop();
    return segment || "Media";
};

const hasKeyword = (source: string, keyword: string) =>
    source.toLowerCase().includes(keyword.toLowerCase());

const hasDvToken = (source: string) =>
    /(^|[^a-z0-9])dv([^a-z0-9]|$)/i.test(source);

const isDolbyVisionSource = (source: string) =>
    hasKeyword(source, "dolbyvision") ||
    hasKeyword(source, "dolby vision") ||
    hasDvToken(source);

const isHdrColorSource = (source: string) =>
    hasKeyword(source, "hdr") ||
    hasKeyword(source, "hdr10") ||
    hasKeyword(source, "hdr10+") ||
    hasKeyword(source, "pq") ||
    hasKeyword(source, "hlg") ||
    hasKeyword(source, "bt2020") ||
    hasKeyword(source, "bt.2020") ||
    hasKeyword(source, "rec2020") ||
    hasKeyword(source, "rec.2020");

const isHdrPlusSource = (source: string) =>
    hasKeyword(source, "hdr10+") ||
    hasKeyword(source, "hdr+") ||
    hasKeyword(source, "hdrplus");

const isHdrSource = (path: string) =>
    isHdrColorSource(path) || isDolbyVisionSource(path);

const formatDuration = (seconds?: number | null) => {
    if (
        typeof seconds !== "number" ||
        !Number.isFinite(seconds) ||
        seconds <= 0
    ) {
        return "Unknown duration";
    }
    const total = Math.floor(seconds);
    const hours = Math.floor(total / 3600)
        .toString()
        .padStart(2, "0");
    const minutes = Math.floor((total % 3600) / 60)
        .toString()
        .padStart(2, "0");
    const secs = Math.floor(total % 60)
        .toString()
        .padStart(2, "0");
    return `${hours}:${minutes}:${secs}`;
};

const formatSize = (bytes?: number | null) => {
    if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes <= 0) {
        return "Unknown size";
    }
    const units = ["B", "KB", "MB", "GB", "TB"];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
        value /= 1024;
        unitIndex += 1;
    }
    const precision = value >= 100 || unitIndex === 0 ? 0 : 1;
    return `${value.toFixed(precision)} ${units[unitIndex]}`;
};

const buildBadges = (path: string, info: MediaInfo) => {
    const badges: string[] = [];
    const isDolbyVision =
        isDolbyVisionSource(path) ||
        isDolbyVisionSource(info.video.codec) ||
        isDolbyVisionSource(info.video.color);
    if (isDolbyVision) {
        badges.push("Dolby Vision");
        return badges;
    }
    if (isHdrColorSource(info.video.color) || isHdrSource(path)) {
        badges.push("HDR");
    }
    if (
        isHdrPlusSource(path) ||
        isHdrPlusSource(info.video.codec) ||
        isHdrPlusSource(info.video.color)
    ) {
        badges.push("HDR+");
    }
    if (
        hasKeyword(info.audio.codec, "dolby") ||
        hasKeyword(info.video.codec, "dolby") ||
        hasKeyword(info.video.color, "dolby")
    ) {
        badges.push("Dolby");
    }
    return badges;
};

export const getMockMediaInfo = (
    path: string,
    runtimeInfo?: RuntimeMediaInfo,
): MediaInfo => {
    const safePath = path || fallbackPath;
    const title = resolveTitle(safePath);
    const isDolbyVision = isDolbyVisionSource(safePath);
    const isHdr = isHdrSource(safePath) || isDolbyVision;

    const info: MediaInfo = {
        ...baseInfo,
        title,
        path: safePath,
        size: formatSize(runtimeInfo?.fileSizeBytes),
        duration: formatDuration(runtimeInfo?.durationSeconds),
        video: {
            ...baseInfo.video,
            codec: isHdr ? "HEVC (H.265)" : baseInfo.video.codec,
            resolution: isHdr ? "3840×2160" : baseInfo.video.resolution,
            fps: isHdr ? "23.976 fps" : baseInfo.video.fps,
            bitRate: isHdr ? "24.2 Mbps" : baseInfo.video.bitRate,
            color: isDolbyVision
                ? "Dolby Vision"
                : isHdr
                  ? "HDR10"
                  : baseInfo.video.color,
        },
        audio: { ...baseInfo.audio },
        subtitles: isHdr
            ? ["English (SRT)", "Chinese (SRT)", "Japanese (ASS)"]
            : baseInfo.subtitles,
        badges: [],
    };

    info.badges = buildBadges(safePath, info);
    return info;
};
