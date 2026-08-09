import type { PlaybackSnapshotDto } from "../../core-client/generated/PlaybackSnapshotDto";

export const playbackProgressPercent = (duration: number, position: number) => {
    if (!Number.isFinite(duration) || duration <= 0 || !Number.isFinite(position)) return 0;
    return Math.max(0, Math.min(100, (position / duration) * 100));
};

export const displayedPlaybackPosition = (
    snapshot: PlaybackSnapshotDto,
    pendingSeekPosition: number | null,
) => pendingSeekPosition ?? snapshot.position;

export const isSeekPositionConfirmed = (
    snapshot: PlaybackSnapshotDto,
    pendingSeekPosition: number,
    toleranceSeconds = 2,
) => Math.abs(snapshot.position - pendingSeekPosition) < toleranceSeconds;
