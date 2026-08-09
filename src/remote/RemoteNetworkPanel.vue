<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import type { NetworkBrowseResultDto } from "../core-client/generated/NetworkBrowseResultDto";
import type { NetworkConnectionSummaryDto } from "../core-client/generated/NetworkConnectionSummaryDto";
import type { PlaybackSnapshotDto } from "../core-client/generated/PlaybackSnapshotDto";
import { remoteCoreClient } from "./remoteCoreClient";

const connections = ref<NetworkConnectionSummaryDto[]>([]);
const selectedConnection = ref<NetworkConnectionSummaryDto | null>(null);
const result = ref<NetworkBrowseResultDto | null>(null);
const loading = ref(false);
const error = ref("");
const pathHistory = ref<string[]>([]);
const playbackKey = ref<string | null>(null);
const dlnaParentById = reactive(new Map<string, string>());
let unsubscribePlayback: (() => void) | null = null;
const atHome = computed(() => selectedConnection.value === null);
const normalizeFilePath = (path: string) => {
  const trimmed = path.trim();
  if (!trimmed || trimmed === "/") return "/";
  return `${trimmed.startsWith("/") ? "" : "/"}${trimmed}`.replace(/\/+$/, "");
};
const normalizeDlnaObjectId = (path: string) => path.trim().replace(/^\/+/, "") || "0";
const parentPath = (path: string) => {
  const normalized = normalizeFilePath(path);
  if (normalized === "/") return null;
  const index = normalized.lastIndexOf("/");
  return index === 0 ? "/" : normalized.slice(0, index);
};
const connectionIdFromKey = (prefix: "webdav" | "smb", value: string) =>
  /^\d+$/.test(value) ? `${prefix}-${value}` : value;
type ActiveNetworkSource =
  | { kind: "webdav" | "smb"; connectionId: string; filePath: string }
  | { kind: "dlna"; connectionId: string; parentPath: string | null };
const activeNetworkSource = computed<ActiveNetworkSource | null>(() => {
  const key = playbackKey.value?.trim() ?? "";
  const parseFileKey = (prefix: string, kind: "webdav" | "smb") => {
    if (!key.startsWith(prefix)) return null;
    const value = key.slice(prefix.length);
    const slashIndex = value.indexOf("/");
    if (slashIndex <= 0) return null;
    try {
      const connectionId = connectionIdFromKey(kind, decodeURIComponent(value.slice(0, slashIndex)));
      return { kind, connectionId, filePath: normalizeFilePath(value.slice(slashIndex)) };
    } catch { return null; }
  };
  const fileSource = parseFileKey("soia-webdav://", "webdav")
    ?? parseFileKey("soia-smb://", "smb");
  if (fileSource) return fileSource;
  if (!key.startsWith("soia-dlna://")) return null;
  const value = key.slice("soia-dlna://".length);
  const slashIndex = value.indexOf("/");
  if (slashIndex <= 0) return null;
  const parts = value.slice(slashIndex + 1).split("/");
  try {
    return {
      kind: "dlna",
      connectionId: decodeURIComponent(value.slice(0, slashIndex)),
      parentPath: parts[1] ? normalizeDlnaObjectId(decodeURIComponent(parts[1])) : null,
    };
  } catch { return null; }
});
const activeFolderPaths = computed(() => {
  const source = activeNetworkSource.value;
  if (!source || source.connectionId !== selectedConnection.value?.id) return new Set<string>();
  const paths = new Set<string>();
  if (source.kind === "dlna") {
    let cursor = source.parentPath;
    while (cursor) {
      paths.add(cursor);
      cursor = dlnaParentById.get(cursor) ?? null;
    }
    return paths;
  }
  let cursor = parentPath(source.filePath);
  while (cursor) {
    paths.add(cursor);
    cursor = parentPath(cursor);
  }
  return paths;
});
const pathCrumbs = computed(() =>
  (result.value?.path ?? "") === "/"
    ? []
    : (result.value?.path ?? "")
        .split("/")
        .filter(Boolean)
        .map((label, index, segments) => ({
          label,
          path: `/${segments.slice(0, index + 1).join("/")}`,
        })),
);

