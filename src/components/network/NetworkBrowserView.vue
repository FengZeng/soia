<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { NetworkFileRow } from "../../types/network";
import { settingsLocale, translateSettingsText } from "../../i18n";

const props = defineProps<{
    isLoading: boolean;
    networkPath: string;
    entries: NetworkFileRow[];
    hasFiles: boolean;
}>();

const emit = defineEmits<{
    (e: "entry-click", entry: NetworkFileRow): void;
}>();

const listEl = ref<HTMLElement | null>(null);
const tr = (text: string): string =>
    translateSettingsText(settingsLocale.value, text);

const activeEntryScrollKey = computed(() => {
    if (props.isLoading) return "";
    const activeEntry = props.entries.find(
        (entry) => entry.isActive || entry.containsActive,
    );
    return activeEntry ? `${props.networkPath}:${activeEntry.path}` : "";
});

const scrollActiveEntryIntoView = async () => {
    if (!activeEntryScrollKey.value) return;
    await nextTick();
    const list = listEl.value;
    const activeEntry = list?.querySelector<HTMLElement>(
        ".network-entry--active, .network-entry--contains-active",
    );
    if (!list || !activeEntry) return;

    const targetTop =
        activeEntry.offsetTop - list.clientHeight / 2 + activeEntry.clientHeight / 2;
    list.scrollTo({
        top: Math.max(0, targetTop),
        behavior: "auto",
    });
};

watch(
    activeEntryScrollKey,
    () => {
        void scrollActiveEntryIntoView();
    },
    { immediate: true },
);
</script>

<template>
    <div class="panel__stack">
        <div class="panel__section panel__section--grow network-browser__section">
            <div class="panel__table panel__table--card panel__table--grow">
                <div ref="listEl" class="network-browser-list">
                    <div
                        v-if="props.isLoading"
                        class="network-folder-loading"
                        aria-live="polite"
                    >
                        <div class="network-files-loader__spinner"></div>
                        <div class="network-files-loader__text">
                            {{ tr("Loading") }} {{ props.networkPath }}
                        </div>
                    </div>
                    <template v-else>
                        <button
                            v-for="file in props.entries"
                            :key="file.path"
                            class="network-entry"
                            :class="{
                                'network-entry--dir': file.type === 'DIR',
                                'network-entry--parent': file.isParent,
                                'network-entry--active': file.isActive,
                                'network-entry--contains-active':
                                    file.containsActive,
                            }"
                            type="button"
                            @click="emit('entry-click', file)"
                        >
                            <div
                                class="network-entry__icon"
                                :class="{
                                    'network-entry__icon--dir': file.type === 'DIR',
                                    'network-entry__icon--parent': file.isParent,
                                }"
                            >
                                <svg
                                    v-if="file.isParent"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path d="M10 8l-4 4 4 4" />
                                    <path d="M20 12H6" />
                                </svg>
                                <svg
                                    v-else-if="file.type === 'DIR'"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path
                                        d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
                                    />
                                </svg>
                                <svg
                                    v-else
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path
                                        d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"
                                    />
                                    <path d="M14 3v5h5" />
                                </svg>
                            </div>
                            <div class="network-entry__content">
                                <div class="network-entry__name">
                                    {{ file.name }}
                                </div>
                                <div class="network-entry__meta">
                                    <span>{{
                                        file.type === "DIR"
                                            ? file.isParent
                                                ? tr("Parent folder")
                                                : tr("Folder")
                                            : file.playbackProgressText || file.size
                                    }}</span>
                                    <span>{{ file.modified || "—" }}</span>
                                </div>
                            </div>
                            <svg
                                v-if="file.type === 'DIR'"
                                class="network-entry__chevron"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            >
                                <path d="M9 6l6 6-6 6" />
                            </svg>
                        </button>
                        <div v-if="!props.hasFiles" class="panel__empty">
                            <div class="panel__empty-title">{{ tr("Empty folder") }}</div>
                            <div class="panel__empty-body">
                                {{ tr("No files found for this location.") }}
                            </div>
                        </div>
                    </template>
                </div>
            </div>
        </div>
    </div>
</template>
