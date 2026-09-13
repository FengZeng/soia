<script setup lang="ts">
import { settingsLocale, translateSettingsText } from "../i18n";
import type { HistoryEntry } from "../types/history";
import { formatDateTime } from "../utils/formatDateTime";
import { formatMonthDay } from "../utils/formatMonthDay";
import { formatTime } from "../utils/formatTime";
import { useHistoryPanel } from "../composables/useHistoryPanel";

const props = defineProps<{
    history: HistoryEntry[];
    isLoading: boolean;
}>();

const emit = defineEmits<{
    (e: "play-history", entry: HistoryEntry): void;
    (e: "clear-history"): void;
    (e: "remove-history", entry: HistoryEntry): void;
    (e: "toggle-pin-history", entry: HistoryEntry): void;
}>();

const {
    expandedPath,
    getDisplayName,
    getDisplayPath,
    getProtocolBadges,
    getPlaybackProgressPercent,
    getPlaybackProgressLabel,
    middleEllipsis,
    toggleExpanded,
    clearExpandedIfMatches,
} = useHistoryPanel();

const onRemoveEntry = (entry: HistoryEntry) => {
    emit("remove-history", entry);
    clearExpandedIfMatches(entry.path);
};

const tr = (text: string): string =>
    translateSettingsText(settingsLocale.value, text);
</script>

<template>
    <div class="history panel panel--history">
        <div class="panel__header">
            <div class="panel__title">{{ tr("History") }}</div>
            <button
                class="panel__reset"
                type="button"
                @click.stop="emit('clear-history')"
                :disabled="props.isLoading"
            >
                {{ tr("Clear") }}
            </button>
        </div>
        <div class="panel__stack">
            <div class="panel__section panel__section--grow">
                <div class="history__content">
                    <div v-if="props.isLoading" class="panel__skeleton">
                        <div class="panel__skeleton-row"></div>
                        <div class="panel__skeleton-row"></div>
                        <div class="panel__skeleton-row"></div>
                    </div>

                    <div v-else-if="!props.history.length" class="panel__empty">
                        <div class="panel__empty-title">{{ tr("No recent plays") }}</div>
                        <div class="panel__empty-body">
                            {{ tr("Open a file to start building your playback history.") }}
                        </div>
                    </div>

                    <div v-else class="history__list">
                        <div
                            v-for="entry in props.history"
                            :key="entry.path"
                            class="history__item"
                            role="button"
                            tabindex="0"
                            @click="emit('play-history', entry)"
                            @keydown.enter="emit('play-history', entry)"
                            @keydown.space.prevent="emit('play-history', entry)"
                        >
                            <div class="history__header">
                                <button
                                    class="history__toggle"
                                    type="button"
                                    @click.stop="toggleExpanded(entry.path)"
                                    @keydown.enter.stop
                                    @keydown.space.prevent.stop
                                    :aria-expanded="expandedPath === entry.path"
                                    :title="
                                        expandedPath === entry.path
                                            ? tr('Collapse')
                                            : tr('Expand')
                                    "
                                >
                                    <svg
                                        class="history__chevron"
                                        :class="{
                                            'history__chevron--open':
                                                expandedPath === entry.path,
                                        }"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="2"
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                        aria-hidden="true"
                                    >
                                        <path d="M6 9l6 6 6-6" />
                                    </svg>
                                </button>
                                <div class="history__main">
                                    <div
                                        class="history__name"
                                        :class="{
                                            'history__name--expanded':
                                                expandedPath === entry.path,
                                        }"
                                    >
                                        {{
                                            middleEllipsis(
                                                getDisplayName(entry),
                                            )
                                        }}
                                    </div>
                                    <span class="history__protocols">
                                        <span
                                            v-for="badge in getProtocolBadges(entry)"
                                            :key="badge.id"
                                            class="history__protocol"
                                            :class="`history__protocol--${badge.id}`"
                                            :title="`${tr('Resume')} ${getPlaybackProgressLabel(entry)}`"
                                            :style="{
                                                '--history-protocol-progress': badge.showsProgress
                                                    ? `${getPlaybackProgressPercent(entry)}%`
                                                    : undefined,
                                            }"
                                        >
                                            {{ badge.label }}
                                        </span>
                                    </span>
                                </div>
                                <div class="history__side">
                                    <span
                                        class="history__last-played"
                                        :title="formatDateTime(entry.lastPlayedAt)"
                                    >
                                        {{ formatMonthDay(entry.lastPlayedAt) }}
                                    </span>
                                    <button
                                        class="history__pin"
                                        :class="{
                                            'history__pin--active': entry.isPinned,
                                        }"
                                        type="button"
                                        :aria-label="
                                            entry.isPinned
                                                ? tr('Unpin history item')
                                                : tr('Pin history item')
                                        "
                                        :title="
                                            entry.isPinned ? tr('Unpin from top') : tr('Pin to top')
                                        "
                                        @click.stop="emit('toggle-pin-history', entry)"
                                        @keydown.enter.stop
                                        @keydown.space.prevent.stop
                                    >
                                        <svg
                                            class="history__pin-icon"
                                            xmlns="http://www.w3.org/2000/svg"
                                            viewBox="0 -960 960 960"
                                            fill="currentColor"
                                            aria-hidden="true"
                                        >
                                            <path d="m640-480 80 80v80H520v240l-40 40-40-40v-240H240v-80l80-80v-280h-40v-80h400v80h-40v280Zm-286 80h252l-46-46v-314H400v314l-46 46Zm126 0Z" />
                                        </svg>
                                    </button>
                                </div>
                            </div>
                            <div
                                v-if="expandedPath === entry.path"
                                class="history__details"
                            >
                                <div class="history__meta history__meta--inline">
                                    <span class="history__meta-main">
                                        {{ formatTime(entry.lastPosition) }}
                                        /
                                        {{
                                            entry.duration > 0
                                                ? formatTime(entry.duration)
                                                : "—"
                                        }}
                                    </span>
                                    <span class="history__meta-sep">·</span>
                                    <span class="history__meta--secondary">
                                    {{ formatDateTime(entry.lastPlayedAt) }}
                                    </span>
                                </div>
                                <div class="history__path">
                                    {{ getDisplayPath(entry.path) }}
                                </div>
                                <button
                                    class="history__remove"
                                    type="button"
                                    :aria-label="tr('Remove history item')"
                                    @click.stop="onRemoveEntry(entry)"
                                    @keydown.enter.stop
                                    @keydown.space.prevent.stop
                                >
                                    <svg
                                        class="history__remove-icon"
                                        xmlns="http://www.w3.org/2000/svg"
                                        height="24px"
                                        viewBox="0 -960 960 960"
                                        width="24px"
                                        fill="currentColor"
                                    >
                                        <path
                                            d="M280-120q-33 0-56.5-23.5T200-200v-520h-40v-80h200v-40h240v40h200v80h-40v520q0 33-23.5 56.5T680-120H280Zm400-600H280v520h400v-520ZM360-280h80v-360h-80v360Zm160 0h80v-360h-80v360ZM280-720v520-520Z"
                                        />
                                    </svg>
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<style scoped src="../styles/panels.css"></style>

