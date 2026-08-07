<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { NetworkBrowseResultDto } from "../core-client/generated/NetworkBrowseResultDto";
import type { NetworkConnectionSummaryDto } from "../core-client/generated/NetworkConnectionSummaryDto";
import { remoteCoreClient } from "./remoteCoreClient";

const connections = ref<NetworkConnectionSummaryDto[]>([]);
const selectedConnectionId = ref("");
const result = ref<NetworkBrowseResultDto | null>(null);
const loading = ref(false);
const error = ref("");
const canGoUp = computed(() => Boolean(result.value?.path && result.value.path !== "/"));

const load = async (path?: string) => {
  if (!selectedConnectionId.value) return;
  loading.value = true; error.value = "";
  try { result.value = await remoteCoreClient.browseNetworkConnection({ connectionId: selectedConnectionId.value, path: path ?? null }); }
  catch { error.value = "Network browse failed"; } finally { loading.value = false; }
};
const selectConnection = () => { result.value = null; void load(); };
const open = (entry: NetworkBrowseResultDto["entries"][number]) => {
  if (entry.entryType === "dir") { void load(entry.path); return; }
  if (entry.playbackKey) void remoteCoreClient.execute({ type: "playSource", key: entry.playbackKey, title: entry.name });
};
onMounted(async () => {
  try { connections.value = await remoteCoreClient.getNetworkConnections(); selectedConnectionId.value = connections.value[0]?.id ?? ""; if (selectedConnectionId.value) await load(); }
  catch { error.value = "Network connections are unavailable"; }
});
</script>
<template>
  <section class="remote-network"><p class="eyebrow">NETWORK</p><h2>Media sources</h2>
    <select v-model="selectedConnectionId" @change="selectConnection"><option v-for="item in connections" :key="item.id" :value="item.id">{{ item.label }} · {{ item.protocol }}</option></select>
    <p v-if="error" class="remote-playlists__error">{{ error }}</p>
    <button v-if="canGoUp" @click="load('/')">Up</button>
    <button v-for="entry in result?.entries ?? []" :key="entry.path" class="remote-network__entry" @click="open(entry)"><span>{{ entry.entryType === 'dir' ? '⌁' : '▶' }}</span>{{ entry.name }}</button>
    <p v-if="loading">Loading…</p>
  </section>
</template>
