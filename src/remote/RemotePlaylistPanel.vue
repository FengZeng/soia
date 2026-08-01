<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { CoreClientError } from "../core-client/CoreClient";
import type { PlaylistEntryDto } from "../core-client/generated/PlaylistEntryDto";
import type { PlaylistSnapshotDto } from "../core-client/generated/PlaylistSnapshotDto";
import { remotePlaylistClient } from "./remoteCoreClient";

const PAGE_SIZE = 100;

const snapshot = ref<PlaylistSnapshotDto | null>(null);
const selectedPlaylistId = ref<string | null>(null);
const entries = ref<PlaylistEntryDto[]>([]);
const entriesTotal = ref(0);
const entriesLoading = ref(false);
const importing = ref(false);
const deletingPlaylistId = ref<string | null>(null);
const source = ref("");
const error = ref("");
const requestClientId = createId("playlist-client");
let entriesRequestRevision = 0;
let unsubscribe: (() => void) | null = null;

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

function applySnapshot(nextSnapshot: PlaylistSnapshotDto) {
    snapshot.value = nextSnapshot;
    if (selectedPlaylistId.value && playlists.value.some((playlist) => playlist.id === selectedPlaylistId.value)) {
        return;
    }
    selectedPlaylistId.value = playlists.value[0]?.id ?? null;
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

async function playEntry(entryId: string) {
    const playlist = selectedPlaylist.value;
    if (!playlist) return;
    error.value = "";
    try {
        await remotePlaylistClient.playEntry({
            commandId: createId("playlist-play"),
            clientId: requestClientId,
            playlistId: playlist.id,
            entryId,
        });
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

async function importPlaylist() {
    const value = source.value.trim();
    if (!value || importing.value) return;
    importing.value = true;
    error.value = "";
    try {
        const result = await remotePlaylistClient.importFromSource({ source: value });
        source.value = "";
        selectedPlaylistId.value = result.playlist?.summary.id ?? selectedPlaylistId.value;
    } catch (nextError) {
        error.value = message(nextError);
    } finally {
        importing.value = false;
    }
}

watch(selectedPlaylistId, () => {
    entriesRequestRevision += 1;
    entries.value = [];
    entriesTotal.value = 0;
    void loadEntries(true);
});

watch(
    () => selectedPlaylist.value?.revision,
    (revision, previousRevision) => {
        if (revision !== undefined && previousRevision !== undefined && revision !== previousRevision) {
            void loadEntries(true);
        }
    },
);

onMounted(() => {
    unsubscribe = remotePlaylistClient.subscribe(applySnapshot);
    void remotePlaylistClient.getSnapshot().then(applySnapshot).catch((nextError) => {
        error.value = message(nextError);
    });
});

onBeforeUnmount(() => unsubscribe?.());
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
                    <li v-for="entry in entries" :key="entry.id">
                        <button @click="playEntry(entry.id)">
                            <span>{{ entryName(entry) }}</span>
                            <small>Play</small>
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

        <form class="remote-playlists__import" @submit.prevent="importPlaylist">
            <label for="remote-playlist-source">Import a supported playlist source</label>
            <div>
                <input
                    id="remote-playlist-source"
                    v-model="source"
                    type="url"
                    inputmode="url"
                    autocomplete="url"
                    placeholder="https://example.com/playlist.m3u"
                >
                <button :disabled="!source.trim() || importing" type="submit">
                    {{ importing ? "Importing…" : "Import" }}
                </button>
            </div>
        </form>
    </section>
</template>
