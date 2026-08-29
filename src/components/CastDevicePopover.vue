<script setup lang="ts">
import { computed, ref } from "vue";
import { useCasting } from "../composables/useCasting";
import { tauriCastingClient } from "../core-client/tauriCastingClient";
import type { CastDeviceDto } from "../core-client/generated/CastDeviceDto";

const props = defineProps<{
    isFileLoaded: boolean;
}>();

const isOpen = ref(false);
const casting = useCasting(tauriCastingClient);
const hasError = computed(() => Boolean(casting.error.value || casting.snapshot.value.lastError));
const errorMessage = computed(
    () => casting.error.value || casting.snapshot.value.lastError?.message || "",
);
const activeCapabilityHint = computed(() => {
    const capabilities = casting.snapshot.value.device?.capabilities;
    if (!capabilities) return "";
    const unavailable: string[] = [];
    if (!capabilities.play || !capabilities.pause) unavailable.push("play/pause");
    if (!capabilities.seek) unavailable.push("seeking");
    if (!capabilities.volume) unavailable.push("volume");
    const receiverControls = unavailable.length
        ? `Unavailable on this receiver: ${unavailable.join(", ")}.`
        : "";
    return `${receiverControls}${receiverControls ? " " : ""}Speed, track selection, video effects and PiP are unavailable while casting.`;
});
const phaseLabel = computed(() => {
    const labels: Record<string, string> = {
        connecting: "Connecting",
        loading: "Loading media",
        playing: "Casting",
        paused: "Paused",
        buffering: "Buffering",
        stopped: "Stopped",
        disconnected: "Disconnected",
        error: "Needs attention",
    };
    return labels[casting.snapshot.value.phase] ?? "";
});

const openAndDiscover = () => {
    isOpen.value = !isOpen.value;
    if (isOpen.value && !casting.devices.value.length) {
        void casting.discover();
    }
};

const deviceCapabilityHint = (device: CastDeviceDto) => {
    const unavailable: string[] = [];
    if (!device.capabilities.play || !device.capabilities.pause) unavailable.push("playback");
    if (!device.capabilities.seek) unavailable.push("seek");
    if (!device.capabilities.volume) unavailable.push("volume");
    return unavailable.length ? `No ${unavailable.join(" or ")}` : "Playback controls ready";
};
</script>

<template>
    <div class="cast-popover" data-window-no-drag>
        <button
            class="icon-button top-bar__action cast-popover__trigger"
            :class="{ 'cast-popover__trigger--active': casting.isActive.value }"
            type="button"
            title="Cast"
            aria-label="Cast"
            :aria-expanded="isOpen"
            @click.stop="openAndDiscover"
        >
            <svg viewBox="0 -960 960 960" aria-hidden="true">
                <path d="M480-480Zm320 320H600q0-20-1.5-40t-4.5-40h206v-480H160v46q-20-3-40-4.5T80-680v-40q0-33 23.5-56.5T160-800h640q33 0 56.5 23.5T880-720v480q0 33-23.5 56.5T800-160Zm-720 0v-120q50 0 85 35t35 85H80Zm200 0q0-83-58.5-141.5T80-360v-80q117 0 198.5 81.5T360-160h-80Zm160 0q0-75-28.5-140.5t-77-114q-48.5-48.5-114-77T80-520v-80q91 0 171 34.5T391-471q60 60 94.5 140T520-160h-80Z" />
            </svg>
        </button>

        <section v-if="isOpen" class="cast-popover__panel" @click.stop>
            <header class="cast-popover__header">
                <div>
                    <strong>Cast</strong>
                    <p v-if="casting.isActive.value">{{ casting.activeDeviceName.value }} · {{ phaseLabel }}</p>
                    <p v-else>Choose a receiver on your network</p>
                </div>
                <button
                    class="cast-popover__scan"
                    type="button"
                    :disabled="casting.isDiscovering.value"
                    @click="casting.discover"
                >
                    {{ casting.isDiscovering.value ? "Scanning…" : "Scan" }}
                </button>
            </header>

            <div v-if="hasError" class="cast-popover__error">
                {{ errorMessage }}
                <button type="button" @click="casting.discover">Retry</button>
            </div>

            <div v-if="casting.isActive.value" class="cast-popover__active">
                <span>{{ casting.snapshot.value.mediaTitle || "Current media" }}</span>
                <button type="button" @click="casting.disconnect">Stop casting</button>
            </div>
            <p v-if="casting.isActive.value && activeCapabilityHint" class="cast-popover__capability-hint">
                {{ activeCapabilityHint }}
            </p>

            <div v-if="casting.isDiscovering.value && !casting.devices.value.length" class="cast-popover__empty">
                Looking for receivers…
            </div>
            <div v-else-if="!casting.devices.value.length" class="cast-popover__empty">
                No receivers found. Check that the device is awake and on the same network.
            </div>
            <ul v-else class="cast-popover__devices">
                <li v-for="device in casting.devices.value" :key="`${device.protocol}:${device.id}`">
                    <button
                        type="button"
                        :disabled="!props.isFileLoaded || casting.isConnecting.value"
                        @click="casting.connect(device.id)"
                    >
                        <span class="cast-popover__device-name">{{ device.name }}</span>
                        <span class="cast-popover__device-meta">
                            <span class="cast-popover__device-protocol">{{ device.protocol === "dlna" ? "DLNA" : "Chromecast" }}</span>
                            <span class="cast-popover__device-capabilities">{{ deviceCapabilityHint(device) }}</span>
                        </span>
                    </button>
                </li>
            </ul>
            <p v-if="!props.isFileLoaded" class="cast-popover__hint">Open media before connecting.</p>
        </section>
    </div>
