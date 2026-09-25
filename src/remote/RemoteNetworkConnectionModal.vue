<script setup lang="ts">
import { computed, reactive, watch } from "vue";
import type { SaveNetworkConnectionDto } from "../core-client/generated/SaveNetworkConnectionDto";

const props = defineProps<{
  open: boolean;
  saving: boolean;
  error: string;
}>();

const emit = defineEmits<{
  (event: "close"): void;
  (event: "submit", connection: SaveNetworkConnectionDto): void;
}>();

const form = reactive({
  protocol: "webdav",
  label: "",
  baseUrl: "",
  host: "",
  share: "",
  group: "",
  username: "",
  password: "",
  defaultPath: "/",
});

const isSmb = computed(() => form.protocol === "smb");
const isDlna = computed(() => form.protocol === "http-dlna");

const reset = () => {
  form.protocol = "webdav";
  form.label = "";
  form.baseUrl = "";
  form.host = "";
  form.share = "";
  form.group = "";
  form.username = "";
  form.password = "";
  form.defaultPath = "/";
};

watch(() => props.open, (open) => {
  if (open) reset();
});

watch(() => form.protocol, (protocol) => {
  form.defaultPath = protocol === "http-dlna" ? "0" : "/";
});

const normalizeHttpUrl = (value: string) => {
  const trimmed = value.trim();
  if (!trimmed || /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)) return trimmed;
  return `http://${trimmed}`;
};

const autoLabel = (baseUrl: string) => {
  if (isSmb.value) {
    const host = form.host.trim();
    const share = form.share.trim().replace(/^\/+/, "");
    return share ? `${share} @ ${host}` : host;
  }
  try {
    return new URL(baseUrl).hostname || "Connection";
  } catch {
    return "Connection";
  }
};

const submit = () => {
  const host = form.host.trim();
  const share = form.share.trim().replace(/^\/+/, "");
  const baseUrl = isSmb.value
    ? (share ? `smb://${host}/${share}` : `smb://${host}/`)
    : normalizeHttpUrl(form.baseUrl);
  const prefix = isSmb.value ? "smb" : isDlna.value ? "dlna" : "webdav";
  const username = isSmb.value && form.group.trim()
    ? `${form.group.trim()};${form.username.trim()}`
    : form.username.trim();

  emit("submit", {
    id: `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    label: form.label.trim() || autoLabel(baseUrl),
    protocol: form.protocol,
    baseUrl,
    username: isDlna.value ? "" : username,
    password: isDlna.value ? "" : form.password,
    defaultPath: form.defaultPath.trim() || (isDlna.value ? "0" : "/"),
  });
};
</script>

<template>
  <div v-if="open" class="remote-network-modal__backdrop" @click="emit('close')"></div>
  <form v-if="open" class="remote-network-modal" @submit.prevent="submit">
    <div class="remote-network-modal__header">
      <div><span>NETWORK SOURCE</span><h3>Add connection</h3></div>
      <button type="button" aria-label="Close" :disabled="saving" @click="emit('close')">×</button>
    </div>
    <label><span>Protocol</span><select v-model="form.protocol"><option value="webdav">WebDAV</option><option value="smb">SMB</option><option value="http-dlna">HTTP DLNA</option></select></label>
    <label><span>Name <small>optional</small></span><input v-model="form.label" type="text" placeholder="Auto-filled from server" /></label>
    <template v-if="isSmb">
      <label><span>Host</span><input v-model="form.host" type="text" required placeholder="192.168.1.20" /></label>
      <label><span>Share <small>optional</small></span><input v-model="form.share" type="text" placeholder="media" /></label>
    </template>
    <label v-else><span>{{ isDlna ? 'Device URL' : 'Server URL' }}</span><input v-model="form.baseUrl" type="text" required :placeholder="isDlna ? 'http://192.168.1.20:8200/MediaServer' : 'http://192.168.1.20:5244/dav'" /></label>
    <template v-if="!isDlna">
      <label><span>Username <small>optional</small></span><input v-model="form.username" type="text" autocomplete="username" /></label>
      <label><span>Password <small>optional</small></span><input v-model="form.password" type="password" autocomplete="current-password" /></label>
      <label v-if="isSmb"><span>Workgroup <small>optional</small></span><input v-model="form.group" type="text" placeholder="WORKGROUP" /></label>
    </template>
    <label><span>{{ isDlna ? 'Content ID' : 'Default path' }}</span><input v-model="form.defaultPath" type="text" :placeholder="isDlna ? '0' : '/'" /></label>
    <p v-if="error" class="remote-network-modal__error">{{ error }}</p>
    <div class="remote-network-modal__actions"><button type="button" :disabled="saving" @click="emit('close')">Cancel</button><button type="submit" :disabled="saving">{{ saving ? 'Saving…' : 'Add connection' }}</button></div>
  </form>
</template>
