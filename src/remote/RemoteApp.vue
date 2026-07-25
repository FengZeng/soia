<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from "vue";
import SeekBar from "../components/player-controls/SeekBar.vue";
import type { CoreClientError } from "../core-client/CoreClient";
import { coreClientKey } from "../core-client/coreClientKey";
import type { PlaybackSnapshotDto } from "../core-client/generated/PlaybackSnapshotDto";
import type { PlaybackCommandDto } from "../core-client/generated/PlaybackCommandDto";
import type { MediaTrackDto } from "../core-client/generated/MediaTrackDto";
import { remoteConnectionState } from "./remoteCoreClient";
import soiaIconUrl from "../../src-tauri/icons/128x128@2x.png";

const state = ref<PlaybackSnapshotDto>({
    protocolVersion: 3,
    revision: 0,
    playbackSessionId: null,
    title: null,
    duration: 0,
    position: 0,
    bufferedPosition: 0,
    isPlaying: false,
    isBuffering: false,
    sourceLoading: false,
    sourceLoadingKey: null,
    sourceLoadError: null,
    speed: 1,
    volume: 100,
    muted: false,
    tracks: [],
    playlistPosition: -1,
    playlistCount: 0,
});
const error = ref("");
const pendingSeek = ref<number | null>(null);
const playbackRates = [2, 1.75, 1.5, 1.25, 1, 0.75, 0.5, 0.25];
const canControl = computed(() => connectionState.value === "Connected");
const durationLabel = computed(() => formatTime(state.value.duration));
const displayedPosition = computed(() => pendingSeek.value ?? state.value.position);
const progressPercent = computed(() => state.value.duration > 0
    ? displayedPosition.value / state.value.duration * 100
    : 0);
const audioTracks = computed(() =>
    state.value.tracks.filter((track) => track.trackType === "audio"),
);
const subtitleTracks = computed(() =>
    state.value.tracks.filter((track) => track.trackType === "sub"),
);
const selectedAudioTrackId = computed(
    () => audioTracks.value.find((track) => track.selected)?.id ?? "",
);
const selectedSubtitleTrackId = computed(
    () => subtitleTracks.value.find((track) => track.selected)?.id ?? 0,
);
const statusMessage = computed(() => {
    if (pendingSeek.value !== null) return "Seeking…";
    if (state.value.sourceLoading) return "Preparing source…";
    if (error.value) return error.value;
    return "";
});
const statusIsError = computed(
    () => Boolean(error.value) && pendingSeek.value === null && !state.value.sourceLoading,
);

function formatTime(seconds: number) {
    const value = Math.max(0, Math.floor(seconds || 0));
    const minutes = Math.floor(value / 60);
    return `${minutes}:${String(value % 60).padStart(2, "0")}`;
}

const connectionLabels = {
    idle: "Connecting…",
    pairing: "Pairing…",
    connecting: "Connecting…",
    connected: "Connected",
    reconnecting: "Reconnecting…",
    incompatible: "Incompatible protocol",
    failed: "Pairing failed",
    closed: "Disconnected",
};
const connectionState = computed(
    () => connectionLabels[remoteConnectionState.value],
);

watch(remoteConnectionState, (nextState) => {
    if (nextState === "connected") error.value = "";
});

const errorMessage = (clientError: CoreClientError) =>
    clientError.type === "core"
        ? clientError.error.message
        : clientError.message;

const remoteClient = inject(coreClientKey);
if (!remoteClient) {
    throw new Error("Remote CoreClient is unavailable");
}

const handleSnapshot = (nextState: PlaybackSnapshotDto) => {
    if (pendingSeek.value !== null && Math.abs(nextState.position - pendingSeek.value) < 2) {
        pendingSeek.value = null;
    }
    state.value = nextState;
    if (nextState.sourceLoadError) {
        error.value = nextState.sourceLoadError;
    } else if (nextState.sourceLoading) {
        error.value = "";
    }
};

function command(command: PlaybackCommandDto) {
    void remoteClient.execute(command).catch((clientError: CoreClientError) => {
        pendingSeek.value = null;
        error.value = errorMessage(clientError);
    });
}

function navigation(action: "previous" | "next") {
    command({ type: action });
}

function seek(position: number) {
    pendingSeek.value = position;
    command({ type: "seekAbsolute", position });
}

function setVolume(event: Event) {
    command({ type: "setVolume", volume: Number((event.target as HTMLInputElement).value) });
}

