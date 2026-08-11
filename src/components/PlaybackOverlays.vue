<script setup lang="ts">
import { computed } from "vue";

type StatusOverlayMode = "play" | "pause";

const props = defineProps<{
    isLoading: boolean;
    loadingSpeedBps?: number | null;
    showStatusOverlay: boolean;
    statusOverlayMode: StatusOverlayMode;
    seekOverlayLeftText?: string;
    seekOverlayRightText?: string;
    seekOverlayLeftTimelineText?: string;
    volumeOverlayText?: string;
    zoomOverlayText?: { label: string; value: string } | null;
    speedOverlayText?: { label: string; value: string } | null;
    surroundOverlayText?: { label: string; value: string } | null;
    hideSeekTimeline?: boolean;
    seekOverlayLeftPulseToken?: number;
    seekOverlayRightPulseToken?: number;
}>();

const loadingSpeedText = computed(() => {
    const speed = props.loadingSpeedBps ?? 0;
    if (!Number.isFinite(speed) || speed <= 0) return "";
    if (speed >= 1024 * 1024) {
        return `${(speed / (1024 * 1024)).toFixed(2)} MB/s`;
    }
    if (speed >= 1024) {
        return `${(speed / 1024).toFixed(1)} KB/s`;
    }
    return `${Math.round(speed)} B/s`;
});

const parseSeekOverlayText = (text?: string) => {
    if (!text) return null;
    const trimmed = text.trim();
    if (!trimmed) return null;
    const sign = trimmed.startsWith("-") ? "-" : "+";
    const value = trimmed.replace(/^[-+]\s*/, "");
    if (!value) return null;
    return { sign, value };
};

const seekOverlayLeftDisplay = computed(() =>
    parseSeekOverlayText(props.seekOverlayLeftText),
);
const seekOverlayRightDisplay = computed(() =>
    parseSeekOverlayText(props.seekOverlayRightText),
);
</script>

<template>
    <div v-if="isLoading" class="loading-overlay ui-surface">
        <div class="loading-card ui-surface">
            <div class="loading-spinner"></div>
            <div class="loading-text">Loading...</div>
            <div v-if="loadingSpeedText" class="loading-speed">
                {{ loadingSpeedText }}
            </div>
        </div>
    </div>

    <transition name="fade-in">
        <div v-if="showStatusOverlay" class="status-overlay">
            <div
                class="status-overlay__button"
                :title="statusOverlayMode === 'pause' ? 'Paused' : 'Playing'"
                :aria-label="
                    statusOverlayMode === 'pause' ? 'Paused' : 'Playing'
                "
                role="img"
            >
                <svg
                    v-if="statusOverlayMode === 'play'"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                >
                    <path d="M8 5v14l11-7z" />
                </svg>
                <svg v-else viewBox="0 0 24 24" fill="currentColor">
                    <path d="M6 5h4v14H6V5zm8 0h4v14h-4V5z" />
                </svg>
            </div>
        </div>
    </transition>

    <div class="seek-overlay" aria-hidden="true">
        <transition name="fade-in">
            <div v-if="volumeOverlayText" class="volume-overlay__text">
                {{ volumeOverlayText }}
            </div>
        </transition>
        <transition name="fade-in">
            <div v-if="zoomOverlayText" class="scale-overlay__badge">
                <span class="scale-overlay__label">{{ zoomOverlayText.label }}</span>
                <span class="scale-overlay__value">{{ zoomOverlayText.value }}</span>
            </div>
        </transition>
        <transition name="fade-in">
            <div v-if="speedOverlayText" class="scale-overlay__badge">
                <span class="scale-overlay__label">{{ speedOverlayText.label }}</span>
                <span class="scale-overlay__value">{{ speedOverlayText.value }}</span>
            </div>
        </transition>
        <transition name="fade-in">
            <div v-if="surroundOverlayText" class="scale-overlay__badge">
                <span class="scale-overlay__label">{{ surroundOverlayText.label }}</span>
                <span class="scale-overlay__value">{{ surroundOverlayText.value }}</span>
            </div>
        </transition>
        <transition name="fade-in">
            <div
                v-if="seekOverlayLeftDisplay"
                :key="`left-${seekOverlayLeftPulseToken ?? 0}`"
                class="seek-overlay__hint is-left"
            >
                <span
                    class="seek-overlay__sign"
                    :class="
                        seekOverlayLeftDisplay.sign === '+'
                            ? 'is-plus'
                            : 'is-minus'
                    "
                    aria-hidden="true"
                ></span>
                <span>{{ seekOverlayLeftDisplay.value }}</span>
            </div>
        </transition>
        <transition name="fade-in">
            <div
                v-if="seekOverlayRightDisplay"
                :key="`right-${seekOverlayRightPulseToken ?? 0}`"
                class="seek-overlay__hint is-right"
            >
                <span
                    class="seek-overlay__sign"
                    :class="
                        seekOverlayRightDisplay.sign === '+'
                            ? 'is-plus'
                            : 'is-minus'
                    "
                    aria-hidden="true"
                ></span>
                <span>{{ seekOverlayRightDisplay.value }}</span>
            </div>
        </transition>

        <transition name="fade-in">
            <div
                v-if="seekOverlayLeftTimelineText && !hideSeekTimeline"
                class="seek-overlay__timeline is-left"
            >
                {{ seekOverlayLeftTimelineText }}
            </div>
        </transition>
    </div>
