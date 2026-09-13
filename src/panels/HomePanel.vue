<script setup lang="ts">
import { settingsLocale, translateSettingsText } from "../i18n";

const isAndroidPlatform =
    typeof navigator !== "undefined" && /\bandroid\b/i.test(navigator.userAgent);

const props = defineProps<{
    isFileLoaded: boolean;
    hover: boolean;
}>();

const emit = defineEmits<{
    (e: "open-file-picker"): void;
    (e: "update:hover", value: boolean): void;
}>();

const tr = (text: string): string =>
    translateSettingsText(settingsLocale.value, text);
</script>

<template>
    <div
        class="home-panel panel"
        @mouseenter="emit('update:hover', true)"
        @mouseleave="emit('update:hover', false)"
    >
        <div class="home-panel__body">
            <button
                type="button"
                class="home-panel__dropzone"
                :class="{
                    'home-panel__dropzone--hidden':
                        props.isFileLoaded && !props.hover,
                }"
                @click="emit('open-file-picker')"
            >

                <h2 class="home-panel__headline">
                    {{
                        isAndroidPlatform
                            ? tr("Tap to choose videos to play")
                            : tr("Drag & drop videos to play, or click to browse files")
                    }}
                </h2>
                <p class="home-panel__subtext">
                    {{ tr("Select one or multiple videos from your device.") }}
                </p>
                <p class="home-panel__subtext home-panel__subtext--muted">
                    {{ tr("Selecting multiple files creates a Playlist automatically.") }}
                    {{
                        isAndroidPlatform
                            ? tr("Use the Playlist button in the top-left corner to view or edit it.")
                            : tr("Move your cursor to the right side of the window and click to view or edit it.")
                    }}
                </p>

                <span class="home-panel__cta">{{ tr("Choose Files") }}</span>
            </button>
        </div>
    </div>
</template>

<style scoped>
.home-panel {
    pointer-events: auto;
}

.home-panel__body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-rows: minmax(0, 2fr) auto minmax(0, 3fr);
    box-sizing: border-box;
}

.home-panel__dropzone {
    width: 60%;
    grid-row: 2;
    margin: 0 auto;
    flex: 0 0 auto;
    min-height: 270px;
    max-height: min(52vh, 420px);
    border: 2px dashed color-mix(in srgb, var(--text-color) 36%, transparent);
    border-radius: 14px;
    background:
        radial-gradient(
            120% 110% at 100% 0%,
            color-mix(in srgb, var(--text-color) 10%, transparent),
            transparent 60%
        ),
        color-mix(in srgb, var(--base-color) 82%, transparent);
    color: color-mix(in srgb, var(--text-color) 84%, transparent);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    justify-content: flex-start;
    align-items: flex-start;
    text-align: left;
    gap: 0.72rem;
    padding: clamp(0.9rem, 1.4vw, 1.2rem) clamp(1.2rem, 2.2vw, 1.9rem);
    opacity: 1;
    transition:
        background 0.24s ease,
        border-color 0.24s ease,
        color 0.24s ease,
        box-shadow 0.24s ease,
        transform 0.24s ease,
        opacity 0.24s ease;
}

.home-panel__dropzone--hidden {
    opacity: 0;
    pointer-events: none;
    border-color: transparent;
}

.home-panel__dropzone:not(.home-panel__dropzone--hidden):hover {
    transform: translateY(-2px);
    border-color: color-mix(in srgb, var(--text-color) 66%, transparent);
    box-shadow: 0 20px 32px rgba(0, 0, 0, 0.24);
}

.home-panel__headline {
    margin: 0.38rem 0 0;
    max-width: 28ch;
    font-size: clamp(1.18rem, 1.75vw, 1.5rem);
    line-height: 1.26;
    letter-spacing: -0.02em;
}

.home-panel__subtext {
    margin: 0;
    max-width: 42ch;
    font-size: 0.95rem;
    line-height: 1.45;
    opacity: 0.78;
}

.home-panel__subtext--muted {
    font-size: 0.88rem;
    opacity: 0.65;
}

.home-panel__cta {
    margin-top: auto;
    align-self: flex-end;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0.5rem 0.9rem;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--text-color) 24%, transparent);
    background: color-mix(in srgb, var(--text-color) 12%, transparent);
    font-size: 0.8rem;
    font-weight: 600;
}

:root[data-theme="light"] .home-panel__dropzone {
    border-color: rgba(40, 68, 116, 0.26);
    background:
        radial-gradient(120% 110% at 100% 0%, rgba(57, 108, 216, 0.12), transparent 60%),
        linear-gradient(165deg, rgba(255, 255, 255, 0.94), rgba(236, 244, 255, 0.94));
    color: rgba(20, 34, 61, 0.92);
}

:root[data-theme="light"] .home-panel__dropzone:not(.home-panel__dropzone--hidden):hover {
    border-color: rgba(57, 108, 216, 0.74);
    box-shadow: 0 16px 30px rgba(57, 108, 216, 0.18);
}

:root[data-theme="light"] .home-panel__cta {
    background: rgba(57, 108, 216, 0.13);
    border-color: rgba(57, 108, 216, 0.28);
}

:root[data-theme="graphite"] .home-panel__dropzone:not(.home-panel__dropzone--hidden):hover {
    border-color: rgba(180, 198, 221, 0.72);
    color: #e8eef8;
    box-shadow: 0 12px 24px rgba(0, 0, 0, 0.24);
}

:root[data-platform="android"] .home-panel__dropzone {
    width: min(420px, calc(100% - 24px));
    max-width: calc(100% - 24px);
    box-sizing: border-box;
    min-height: 150px;
    max-height: min(44vh, 230px);
    gap: 7px;
    padding: 11px 15px;
    border-width: 1px;
    border-radius: 10px;
}

:root[data-platform="android"] .home-panel__headline {
    margin-top: 2px;
    max-width: 34ch;
    font-size: 14px;
    line-height: 1.24;
}

:root[data-platform="android"] .home-panel__subtext {
    width: 100%;
    max-width: none;
    box-sizing: border-box;
    font-size: 11px;
    line-height: 1.35;
}

:root[data-platform="android"] .home-panel__subtext--muted {
    font-size: 10px;
}

:root[data-platform="android"] .home-panel__cta {
    padding: 5px 10px;
    font-size: 10px;
}

@media (max-width: 820px) {
    .home-panel {
        min-height: 0;
    }

    .home-panel__dropzone {
        min-height: 240px;
        max-height: 340px;
    }

    .home-panel__headline {
        font-size: 1.16rem;
    }

    .home-panel__subtext {
        font-size: 0.9rem;
    }
}
</style>
