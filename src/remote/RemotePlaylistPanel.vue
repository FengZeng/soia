<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { CoreClientError } from "../core-client/CoreClient";
import type { PlaybackSnapshotDto } from "../core-client/generated/PlaybackSnapshotDto";
import type { PlaylistEntryDto } from "../core-client/generated/PlaylistEntryDto";
import type { PlaylistSnapshotDto } from "../core-client/generated/PlaylistSnapshotDto";
import { remoteCoreClient, remotePlaylistClient } from "./remoteCoreClient";

const PAGE_SIZE = 100;

const props = defineProps<{
    active: boolean;
}>();

const snapshot = ref<PlaylistSnapshotDto | null>(null);
const selectedPlaylistId = ref<string | null>(null);
const entries = ref<PlaylistEntryDto[]>([]);
const entriesTotal = ref(0);
const entriesLoading = ref(false);
const deletingPlaylistId = ref<string | null>(null);
const playbackKey = ref<string | null>(null);
const playbackPlaylistId = ref<string | null>(null);
const playbackSnapshotReceived = ref(false);
const error = ref("");
const requestClientId = createId("playlist-client");
let entriesRequestRevision = 0;
let preferredSelectionRevision = 0;
let preferredSelectionPending = false;
let unsubscribePlaylist: (() => void) | null = null;
let unsubscribePlayback: (() => void) | null = null;

const playlists = computed(() => snapshot.value?.playlists ?? []);
const selectedPlaylist = computed(() =>
    playlists.value.find((playlist) => playlist.id === selectedPlaylistId.value) ?? null,
);
const canLoadMore = computed(() => entries.value.length < entriesTotal.value);

function createId(prefix: string) {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
        return `${prefix}-${crypto.randomUUID()}`;
    }
    return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function message(error: unknown) {
    const clientError = error as CoreClientError;
    return clientError?.type === "core"
        ? clientError.error.message
        : clientError?.message || "Playlist request failed";
}