</template>

<style scoped>
.loading-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 140;
    pointer-events: none;
    background: transparent;
    border-color: transparent;
    box-shadow: none;
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
}

.status-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 130;
    pointer-events: none;
}

.status-overlay__button {
    width: 128px;
    height: 128px;
    border-radius: 999px;
    border: none;
    background: radial-gradient(
        circle,
        rgba(0, 0, 0, 0.55) 0%,
        rgba(0, 0, 0, 0.4) 60%,
        rgba(0, 0, 0, 0.28) 100%
    );
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 18px 36px rgba(0, 0, 0, 0.4);
    overflow: hidden;
    pointer-events: none;
    transition:
        transform 0.2s ease,
        background-color 0.2s ease,
        box-shadow 0.2s ease;
}

.status-overlay__button svg {
    width: 82px;
    height: 82px;
    display: block;
}

.seek-overlay {
    position: fixed;
    inset: 0;
    z-index: 132;
    pointer-events: none;
}

.seek-overlay__hint {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    width: 5ch;
    display: inline-flex;
    align-items: center;
    gap: 0.55ch;
    padding: 0;
    color: rgba(255, 255, 255, 0.96);
    font-size: 28px;
    font-family:
        "SF Pro Display",
        "SF Pro Text",
        "Segoe UI",
        "PingFang SC",
        "Hiragino Sans GB",
        Roboto,
        Arial,
        sans-serif;
    font-weight: 500;
    line-height: 1;
    text-align: left;
    white-space: pre;
    font-variant-numeric: tabular-nums;
}

.seek-overlay__sign {
    position: relative;
    width: 0.52em;
    height: 0.52em;
    flex: 0 0 auto;
}

.seek-overlay__sign::before,
.seek-overlay__sign::after {
    content: "";
    position: absolute;
    left: 50%;
    top: 50%;
    background: currentColor;
    transform: translate(-50%, -50%);
    border-radius: 999px;
}

.seek-overlay__sign::before {
    width: 100%;
    height: 0.1em;
}

.seek-overlay__sign.is-minus::before {
    width: 93%;
}

.seek-overlay__sign.is-plus::after {
    width: 0.1em;
    height: 100%;
}

.seek-overlay__sign.is-minus::after {
    display: none;
}

.seek-overlay__hint.is-left {
    left: clamp(20px, 8vw, 140px);
}

.seek-overlay__hint.is-right {
    right: clamp(20px, 8vw, 140px);
}

.seek-overlay__timeline {
    position: absolute;
    bottom: clamp(10px, 2.2vh, 24px);
    font-size: 14px;
    font-family:
        "SF Pro Text",
        "Segoe UI",
        "PingFang SC",
        "Hiragino Sans GB",
        Roboto,
        Arial,
        sans-serif;
    font-weight: 500;
    line-height: 1.1;
    letter-spacing: 0.01em;
    color: rgba(255, 255, 255, 0.9);
    font-variant-numeric: tabular-nums;
}

