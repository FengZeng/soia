#!/usr/bin/env node
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(filename);
const projectRoot = dirname(scriptDir);
const tauriRuntimeMacConfigPath = resolve(projectRoot, "src-tauri", "tauri.runtime.macos.json");
const runtimeManifestDir = resolve(projectRoot, "src-tauri", "libs");
const mpvRuntimeLibsDir = resolve(runtimeManifestDir, "mpv");

const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
const platformArgIndex = args.findIndex((arg) => arg === "--platform");
const platformArg = platformArgIndex >= 0 ? args[platformArgIndex + 1] : undefined;
const targetArgIndex = args.findIndex((arg) => arg === "--target");
const targetArg = targetArgIndex >= 0 ? args[targetArgIndex + 1] : undefined;
const targetPlatform = platformArg ?? process.platform;

const supportedPlatforms = new Set(["darwin", "linux", "win32"]);
if (!supportedPlatforms.has(targetPlatform)) {
  console.error(`[ERROR] Unsupported platform: ${targetPlatform}`);
  process.exit(1);
}

function resolveTargetTriple(platform) {
  const fromArgs = targetArg && !targetArg.startsWith("-") ? targetArg : "";
  const fromEnv = process.env.TARGET || process.env.CARGO_BUILD_TARGET || "";
  if (fromArgs) return fromArgs;
  if (fromEnv) return fromEnv;

  const arch = (process.env.MPV_TARGET_ARCH || process.arch || "").toLowerCase();
  if (platform === "darwin") {
    if (arch === "arm64" || arch === "aarch64") return "aarch64-apple-darwin";
    return "x86_64-apple-darwin";
  }
  if (platform === "linux") {
    if (arch === "arm64" || arch === "aarch64") return "aarch64-unknown-linux-gnu";
    return "x86_64-unknown-linux-gnu";
  }
  if (platform === "win32") {
    if (arch === "arm64" || arch === "aarch64") return "aarch64-pc-windows-msvc";
    if (arch === "ia32" || arch === "x86" || arch === "i686") return "i686-pc-windows-msvc";
    return "x86_64-pc-windows-msvc";
  }
  return "";
}

const targetTriple = resolveTargetTriple(targetPlatform);

function listMpvEntries(platform) {
  if (!existsSync(mpvRuntimeLibsDir)) {
    throw new Error(`runtime libs directory not found: ${mpvRuntimeLibsDir}`);
  }

  const entries = readdirSync(mpvRuntimeLibsDir, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .filter((entry) => {
      if (platform === "darwin") return entry.name.endsWith(".dylib");
      if (platform === "linux") {
        if (!/\.so(\..+)?$/.test(entry.name)) return false;
        return !isExcludedLinuxRuntimeLib(entry.name);
      }
      return entry.name.toLowerCase().endsWith(".dll");
    })
    .map((entry) => `libs/mpv/${entry.name}`)
    .sort((a, b) => a.localeCompare(b));

  if (entries.length === 0) {
    throw new Error(`no runtime libraries found for ${platform} under: ${mpvRuntimeLibsDir}`);
  }

  return entries;
}

function isExcludedLinuxRuntimeLib(name) {
  return [
    /^libEGL\.so(\..+)?$/,
    /^libGL\.so(\..+)?$/,
    /^libGLX\.so(\..+)?$/,
    /^libGLdispatch\.so(\..+)?$/,
    /^libGLES.*\.so(\..+)?$/,
    /^libgbm\.so(\..+)?$/,
    /^libdrm\.so(\..+)?$/,
    /^libwayland.*\.so(\..+)?$/,
  ].some((pattern) => pattern.test(name));
}

function listRuntimeEntries(platform) {
  return listMpvEntries(platform).sort((a, b) => a.localeCompare(b));
}

function sameStringArray(a, b) {
  if (a.length !== b.length) return false;
  return a.every((value, index) => value === b[index]);
}

function syncDarwinFrameworks(entries) {
  let current = [];
  if (existsSync(tauriRuntimeMacConfigPath)) {
    const raw = readFileSync(tauriRuntimeMacConfigPath, "utf8");
    const config = JSON.parse(raw);
    current =
      config &&
      config.bundle &&
      config.bundle.macOS &&
      Array.isArray(config.bundle.macOS.frameworks)
        ? [...config.bundle.macOS.frameworks].sort((a, b) => a.localeCompare(b))
        : [];
  }

  const changed = !sameStringArray(current, entries);

  if (checkOnly) {
    if (changed) {
      console.error("[ERROR] tauri macOS runtime config is out of sync.");
      console.error("[ERROR] Run: pnpm setup:libs");
      process.exit(1);
    }
    console.log(`[INFO] macOS runtime config in sync (${entries.length})`);
    return;
  }

  if (!changed) {
    console.log(`[INFO] macOS runtime config already in sync (${entries.length})`);
    return;
  }

  const runtimeConfig = {
    bundle: {
      macOS: {
        frameworks: entries,
      },
    },
  };
  writeFileSync(tauriRuntimeMacConfigPath, `${JSON.stringify(runtimeConfig, null, 2)}\n`, "utf8");
  console.log(
    `[INFO] Synced macOS runtime config (${entries.length}): ${tauriRuntimeMacConfigPath}`
  );
}

function manifestPathFor(platform) {
  return resolve(runtimeManifestDir, `runtime-libs.${platform}.json`);
}

function syncRuntimeManifest(platform, entries) {
  const manifestPath = manifestPathFor(platform);
  const nextManifest = {
    runtimes: ["mpv"],
    platform,
    target: targetTriple,
    entries,
  };

  let currentEntries = [];
  let currentTarget = "";
  if (existsSync(manifestPath)) {
    const raw = readFileSync(manifestPath, "utf8");
    const parsed = JSON.parse(raw);
    currentEntries = Array.isArray(parsed.entries)
      ? [...parsed.entries].sort((a, b) => a.localeCompare(b))
      : [];
    currentTarget = typeof parsed.target === "string" ? parsed.target : "";
  }

  const changed = currentTarget !== targetTriple || !sameStringArray(currentEntries, entries);

  if (checkOnly) {
    if (changed) {
      console.error(`[ERROR] Runtime manifest is out of sync: ${manifestPath}`);
      console.error(
        `[ERROR] Run: node ./scripts/sync_runtime_libs.mjs --platform ${platform} --target ${targetTriple}`
      );
      process.exit(1);
    }
    console.log(`[INFO] Runtime manifest in sync (${platform}, ${entries.length})`);
    return;
  }

  if (!changed) {
    console.log(`[INFO] Runtime manifest already in sync (${platform}, ${entries.length})`);
    return;
  }

  writeFileSync(manifestPath, `${JSON.stringify(nextManifest, null, 2)}\n`, "utf8");
  console.log(`[INFO] Synced runtime manifest: ${manifestPath}`);
}

function main() {
  const entries = listRuntimeEntries(targetPlatform);
  if (targetPlatform === "darwin") {
    syncDarwinFrameworks(entries);
  } else {
    syncRuntimeManifest(targetPlatform, entries);
  }
}

try {
  main();
} catch (error) {
  console.error(`[ERROR] ${error.message}`);
  process.exit(1);
}
