<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import SeekBar from "../components/player-controls/SeekBar.vue";

type RemoteState = {
    title: string | null;
    duration: number;
    position: number;
    isPlaying: boolean;
    volume: number;
    muted: boolean;
    playlistPosition: number;
    playlistCount: number;
};

const state = ref<RemoteState>({ title: null, duration: 0, position: 0, isPlaying: false, volume: 100, muted: false, playlistPosition: -1, playlistCount: 0 });
const connectionState = ref("Connecting…");
const error = ref("");
let socket: WebSocket | null = null;
let reconnectTimer: number | null = null;
let commandSequence = 0;
const pendingSeek = ref<number | null>(null);

let pairCode = new URLSearchParams(window.location.hash.slice(1)).get("pair");
const canControl = computed(() => connectionState.value === "Connected");
const durationLabel = computed(() => formatTime(state.value.duration));
const displayedPosition = computed(() => pendingSeek.value ?? state.value.position);
const progressPercent = computed(() => state.value.duration > 0
    ? displayedPosition.value / state.value.duration * 100
    : 0);

function formatTime(seconds: number) {
    const value = Math.max(0, Math.floor(seconds || 0));
    const minutes = Math.floor(value / 60);
    return `${minutes}:${String(value % 60).padStart(2, "0")}`;
}

async function connect() {
    if (pairCode) {
        try {
            const response = await fetch("/api/pair", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ pairCode }),
            });
            if (!response.ok) throw new Error("Pairing code expired");
            window.history.replaceState(null, "", window.location.pathname);
            pairCode = null;
        } catch (pairError) {
            connectionState.value = "Pairing failed";
            error.value = pairError instanceof Error ? pairError.message : "Could not pair with Soia.";
            return;
        }
    }
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    socket = new WebSocket(`${protocol}//${window.location.host}/ws`);
    socket.onopen = () => { connectionState.value = "Connected"; error.value = ""; };
    socket.onmessage = (event) => {
        const message = JSON.parse(event.data) as { type: string; state?: RemoteState; error?: string };
        if (message.type === "state" && message.state) {
            const nextState = message.state;
            if (pendingSeek.value !== null && Math.abs(nextState.position - pendingSeek.value) < 2) {
                pendingSeek.value = null;
            }
            state.value = nextState;
        }
        if (message.type === "error") {
            pendingSeek.value = null;
            error.value = message.error ?? "The command failed.";
        }
    };
    socket.onclose = () => {
        connectionState.value = "Reconnecting…";
        reconnectTimer = window.setTimeout(connect, 1500);
    };
    socket.onerror = () => { error.value = "Could not connect to Soia."; };
}

function command(action: string, value?: number) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    commandSequence += 1;
    socket.send(JSON.stringify({ type: "command", id: String(commandSequence), action, value }));
}

function seek(position: number) {
    pendingSeek.value = position;
    command("seek", position);
}

function setVolume(event: Event) {
    command("setVolume", Number((event.target as HTMLInputElement).value));
}

onMounted(connect);
onBeforeUnmount(() => { socket?.close(); if (reconnectTimer) window.clearTimeout(reconnectTimer); });
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
            <div class="transport">
                <button :disabled="!canControl" aria-label="Previous item" @click="command('previous')">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 18V6h2v12H6zm3.5-6 8.5 6V6l-8.5 6z" /></svg>
                </button>
                <button :disabled="!canControl" aria-label="Back 5 seconds" @click="command('seekRelative', -5)">
                    <svg viewBox="0 -960 960 960" aria-hidden="true"><path d="M339.5-108.5q-65.5-28.5-114-77t-77-114Q120-365 120-440h80q0 117 81.5 198.5T480-160q117 0 198.5-81.5T760-440q0-117-81.5-198.5T480-720h-6l62 62-56 58-160-160 160-160 56 58-62 62h6q75 0 140.5 28.5t114 77q48.5 48.5 77 114T840-440q0 75-28.5 140.5t-77 114q-48.5 48.5-114 77T480-80q-75 0-140.5-28.5ZM380-320v-60h120v-40H380v-140h180v60H440v40h80q17 0 28.5 11.5T560-420v60q0 17-11.5 28.5T520-320H380Z" /></svg>
                </button>
                <button class="play" :disabled="!canControl" :aria-label="state.isPlaying ? 'Pause' : 'Play'" @click="command('togglePause')">
                    <svg v-if="state.isPlaying" viewBox="0 0 24 24" aria-hidden="true"><path d="M14,19H18V5H14M6,19H10V5H6V19Z" /></svg>
                    <svg v-else viewBox="0 0 24 24" aria-hidden="true"><path d="M8,5.14V19.14L19,12.14L8,5.14Z" /></svg>
                </button>
                <button :disabled="!canControl" aria-label="Forward 5 seconds" @click="command('seekRelative', 5)">
                    <svg viewBox="0 -960 960 960" aria-hidden="true"><path d="M339.5-108.5q-65.5-28.5-114-77t-77-114Q120-365 120-440t28.5-140.5q28.5-65.5 77-114t114-77Q405-800 480-800h6l-62-62 56-58 160 160-160 160-56-58 62-62h-6q-117 0-198.5 81.5T200-440q0 117 81.5 198.5T480-160q117 0 198.5-81.5T760-440h80q0 75-28.5 140.5t-77 114q-48.5 48.5-114 77T480-80q-75 0-140.5-28.5ZM380-320v-60h120v-40H380v-140h180v60H440v40h80q17 0 28.5 11.5T560-420v60q0 17-11.5 28.5T520-320H380Z" /></svg>
                </button>
                <button :disabled="!canControl" aria-label="Next item" @click="command('next')">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M16 6v12h2V6h-2zm-1.5 6L6 18V6l8.5 6z" /></svg>
                </button>
            </div>
            <div class="volume">
                <button :disabled="!canControl" :aria-label="state.muted ? 'Unmute' : 'Mute'" @click="command('toggleMute')">{{ state.muted ? "🔇" : "🔊" }}</button>
                <input type="range" min="0" max="130" :value="state.volume" :disabled="!canControl" @change="setVolume" />
                <span>{{ Math.round(state.volume) }}%</span>
            </div>
        </section>
        <p v-if="error" class="error">{{ error }}</p>
    </main>
</template>
