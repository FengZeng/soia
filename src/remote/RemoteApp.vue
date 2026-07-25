<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from "vue";
import SeekBar from "../components/player-controls/SeekBar.vue";
import type { CoreClientError } from "../core-client/CoreClient";
import { coreClientKey } from "../core-client/coreClientKey";
import type { PlaybackSnapshotDto } from "../core-client/generated/PlaybackSnapshotDto";
import type { PlaybackCommandDto } from "../core-client/generated/PlaybackCommandDto";
import type { MediaTrackDto } from "../core-client/generated/MediaTrackDto";
import { remoteConnectionState } from "./remoteCoreClient";

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
        <header><span class="brand">SOIA</span><span class="connection">{{ connectionState }}</span></header>
        <section class="now-playing">
            <div class="artwork">▶</div>
            <p class="eyebrow">NOW PLAYING</p>
            <h1>{{ state.title || "Nothing playing" }}</h1>
        </section>
        <section class="controls" :aria-disabled="!canControl">
            <SeekBar
                class="remote-seek"
                :duration="state.duration"
                :progress-percent="progressPercent"
                :buffered-percent="progressPercent"
                :format-time="formatTime"
                :show-hover-tooltip="false"
                @seek="seek"
            />
            <div class="time"><span>{{ formatTime(displayedPosition) }}</span><span>{{ durationLabel }}</span></div>
            <p v-if="pendingSeek !== null" class="seeking">Seeking…</p>
            <p v-else-if="state.sourceLoading" class="seeking">Preparing source…</p>
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
                <button :disabled="!canControl" :aria-label="state.muted ? 'Unmute' : 'Mute'" @click="command({ type: 'setMuted', muted: !state.muted })">{{ state.muted ? "🔇" : "🔊" }}</button>
                <input type="range" min="0" max="130" :value="state.volume" :disabled="!canControl" @change="setVolume" />
                <span>{{ Math.round(state.volume) }}%</span>
            </div>
            <label class="speed-control">
                <span>Playback speed</span>
                <select :value="state.speed" :disabled="!canControl" @change="setSpeed">
                    <option v-for="rate in playbackRates" :key="rate" :value="rate">{{ rate }}×</option>
                </select>
            </label>
            <div class="track-controls">
                <label>
                    <span>Audio</span>
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
                <label>
                    <span>Subtitles</span>
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
        <p v-if="error" class="error">{{ error }}</p>
    </main>
</template>