</template>

<style scoped>
.cast-popover { position: relative; z-index: 120; flex: none; }
.cast-popover__trigger { display: grid; width: var(--top-bar-action-size, 30px); height: var(--top-bar-action-size, 30px); place-items: center; border-radius: 8px; padding: var(--top-bar-action-padding, 4px); margin-top: var(--top-bar-action-margin-top, 0px); color: white; position: relative; transition: color .2s, transform .1s; }
.cast-popover__trigger:hover { color: #ccc; transform: scale(1.1); }
.cast-popover__trigger:active { transform: scale(.95); }
.cast-popover__trigger--active { color: #86d6ff; }
.cast-popover__trigger svg { width: 100%; height: 100%; fill: currentColor; }
.cast-popover__panel { position: absolute; top: calc(var(--top-bar-action-size, 30px) + 6px); right: 0; width: min(330px, calc(100vw - 24px)); overflow: hidden; border: 1px solid rgba(255,255,255,.14); border-radius: 12px; color: rgba(255,255,255,.9); background: rgba(26,29,35,.96); box-shadow: 0 16px 42px rgba(0,0,0,.35); backdrop-filter: blur(16px); }
.cast-popover__header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 13px 14px 11px; border-bottom: 1px solid rgba(255,255,255,.1); }
.cast-popover__header strong { font-size: 13px; font-weight: 650; }
.cast-popover__header p, .cast-popover__hint { margin: 3px 0 0; color: rgba(255,255,255,.56); font-size: 11px; }
.cast-popover__scan, .cast-popover__active button, .cast-popover__error button { border: 0; border-radius: 6px; padding: 5px 8px; color: inherit; background: rgba(255,255,255,.11); font: inherit; font-size: 11px; cursor: pointer; }
.cast-popover__scan:disabled { cursor: wait; opacity: .55; }
.cast-popover__devices { max-height: 250px; margin: 0; padding: 5px; overflow: auto; list-style: none; }
.cast-popover__devices button { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 12px; border: 0; border-radius: 8px; padding: 10px; color: inherit; background: transparent; text-align: left; cursor: pointer; }
.cast-popover__devices button:hover:not(:disabled) { background: rgba(255,255,255,.09); }
.cast-popover__devices button:disabled { cursor: not-allowed; opacity: .45; }
.cast-popover__device-name { font-size: 12px; font-weight: 560; }
.cast-popover__device-meta { display: inline-flex; flex-direction: column; align-items: flex-end; gap: 2px; }
.cast-popover__device-protocol { color: rgba(255,255,255,.5); font-size: 10px; }
.cast-popover__device-capabilities { color: rgba(255,255,255,.42); font-size: 9px; }
.cast-popover__empty { padding: 24px 18px; color: rgba(255,255,255,.58); font-size: 12px; line-height: 1.45; text-align: center; }
.cast-popover__active, .cast-popover__error { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 9px 12px; border-bottom: 1px solid rgba(255,255,255,.08); font-size: 11px; }
.cast-popover__active span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cast-popover__error { color: #ffb4ae; background: rgba(190,58,48,.14); }
.cast-popover__error button { color: inherit; }
.cast-popover__hint { padding: 0 12px 11px; }
.cast-popover__capability-hint { margin: 0; padding: 8px 12px; border-bottom: 1px solid rgba(255,255,255,.08); color: rgba(255,220,150,.78); font-size: 10px; line-height: 1.4; }

:global(:root[data-theme="light"]) .cast-popover__trigger { color: rgba(28,38,52,.9); }
:global(:root[data-theme="light"]) .cast-popover__trigger:hover { color: rgba(18,28,40,1); }
:global(:root[data-theme="light"]) .cast-popover__trigger--active { color: #2f65c9; }
:global(:root[data-theme="light"]) .cast-popover__panel { border-color: rgba(30,46,63,.14); color: #263545; background: rgba(250,252,255,.97); box-shadow: 0 16px 42px rgba(26,42,58,.18); }
:global(:root[data-theme="light"]) .cast-popover__header, :global(:root[data-theme="light"]) .cast-popover__active { border-color: rgba(30,46,63,.1); }
:global(:root[data-theme="light"]) .cast-popover__header p, :global(:root[data-theme="light"]) .cast-popover__hint, :global(:root[data-theme="light"]) .cast-popover__empty, :global(:root[data-theme="light"]) .cast-popover__device-protocol { color: rgba(38,53,69,.58); }
:global(:root[data-theme="light"]) .cast-popover__device-capabilities { color: rgba(38,53,69,.48); }
:global(:root[data-theme="light"]) .cast-popover__capability-hint { border-color: rgba(30,46,63,.1); color: rgba(139,92,20,.9); }
</style>