const load = async (path?: string): Promise<boolean> => {
  if (!selectedConnection.value) return false;
  loading.value = true; error.value = "";
  try {
    const nextResult = await remoteCoreClient.browseNetworkConnection({ connectionId: selectedConnection.value.id, path: path ?? null });
    result.value = nextResult;
    if (selectedConnection.value.protocol === "http-dlna" || selectedConnection.value.protocol === "dlna") {
      const currentPath = normalizeDlnaObjectId(nextResult.path);
      for (const entry of nextResult.entries) {
        dlnaParentById.set(normalizeDlnaObjectId(entry.path), currentPath);
      }
    }
    return true;
  } catch {
    error.value = "Network browse failed";
    return false;
  } finally { loading.value = false; }
};
// An empty path is an initial connection request. The host resolves it to the
// connection's shared defaultPath, which the desktop app updates after browsing.
const openConnection = (connection: NetworkConnectionSummaryDto) => { selectedConnection.value = connection; result.value = null; pathHistory.value = []; dlnaParentById.clear(); void load(); };
const goHome = () => { selectedConnection.value = null; result.value = null; pathHistory.value = []; error.value = ""; };
const refresh = async () => {
  if (loading.value) return;
  if (!selectedConnection.value) {
    loading.value = true; error.value = "";
    try { connections.value = await remoteCoreClient.getNetworkConnections(); }
    catch { error.value = "Network connections are unavailable"; }
    finally { loading.value = false; }
    return;
  }
  await load(result.value?.path);
};
const goBack = async () => {
  const parentPath = pathHistory.value.at(-1);
  if (!parentPath || loading.value) return;
  if (await load(parentPath)) pathHistory.value.pop();
};
const browsePath = async (path: string) => {
  if (loading.value || path === result.value?.path) return;
  if (await load(path)) {
    const index = pathHistory.value.lastIndexOf(path);
    pathHistory.value = index >= 0 ? pathHistory.value.slice(0, index) : [];
  }
};
const open = async (entry: NetworkBrowseResultDto["entries"][number]) => {
  if (entry.entryType === "dir") {
    const currentPath = result.value?.path;
    if (currentPath && await load(entry.path)) pathHistory.value.push(currentPath);
    return;
  }
  if (entry.playbackKey) void remoteCoreClient.execute({ type: "playSource", key: entry.playbackKey, title: entry.name });
};

const isPlayingEntry = (entry: NetworkBrowseResultDto["entries"][number]) =>
  entry.entryType !== "dir" && entry.playbackKey === playbackKey.value;
const containsPlayingEntry = (entry: NetworkBrowseResultDto["entries"][number]) =>
  entry.entryType === "dir"
    && activeFolderPaths.value.has(
      selectedConnection.value?.protocol === "http-dlna" || selectedConnection.value?.protocol === "dlna"
        ? normalizeDlnaObjectId(entry.path)
        : normalizeFilePath(entry.path),
    );

onMounted(() => {
  unsubscribePlayback = remoteCoreClient.subscribe((snapshot: PlaybackSnapshotDto) => {
    playbackKey.value = snapshot.playbackKey;
  });
  void refresh();
});

onBeforeUnmount(() => {
  unsubscribePlayback?.();
  unsubscribePlayback = null;
});
</script>
<template>
  <section class="remote-network"><div class="remote-network__heading"><button v-if="!atHome" class="remote-network__home" aria-label="Network home" title="Home" @click="goHome"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 10.5 12 4l8 6.5v8A1.5 1.5 0 0 1 18.5 20h-13A1.5 1.5 0 0 1 4 18.5Z"/><path d="M9 20v-6h6v6"/></svg></button><h2>{{ atHome ? 'Media sources' : selectedConnection?.label }}</h2><button class="remote-network__refresh" :disabled="loading" aria-label="Refresh" title="Refresh" @click="refresh"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M17.65 6.35C16.2 4.9 14.21 4 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08c-.82 2.33-3.04 4-5.65 4-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4z"/></svg></button></div>
    <p v-if="error" class="remote-playlists__error">{{ error }}</p>
    <div v-if="atHome" class="remote-network__connections"><button v-for="item in connections" :key="item.id" class="remote-network__entry" @click="openConnection(item)"><span>⌁</span><span>{{ item.label }}<small>{{ item.protocol }}</small></span></button><p v-if="!connections.length && !loading">No network sources available</p></div>
    <template v-else><div v-if="result?.path !== '/'" class="remote-network__path"><button v-if="pathHistory.length" class="remote-network__back" :disabled="loading" aria-label="Back to parent folder" @click="goBack"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m14 18-6-6 6-6M8 12h12"/></svg></button><button class="remote-network__crumb remote-network__crumb--root" :disabled="loading" @click="browsePath('/')">/</button><template v-for="(crumb, index) in pathCrumbs" :key="crumb.path"><span v-if="index > 0">/</span><button class="remote-network__crumb" :class="{ 'remote-network__crumb--current': crumb.path === result?.path }" :disabled="loading || crumb.path === result?.path" @click="browsePath(crumb.path)">{{ crumb.label }}</button></template></div><button v-for="entry in result?.entries ?? []" :key="entry.path" class="remote-network__entry" :class="{ 'remote-network__entry--playing': isPlayingEntry(entry), 'remote-network__entry--contains-playing': containsPlayingEntry(entry) }" :aria-current="isPlayingEntry(entry) || containsPlayingEntry(entry) ? 'true' : undefined" @click="open(entry)"><svg v-if="entry.entryType === 'dir'" class="remote-network__folder" viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5A2.5 2.5 0 0 1 5.5 4H10l2 2.5h6.5A2.5 2.5 0 0 1 21 9v8.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5Z"/></svg><span v-else>▶</span><span class="remote-network__entry-name">{{ entry.name }}</span><small v-if="isPlayingEntry(entry)">Playing</small></button></template>
    <p v-if="loading">Loading…</p>
  </section>
</template>
