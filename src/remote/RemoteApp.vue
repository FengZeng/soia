<script setup lang="ts">
import { computed, ref } from "vue";
import { remoteConnectionState } from "./remoteCoreClient";
import RemotePlaybackPanel from "./RemotePlaybackPanel.vue";
import RemotePlaylistPanel from "./RemotePlaylistPanel.vue";
import RemoteNetworkPanel from "./RemoteNetworkPanel.vue";
import soiaIconUrl from "../../src-tauri/icons/128x128@2x.png";

const activeView = ref<"playback" | "playlists" | "network">("playback");
const canControl = computed(() => remoteConnectionState.value === "connected");
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
const connectionState = computed(() => connectionLabels[remoteConnectionState.value]);
</script>

<template>
    <main class="remote-app">
        <header class="app-header">
            <span class="brand"><span class="brand-mark"><img :src="soiaIconUrl" alt="" /></span>SOIA</span>
            <span class="connection" :class="{ 'connection--active': canControl }">
                <span class="connection-dot" aria-hidden="true"></span>
                {{ connectionState }}
            </span>
        </header>
        <nav class="remote-view-tabs" aria-label="Remote sections">
            <button
                class="remote-view-tabs__button"
                :class="{ 'remote-view-tabs__button--active': activeView === 'playback' }"
                :aria-pressed="activeView === 'playback'"
                @click="activeView = 'playback'"
            >
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14l11-7L8 5Zm-4 0h2v14H4V5Z" /></svg>
                <span>Playback</span>
            </button>
            <button class="remote-view-tabs__button" :class="{ 'remote-view-tabs__button--active': activeView === 'network' }" @click="activeView = 'network'">Network</button>
            <button
                class="remote-view-tabs__button"
                :class="{ 'remote-view-tabs__button--active': activeView === 'playlists' }"
                :aria-pressed="activeView === 'playlists'"
                @click="activeView = 'playlists'"
            >
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h16v2H4V6Zm0 5h16v2H4v-2Zm0 5h10v2H4v-2Z" /></svg>
                <span>Playlists</span>
            </button>
        </nav>
        <RemotePlaybackPanel v-show="activeView === 'playback'" />
        <RemotePlaylistPanel :active="activeView === 'playlists'" v-show="activeView === 'playlists'" />
        <RemoteNetworkPanel v-show="activeView === 'network'" />
    </main>
</template>