<style scoped>
.history {
    display: flex;
    flex-direction: column;
    gap: 12px;
    pointer-events: auto;
    cursor: default;
    user-select: none;
    -webkit-user-select: none;
    transform: none;
    overflow: hidden;
}

.history__list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 6px 6px 6px;
    background: transparent;
    scrollbar-width: none;
    -ms-overflow-style: none;
}

.history__list::-webkit-scrollbar {
    width: 0;
    height: 0;
}

.history__content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
}

.history__item {
    width: 100%;
    box-sizing: border-box;
    text-align: left;
    border: none;
    --history-title-line-height: 20px;
    background: transparent;
    color: var(--text-color);
    border-radius: 10px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    transition:
        border-color 0.2s ease,
        background-color 0.2s ease,
        transform 0.1s ease;
    cursor: pointer;
}

.history__item:hover {
    background: rgba(0, 0, 0, 0.05);
    transform: translateY(-1px);
}

.history__item:focus-visible {
    outline: 2px solid rgba(57, 108, 216, 0.55);
    outline-offset: 1px;
}

.history__header {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    min-width: 0;
    width: 100%;
}

.history__main {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    min-width: 0;
    flex: 1 1 auto;
}

.history__toggle {
    width: 18px;
    height: 18px;
    border: none;
    background: transparent;
    padding: 0;
    display: grid;
    place-items: center;
    cursor: pointer;
    color: inherit;
    flex: 0 0 auto;
}

.history__chevron {
    width: 16px;
    height: 16px;
    transform: rotate(-90deg);
    transition: transform 0.2s ease;
}

.history__chevron--open {
    transform: rotate(0deg);
}

.history__name {
    font-size: 14px;
    font-weight: 400;
    line-height: var(--history-title-line-height);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
}

.history__name--expanded {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
}

.history__side {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
    height: var(--history-title-line-height);
    flex: 0 0 auto;
}

.history__protocol {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    font-size: 8px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: rgba(255, 255, 255, 0.74);
    background: linear-gradient(
        90deg,
        rgba(73, 140, 255, 0.38) 0%,
        rgba(73, 140, 255, 0.38) var(--history-protocol-progress, 0%),
        rgba(255, 255, 255, 0.06) var(--history-protocol-progress, 0%),
        rgba(255, 255, 255, 0.06) 100%
    );
    flex: 0 0 auto;
    transition: background 0.2s ease;
    min-width: 45px;
    text-align: center;
}

.history__protocols {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
    height: var(--history-title-line-height);
}

.history__protocol--live {
    min-width: 38px;
}

.history__protocol--live::before {
    content: "";
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: #ff5252;
    flex: 0 0 auto;
}

:root[data-theme="light"] .history__protocol {
    border-color: rgba(57, 108, 216, 0.38);
    color: rgba(33, 62, 112, 0.92);
    background: linear-gradient(
        90deg,
        rgba(73, 140, 255, 0.42) 0%,
        rgba(73, 140, 255, 0.42) var(--history-protocol-progress, 0%),
        rgba(29, 49, 82, 0.14) var(--history-protocol-progress, 0%),
        rgba(29, 49, 82, 0.14) 100%
    );
}