function setSpeed(event: Event) {
    command({ type: "setSpeed", speed: Number((event.target as HTMLSelectElement).value) });
}

function trackLabel(track: MediaTrackDto) {
    const labels = [track.title || track.lang || `Track ${track.id}`];
    if (track.lang && track.lang !== track.title) labels.push(track.lang);
    if (track.codec) labels.push(track.codec.toUpperCase());
    if (track.isDefault) labels.push("Default");
    if (track.forced) labels.push("Forced");
    return labels.join(" · ");
}

function selectAudioTrack(event: Event) {
    const trackId = Number((event.target as HTMLSelectElement).value);
    if (trackId > 0) command({ type: "selectAudioTrack", trackId });
}

function selectSubtitleTrack(event: Event) {
    const trackId = Number((event.target as HTMLSelectElement).value);
    command(trackId > 0
        ? { type: "selectSubtitleTrack", trackId }
        : { type: "disableSubtitles" });
}

let unsubscribe: (() => void) | null = null;

onMounted(() => {
    unsubscribe = remoteClient.subscribe(handleSnapshot, (clientError) => {
        error.value = errorMessage(clientError);
    });
});

onBeforeUnmount(() => {
    unsubscribe?.();
});
</script>

<template>
    <main class="remote-app">
        <header class="app-header">
            <span class="brand"><span class="brand-mark">S</span>SOIA</span>
            <span
                class="connection"
                :class="{ 'connection--active': canControl }"
            >
                <span class="connection-dot" aria-hidden="true"></span>
                {{ connectionState }}
            </span>
        </header>
        <section class="now-playing">
            <p
                class="playback-status"
                :class="{ 'playback-status--error': statusIsError }"
                :title="statusMessage || undefined"
                aria-live="polite"
            >
                <span v-if="statusMessage">{{ statusMessage }}</span>
            </p>
            <div class="artwork">
                <img :src="soiaIconUrl" alt="Soia" />
            </div>
            <p class="eyebrow">NOW PLAYING</p>
            <h1>{{ state.title || "Nothing playing" }}</h1>
        </section>
        <section class="controls" :aria-disabled="!canControl">
            <div class="seek-group">
                <SeekBar
                    class="remote-seek"
                    :duration="state.duration"
                    :progress-percent="progressPercent"
                    :buffered-percent="progressPercent"
                    :format-time="formatTime"
                    :always-show-scrubber="true"
                    :show-hover-tooltip="false"
                    @seek="seek"
                />
                <div class="time"><span>{{ formatTime(displayedPosition) }}</span><span>{{ durationLabel }}</span></div>
            </div>
            <div class="transport">
                <button :disabled="!canControl" aria-label="Previous item" @click="navigation('previous')">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 18V6h2v12H6zm3.5-6 8.5 6V6l-8.5 6z" /></svg>
                </button>
                <button :disabled="!canControl" aria-label="Back 5 seconds" @click="command({ type: 'seekRelative', seconds: -5 })">
                    <svg viewBox="0 -960 960 960" aria-hidden="true"><path d="M339.5-108.5q-65.5-28.5-114-77t-77-114Q120-365 120-440h80q0 117 81.5 198.5T480-160q117 0 198.5-81.5T760-440q0-117-81.5-198.5T480-720h-6l62 62-56 58-160-160 160-160 56 58-62 62h6q75 0 140.5 28.5t114 77q48.5 48.5 77 114T840-440q0 75-28.5 140.5t-77 114q-48.5 48.5-114 77T480-80q-75 0-140.5-28.5ZM380-320v-60h120v-40H380v-140h180v60H440v40h80q17 0 28.5 11.5T560-420v60q0 17-11.5 28.5T520-320H380Z" /></svg>
                </button>
                <button class="play" :disabled="!canControl" :aria-label="state.isPlaying ? 'Pause' : 'Play'" @click="command({ type: 'setPaused', paused: state.isPlaying })">
                    <svg v-if="state.isPlaying" viewBox="0 0 24 24" aria-hidden="true"><path d="M14,19H18V5H14M6,19H10V5H6V19Z" /></svg>
                    <svg v-else viewBox="0 0 24 24" aria-hidden="true"><path d="M8,5.14V19.14L19,12.14L8,5.14Z" /></svg>
                </button>
                <button :disabled="!canControl" aria-label="Forward 5 seconds" @click="command({ type: 'seekRelative', seconds: 5 })">
                    <svg viewBox="0 -960 960 960" aria-hidden="true"><path d="M339.5-108.5q-65.5-28.5-114-77t-77-114Q120-365 120-440t28.5-140.5q28.5-65.5 77-114t114-77Q405-800 480-800h6l-62-62 56-58 160 160-160 160-56-58 62-62h-6q-117 0-198.5 81.5T200-440q0 117 81.5 198.5T480-160q117 0 198.5-81.5T760-440h80q0 75-28.5 140.5t-77 114q-48.5 48.5-114 77T480-80q-75 0-140.5-28.5ZM380-320v-60h120v-40H380v-140h180v60H440v40h80q17 0 28.5 11.5T560-420v60q0 17-11.5 28.5T520-320H380Z" /></svg>
                </button>
                <button :disabled="!canControl" aria-label="Next item" @click="navigation('next')">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M16 6v12h2V6h-2zm-1.5 6L6 18V6l8.5 6z" /></svg>
                </button>
            </div>
            <div class="volume">
                <button :disabled="!canControl" :aria-label="state.muted ? 'Unmute' : 'Mute'" @click="command({ type: 'setMuted', muted: !state.muted })">
                    <svg v-if="state.muted" viewBox="0 0 24 24" aria-hidden="true"><path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63Zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51A8.8 8.8 0 0 0 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71ZM4.27 3 3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25A8.9 8.9 0 0 1 14 18.7v2.06a10.7 10.7 0 0 0 3.69-1.81L19.73 21 21 19.73 4.27 3ZM12 4 9.91 6.09 12 8.18V4Z" /></svg>
                    <svg v-else viewBox="0 0 24 24" aria-hidden="true"><path d="M3 9v6h4l5 5V4L7 9H3Zm13.5 3A4.5 4.5 0 0 0 14 7.97v8.05A4.5 4.5 0 0 0 16.5 12ZM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77Z" /></svg>
                </button>
                <input type="range" min="0" max="130" :value="state.volume" :disabled="!canControl" @change="setVolume" />
                <output>{{ Math.round(state.volume) }}%</output>
            </div>
            <label class="control-field speed-control">
                <span class="control-label">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4a8 8 0 1 0 8 8 8.01 8.01 0 0 0-8-8Zm0 14a6 6 0 1 1 6-6 6.01 6.01 0 0 1-6 6Zm3.8-8.6-4.36 2.18A1 1 0 0 0 11 12.9V15h2v-1.48l3.7-1.85-.9-1.79V9.4Z" /></svg>
                    <span>Speed</span>
                </span>
                <select :value="state.speed" :disabled="!canControl" @change="setSpeed">
                    <option v-for="rate in playbackRates" :key="rate" :value="rate">{{ rate }}×</option>
                </select>
            </label>
            <div class="track-controls">
                <label class="control-field">
                    <span class="control-label">
                        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3h-6Z" /></svg>
                        <span>Audio</span>
                    </span>
                    <select
                        :value="selectedAudioTrackId"
                        :disabled="!canControl || !audioTracks.length"
                        @change="selectAudioTrack"
                    >
                        <option v-if="!audioTracks.length" value="">Unavailable</option>
                        <option v-for="track in audioTracks" :key="track.id" :value="track.id">
                            {{ trackLabel(track) }}
                        </option>
                    </select>
                </label>
                <label class="control-field">
                    <span class="control-label">
                        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2ZM11 11H9.5v-1h-3v4h3v-1H11v1a1.5 1.5 0 0 1-1.5 1.5h-3A1.5 1.5 0 0 1 5 14v-4a1.5 1.5 0 0 1 1.5-1.5h3A1.5 1.5 0 0 1 11 10v1Zm8 0h-1.5v-1h-3v4h3v-1H19v1a1.5 1.5 0 0 1-1.5 1.5h-3A1.5 1.5 0 0 1 13 14v-4a1.5 1.5 0 0 1 1.5-1.5h3A1.5 1.5 0 0 1 19 10v1Z" /></svg>
                        <span>Subtitles</span>
                    </span>
                    <select
                        :value="selectedSubtitleTrackId"
                        :disabled="!canControl || !subtitleTracks.length"
                        @change="selectSubtitleTrack"
                    >
                        <option :value="0">Off</option>
                        <option v-for="track in subtitleTracks" :key="track.id" :value="track.id">
                            {{ trackLabel(track) }}
                        </option>
                    </select>
                </label>
            </div>
        </section>
    </main>
</template>