function entryName(entry: PlaylistEntryDto) {
    const title = entry.title?.trim();
    if (title) return title;
    const playbackKey = entry.playbackKey.trim();
    const path = playbackKey.replace(/[?#].*$/, "").replace(/[\\/]+$/, "");
    return path.split(/[\\/]/).pop() || playbackKey;
}

function isPlayingEntry(entry: PlaylistEntryDto) {
    return playbackPlaylistId.value === selectedPlaylistId.value
        && playbackKey.value !== null
        && entry.playbackKey === playbackKey.value;
}

function applySnapshot(nextSnapshot: PlaylistSnapshotDto) {
    snapshot.value = nextSnapshot;
    if (selectedPlaylistId.value && playlists.value.some((playlist) => playlist.id === selectedPlaylistId.value)) {
        void selectPreferredPlaylist();
        return;
    }
    selectedPlaylistId.value = playlists.value[0]?.id ?? null;
    void selectPreferredPlaylist();
}

function applyPlaybackSnapshot(nextSnapshot: PlaybackSnapshotDto) {
    playbackKey.value = nextSnapshot.playbackKey;
    playbackPlaylistId.value = nextSnapshot.playbackPlaylistId;
    playbackSnapshotReceived.value = true;
    void selectPreferredPlaylist();
}

async function findPlaylistContainingPlaybackKey(key: string) {
    for (const playlist of playlists.value) {
        if (!playlist.entryCount) continue;
        for (let offset = 0; offset < playlist.entryCount; offset += PAGE_SIZE) {
            const page = await remotePlaylistClient.getEntriesPage({
                playlistId: playlist.id,
                offset,
                limit: PAGE_SIZE,
            });
            if (page.entries.some((entry) => entry.playbackKey === key)) return playlist.id;
            if (!page.entries.length) break;
        }
    }
    return null;
}

async function selectPreferredPlaylist() {
    if (!preferredSelectionPending || !props.active || !snapshot.value || !playbackSnapshotReceived.value) {
        return;
    }
    const requestRevision = ++preferredSelectionRevision;
    const firstNonEmptyPlaylistId = playlists.value.find((playlist) => playlist.entryCount > 0)?.id
        ?? playlists.value[0]?.id
        ?? null;
    const currentPlaybackPlaylistId = playbackPlaylistId.value;
    let matchingPlaylistId = currentPlaybackPlaylistId
        && playlists.value.some((playlist) => playlist.id === currentPlaybackPlaylistId)
        ? currentPlaybackPlaylistId
        : null;
    if (!matchingPlaylistId && playbackKey.value) {
        try {
            matchingPlaylistId = await findPlaylistContainingPlaybackKey(playbackKey.value);
        } catch (nextError) {
            error.value = message(nextError);
        }
    }
    if (
        requestRevision !== preferredSelectionRevision
        || !preferredSelectionPending
        || !props.active
    ) {
        return;
    }
    preferredSelectionPending = false;
    selectedPlaylistId.value = matchingPlaylistId ?? firstNonEmptyPlaylistId;
}

function requestPreferredPlaylistSelection() {
    preferredSelectionPending = true;
    void selectPreferredPlaylist();
}

async function loadEntries(reset: boolean) {
    const playlist = selectedPlaylist.value;
    if (!playlist || entriesLoading.value) return;
    const requestRevision = ++entriesRequestRevision;
    const offset = reset ? 0 : entries.value.length;
    entriesLoading.value = true;
    try {
        const page = await remotePlaylistClient.getEntriesPage({
            playlistId: playlist.id,
            offset,
            limit: PAGE_SIZE,
        });
        if (requestRevision !== entriesRequestRevision || selectedPlaylistId.value !== playlist.id) return;
        entries.value = reset ? page.entries : [...entries.value, ...page.entries];
        entriesTotal.value = page.total;
    } catch (nextError) {
        if (requestRevision === entriesRequestRevision) error.value = message(nextError);
    } finally {
        if (requestRevision === entriesRequestRevision) entriesLoading.value = false;
    }
}

function selectPlaylist(playlistId: string) {
    if (selectedPlaylistId.value === playlistId) return;
    selectedPlaylistId.value = playlistId;
}

async function playEntry(entry: PlaylistEntryDto) {
    const playlist = selectedPlaylist.value;
    if (!playlist) return;
    error.value = "";
    try {
        await remotePlaylistClient.playEntry({
            commandId: createId("playlist-play"),
            clientId: requestClientId,
            playlistId: playlist.id,
            entryId: entry.id,
        });
        playbackKey.value = entry.playbackKey;
        playbackPlaylistId.value = playlist.id;
    } catch (nextError) {
        error.value = message(nextError);
    }
}

async function deletePlaylist() {
    const playlist = selectedPlaylist.value;
    if (!playlist || playlist.isProtected) return;
    if (!window.confirm(`Delete “${playlist.name}”? This cannot be undone.`)) return;
    deletingPlaylistId.value = playlist.id;
    error.value = "";
    try {
        await remotePlaylistClient.delete({
            playlistId: playlist.id,
            expectedPlaylistRevision: playlist.revision,
        });
    } catch (nextError) {
        error.value = message(nextError);
    } finally {
        deletingPlaylistId.value = null;
    }
}

watch(selectedPlaylistId, () => {
    entriesRequestRevision += 1;
    entries.value = [];
    entriesTotal.value = 0;
    void loadEntries(true);
});

watch(
    () => props.active,
    (active) => {
        if (active) requestPreferredPlaylistSelection();
    },
);

watch(
    () => selectedPlaylist.value?.revision,
    (revision, previousRevision) => {
        if (revision !== undefined && previousRevision !== undefined && revision !== previousRevision) {
            void loadEntries(true);
        }
    },
);

onMounted(() => {
    unsubscribePlaylist = remotePlaylistClient.subscribe(applySnapshot);
    unsubscribePlayback = remoteCoreClient.subscribe(applyPlaybackSnapshot);
    void remotePlaylistClient.getSnapshot().then(applySnapshot).catch((nextError) => {
        error.value = message(nextError);
    });
});

onBeforeUnmount(() => {
    unsubscribePlaylist?.();
    unsubscribePlayback?.();
});
</script>

<template>
    <section class="remote-playlists" aria-labelledby="playlist-heading">
        <div class="remote-playlists__heading">
            <div>
                <p class="eyebrow">PLAYLISTS</p>
                <h2 id="playlist-heading">Queue library</h2>
            </div>
            <span class="remote-playlists__count">{{ playlists.length }}</span>
        </div>

        <p v-if="error" class="remote-playlists__error" role="alert">{{ error }}</p>

        <div v-if="playlists.length" class="remote-playlists__body">
            <nav class="remote-playlists__list" aria-label="Playlists">
                <button
                    v-for="playlist in playlists"
                    :key="playlist.id"
                    class="remote-playlists__item"
                    :class="{ 'remote-playlists__item--selected': playlist.id === selectedPlaylistId }"
                    :aria-current="playlist.id === selectedPlaylistId ? 'true' : undefined"
                    @click="selectPlaylist(playlist.id)"
                >
                    <span>{{ playlist.name }}</span>
                    <small>{{ playlist.entryCount }}</small>
                </button>
            </nav>

            <div v-if="selectedPlaylist" class="remote-playlists__entries">
                <div class="remote-playlists__selection">
                    <div>
                        <strong>{{ selectedPlaylist.name }}</strong>
                        <span>{{ entriesTotal }} tracks</span>
                    </div>
                    <button
                        v-if="!selectedPlaylist.isProtected"
                        class="remote-playlists__delete"
                        :disabled="deletingPlaylistId === selectedPlaylist.id"
                        @click="deletePlaylist"
                    >
                        {{ deletingPlaylistId === selectedPlaylist.id ? "Deleting…" : "Delete" }}
                    </button>
                </div>
                <ol class="remote-playlists__entry-list">
                    <li
                        v-for="entry in entries"
                        :key="entry.id"
                        :class="{ 'remote-playlists__entry--playing': isPlayingEntry(entry) }"
                    >
                        <button
                            :aria-current="isPlayingEntry(entry) ? 'true' : undefined"
                            @click="playEntry(entry)"
                        >
                            <span>{{ entryName(entry) }}</span>
                            <small v-if="isPlayingEntry(entry)">Playing</small>
                        </button>
                    </li>
                </ol>
                <p v-if="entriesLoading && !entries.length" class="remote-playlists__empty">Loading tracks…</p>
                <p v-else-if="!entriesLoading && !entries.length" class="remote-playlists__empty">No tracks yet.</p>
                <button
                    v-if="canLoadMore"
                    class="remote-playlists__more"
                    :disabled="entriesLoading"
                    @click="loadEntries(false)"
                >
                    {{ entriesLoading ? "Loading…" : `Load more (${entriesTotal - entries.length})` }}
                </button>
            </div>
        </div>
        <p v-else class="remote-playlists__empty">No playlists available.</p>
    </section>
</template>