.history__last-played {
    font-size: 11px;
    color: rgba(0, 0, 0, 0.52);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    display: inline-flex;
    align-items: center;
    flex: 0 0 auto;
    min-width: 36px;
    text-align: right;
}

.history__pin {
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    color: rgba(0, 0, 0, 0.5);
    padding: 0;
    border-radius: 6px;
    cursor: pointer;
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    opacity: 0.45;
    transition:
        color 0.2s ease,
        background-color 0.2s ease,
        opacity 0.2s ease;
}

.history__pin:hover {
    background: rgba(0, 0, 0, 0.06);
    opacity: 1;
}

.history__pin:focus-visible {
    outline: 2px solid rgba(57, 108, 216, 0.5);
    outline-offset: 1px;
    opacity: 1;
}

.history__pin--active {
    color: rgb(50, 109, 223);
    opacity: 1;
}

.history__pin-icon {
    width: 15px;
    height: 15px;
}

.history__details {
    padding-left: 26px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    box-sizing: border-box;
    position: relative;
}

.history__remove {
    position: absolute;
    right: -2px;
    bottom: -2px;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: inherit;
    padding: 0;
    border-radius: 8px;
    cursor: pointer;
    display: grid;
    place-items: center;
    opacity: 0;
    pointer-events: none;
    transition:
        color 0.2s ease,
        background-color 0.2s ease,
        transform 0.2s ease,
        opacity 0.3s ease;
}

.history__item:hover .history__remove,
.history__details:hover .history__remove,
.history__remove:focus-visible {
    opacity: 1;
    pointer-events: auto;
}

.history__remove:focus-visible {
    outline: 2px solid rgba(57, 108, 216, 0.5);
    outline-offset: 1px;
}

.history__remove-icon {
    width: 20px;
    height: 20px;
    fill: currentColor;
}

.history__remove:hover {
    background: rgba(0, 0, 0, 0.06);
    color: rgba(168, 35, 35, 0.95);
    transform: translateY(-1px);
}

.history__meta {
    font-size: 12px;
    color: rgba(0, 0, 0, 0.55);
}

.history__meta--secondary {
    font-size: 11px;
    color: rgba(0, 0, 0, 0.5);
}

.history__meta--inline {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    min-width: 0;
}

.history__meta-main {
    font-size: 12px;
    font-weight: 600;
    color: rgba(0, 0, 0, 0.68);
}

.history__meta-sep {
    color: rgba(0, 0, 0, 0.38);
}

.history__path {
    font-size: 11px;
    color: rgba(0, 0, 0, 0.45);
    word-break: break-all;
}

@media (prefers-color-scheme: dark) {
    :root:not([data-theme]) .history__item {
        border: none;
        background: transparent;
        color: #fff;
    }

    :root:not([data-theme]) .history__item:hover {
        background: rgba(255, 255, 255, 0.06);
    }

    :root:not([data-theme]) .history__meta {
        color: rgba(255, 255, 255, 0.7);
    }

    :root:not([data-theme]) .history__meta--secondary {
        color: rgba(255, 255, 255, 0.6);
    }

    :root:not([data-theme]) .history__meta-main {
        color: rgba(255, 255, 255, 0.86);
    }

    :root:not([data-theme]) .history__meta-sep {
        color: rgba(255, 255, 255, 0.36);
    }

    :root:not([data-theme]) .history__path {
        color: rgba(255, 255, 255, 0.5);
    }

    :root:not([data-theme]) .history__last-played {
        color: rgba(255, 255, 255, 0.58);
    }

    :root:not([data-theme]) .history__pin {
        color: rgba(255, 255, 255, 0.6);
    }

    :root:not([data-theme]) .history__pin:hover {
        background: rgba(255, 255, 255, 0.1);
    }

    :root:not([data-theme]) .history__pin--active {
        color: rgba(122, 171, 255, 0.98);
    }

    :root:not([data-theme]) .history__remove:hover {
        background: rgba(255, 255, 255, 0.1);
        color: rgba(255, 177, 177, 0.95);
        transform: translateY(-1px);
    }

}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__item {
    border: none;
    background: transparent;
    color: #fff;
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__item:hover {
    background: rgba(255, 255, 255, 0.06);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__meta {
    color: rgba(255, 255, 255, 0.7);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__meta--secondary {
    color: rgba(255, 255, 255, 0.6);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__meta-main {
    color: rgba(255, 255, 255, 0.86);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__meta-sep {
    color: rgba(255, 255, 255, 0.36);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__path {
    color: rgba(255, 255, 255, 0.5);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__last-played {
    color: rgba(255, 255, 255, 0.58);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__pin {
    color: rgba(255, 255, 255, 0.6);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__pin:hover {
    background: rgba(255, 255, 255, 0.1);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__pin--active {
    color: rgba(122, 171, 255, 0.98);
}

:root:is([data-theme="dark"], [data-theme="graphite"]) .history__remove:hover {
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 177, 177, 0.95);
    transform: translateY(-1px);
}
</style>