.seek-overlay__timeline.is-left {
    left: clamp(8px, 1.8vw, 20px);
}

.volume-overlay__text {
    position: absolute;
    top: clamp(42px, 5.5vh, 66px);
    left: clamp(16px, 2vw, 28px);
    padding: 5px 9px;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.38);
    color: rgba(255, 255, 255, 0.94);
    font-size: 16px;
    font-weight: 500;
    line-height: 1.1;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(8px);
}

.scale-overlay__badge {
    position: absolute;
    top: clamp(42px, 5.5vh, 66px);
    right: clamp(16px, 2vw, 28px);
    padding: 6px 12px 7px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.38);
    color: rgba(255, 255, 255, 0.94);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    backdrop-filter: blur(8px);
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.45);
}

.scale-overlay__label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    opacity: 0.72;
    line-height: 1;
}

.scale-overlay__value {
    font-size: 18px;
    font-weight: 600;
    line-height: 1.1;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    letter-spacing: 0.02em;
}

.fade-in-enter-active,
.fade-in-leave-active {
    transition: opacity 0.2s ease;
}

.fade-in-enter-from,
.fade-in-leave-to {
    opacity: 0;
}

.loading-card {
    width: 120px;
    height: 120px;
    padding: 0;
    border-radius: 14px;
    color: #f6f6f6;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    border: 1px solid var(--glass-border);
}

.loading-spinner {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.25);
    border-top-color: rgba(255, 255, 255, 0.9);
    animation: loading-spin 0.9s linear infinite;
}

.loading-text {
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
}

.loading-speed {
    font-size: 11px;
    letter-spacing: 0.02em;
    color: rgba(255, 255, 255, 0.78);
}

:root[data-theme="light"] .loading-card {
    color: rgba(28, 38, 52, 0.9);
    border: 1px solid rgba(0, 0, 0, 0.12);
}

:root[data-theme="light"] .seek-overlay__hint {
    color: rgba(27, 39, 54, 0.92);
}

:root[data-theme="light"] .seek-overlay__timeline {
    color: rgba(27, 39, 54, 0.86);
}

:root[data-theme="light"] .volume-overlay__text {
    color: rgba(27, 39, 54, 0.9);
    background: rgba(255, 255, 255, 0.72);
    text-shadow: none;
}

:root[data-theme="light"] .scale-overlay__badge {
    color: rgba(27, 39, 54, 0.9);
    background: rgba(255, 255, 255, 0.72);
    text-shadow: none;
}

:root[data-theme="light"] .loading-spinner {
    border-color: rgba(0, 0, 0, 0.2);
    border-top-color: rgba(57, 108, 216, 0.78);
}

:root[data-theme="light"] .loading-text {
    color: rgba(31, 43, 59, 0.72);
}

:root[data-theme="light"] .loading-speed {
    color: rgba(31, 43, 59, 0.66);
}

:root[data-theme="graphite"] .loading-card {
    color: #edf1f6;
    border: 1px solid rgba(146, 158, 175, 0.32);
}

:root[data-theme="graphite"] .seek-overlay__hint {
    color: rgba(235, 241, 249, 0.95);
}

:root[data-theme="graphite"] .seek-overlay__timeline {
    color: rgba(235, 241, 249, 0.9);
}

:root[data-theme="graphite"] .volume-overlay__text {
    color: rgba(235, 241, 249, 0.94);
    background: rgba(25, 28, 32, 0.48);
}

:root[data-theme="graphite"] .scale-overlay__badge {
    color: rgba(235, 241, 249, 0.94);
    background: rgba(25, 28, 32, 0.48);
}

:root[data-theme="graphite"] .loading-spinner {
    border-color: rgba(188, 196, 208, 0.24);
    border-top-color: rgba(232, 238, 247, 0.92);
}

:root[data-theme="graphite"] .loading-text {
    color: rgba(221, 227, 236, 0.78);
}

:root[data-theme="graphite"] .loading-speed {
    color: rgba(221, 227, 236, 0.72);
}

@keyframes loading-spin {
    to {
        transform: rotate(360deg);
    }
}
</style>
