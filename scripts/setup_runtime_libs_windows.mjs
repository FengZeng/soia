#!/usr/bin/env node
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { basename, dirname, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const filename = fileURLToPath(import.meta.url);
const scriptDir = dirname(filename);
const projectRoot = dirname(scriptDir);
const runtimeDir = resolve(projectRoot, "src-tauri", "libs", "mpv");
const syncScript = resolve(scriptDir, "sync_runtime_libs.mjs");
const runtimeReleaseConfigPath = resolve(scriptDir, "runtime_libs_release_config.env");
const rawArgs = process.argv.slice(2);
const importLibraryCandidates = [
  "mpv.lib",
  "mpv-2.lib",
  "libmpv.lib",
  "libmpv-2.lib",
  "libmpv.dll.a",
  "libmpv-2.dll.a",
  "mpv.dll.a",
  "mpv-2.dll.a",
];

function printUsage() {
  console.log("Usage: node ./scripts/setup_runtime_libs_windows.mjs [local-mpv-bundle-dir]");
}

if (rawArgs.includes("--help") || rawArgs.includes("-h")) {
  printUsage();
  process.exit(0);
}

if (rawArgs.length > 1) {
  console.error("[ERROR] Too many arguments.");
  printUsage();
  process.exit(1);
}

if (rawArgs.length === 1 && rawArgs[0].startsWith("-")) {
  console.error(`[ERROR] Unsupported argument: ${rawArgs[0]}`);
  printUsage();
  process.exit(1);
}

if (rawArgs[0]) {
  process.env.MPV_LOCAL_BUNDLE_DIR = rawArgs[0];
}

if (process.platform !== "win32") {
  console.error("[ERROR] setup_runtime_libs_windows.mjs requires a Windows host.");
  process.exit(1);
}

function normalizeArch(input) {
  const value = String(input || "").toLowerCase();
  if (["arm64", "aarch64"].includes(value)) {
    return { name: "arm64", pattern: /(arm64|aarch64)/i };
  }
  if (["x64", "x86_64", "amd64", "win64"].includes(value)) {
    return { name: "x64", pattern: /(x64|x86_64|amd64|win64)/i };
  }
  if (["x86", "ia32", "i686", "win32"].includes(value)) {
    return { name: "x86", pattern: /(x86|ia32|i686|win32)/i };
  }
  return { name: value || process.arch, pattern: new RegExp(value || process.arch, "i") };
}

function loadRuntimeReleaseConfig() {
  if (!existsSync(runtimeReleaseConfigPath)) {
    return {};
  }

  const content = readFileSync(runtimeReleaseConfigPath, "utf8");
  const result = {};
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const separatorIndex = line.indexOf("=");
    if (separatorIndex <= 0) continue;
    const key = line.slice(0, separatorIndex).trim();
    const value = line.slice(separatorIndex + 1).trim();
    if (!key || !value) continue;
    result[key] = value;
  }
  return result;
}

const runtimeReleaseDefaults = loadRuntimeReleaseConfig();

function runOrExit(command, args, errorMessage) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    stdio: "inherit",
    env: process.env,
  });
  if (result.status !== 0) {
    console.error(errorMessage);
    process.exit(result.status ?? 1);
  }
}

function runOrThrow(command, args, errorMessage) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    stdio: "inherit",
    env: process.env,
  });
  if (result.status !== 0) {
    throw new Error(errorMessage);
  }
}

async function fetchReleaseJson(apiUrl) {
  const token = process.env.MPV_GITHUB_TOKEN || process.env.GITHUB_TOKEN || "";
  const headers = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  const response = await fetch(apiUrl, { headers });
  if (!response.ok) {
    throw new Error(
      `Failed to query release metadata (${response.status} ${response.statusText}): ${apiUrl}`
    );
  }
  return response.json();
}

function selectAssetUrl(releaseJson, explicitUrl) {
  if (explicitUrl) {
    return explicitUrl;
  }

  const assets = Array.isArray(releaseJson?.assets) ? releaseJson.assets : [];
  const urls = assets
    .map((asset) => asset?.browser_download_url)
    .filter((url) => typeof url === "string" && url.length > 0);
  if (urls.length === 0) {
    return "";
  }

  const rawTargetArch = process.env.MPV_TARGET_ARCH || process.arch;
  const arch = normalizeArch(rawTargetArch);
  const isWindowsAsset = (url) => /(windows|win32|win64|msvc|mingw)/i.test(url);
  const hasSupportedExtension = (url) => /\.(zip|tar\.gz|tgz|7z|dll)(\?|$)/i.test(url);

  console.log(`[INFO] Selecting Windows asset for architecture: ${arch.name}`);

  let candidate = urls.find(
    (url) => isWindowsAsset(url) && hasSupportedExtension(url) && arch.pattern.test(url)
  );
  if (candidate) return candidate;

  candidate = urls.find(
    (url) => isWindowsAsset(url) && hasSupportedExtension(url) && /universal|all/i.test(url)
  );
  if (candidate) return candidate;

  const available = urls
    .map((url) => basename(url.split("?")[0]))
    .join(", ");
  throw new Error(
    `[ERROR] No Windows ${arch.name} asset found in release. ` +
      `Provide an asset whose name matches ${arch.pattern} (or 'universal'/'all'). ` +
      `Available assets: ${available || "(none)"}`
  );
}

async function downloadAsset(assetUrl, outputFile) {
  const response = await fetch(assetUrl);
  if (!response.ok) {
    throw new Error(`Failed to download asset (${response.status} ${response.statusText}): ${assetUrl}`);
  }
  const body = await response.arrayBuffer();
  writeFileSync(outputFile, Buffer.from(body));
}

function extractAsset(assetFile, extractDir) {
  const name = basename(assetFile).toLowerCase();

  if (name.endsWith(".zip")) {
    const escapedSrc = assetFile.replace(/'/g, "''");
    const escapedDst = extractDir.replace(/'/g, "''");
    runOrThrow(
      "powershell",
      [
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        `Expand-Archive -LiteralPath '${escapedSrc}' -DestinationPath '${escapedDst}' -Force`,
      ],
      `[ERROR] Failed to extract zip asset: ${basename(assetFile)}`
    );
    return;
  }

  if (name.endsWith(".tar.gz") || name.endsWith(".tgz")) {
    runOrThrow("tar", ["-xzf", assetFile, "-C", extractDir], `[ERROR] Failed to extract tar asset.`);
    return;
  }

  if (name.endsWith(".7z")) {
    runOrThrow("7z", ["x", "-y", `-o${extractDir}`, assetFile], `[ERROR] Failed to extract 7z asset.`);
    return;
  }

  if (name.endsWith(".dll")) {
    copyFileSync(assetFile, resolve(extractDir, basename(assetFile)));
    return;
  }

  throw new Error(`[ERROR] Unsupported asset format: ${basename(assetFile)}`);
}

function listFilesRecursive(dir) {
  const stack = [dir];
  const files = [];

  while (stack.length > 0) {
    const current = stack.pop();
    const entries = readdirSync(current, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = resolve(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.isFile()) {
        files.push(fullPath);
      }
    }
  }

  return files.sort((a, b) => a.localeCompare(b));
}

function isRuntimeArtifact(filePath) {
  const lower = basename(filePath).toLowerCase();
  return lower.endsWith(".dll") || lower.endsWith(".lib") || lower.endsWith(".dll.a");
}

function ensureHasMpvDll(filePaths) {
  const hasMpvDll = filePaths.some((filePath) => {
    const lower = basename(filePath).toLowerCase();
    return lower.endsWith(".dll") && lower.includes("mpv");
  });
  if (!hasMpvDll) {
    throw new Error("[ERROR] Runtime bundle does not contain an mpv DLL.");
  }
}

function findExistingImportLibrary(outputDir) {
  for (const fileName of importLibraryCandidates) {
    const candidate = resolve(outputDir, fileName);
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return "";
}

function ensureWindowsImportLibrary(outputDir) {
  const existing = findExistingImportLibrary(outputDir);
  if (existing) return;

  throw new Error(
    "[ERROR] Missing Windows import library (.lib/.dll.a). Please provide libmpv.dll.a (or a compatible import lib) together with mpv DLLs."
  );
}

function copyArtifactsToRuntimeDir(sourceDir, outputDir) {
  mkdirSync(outputDir, { recursive: true });

  const sourceReal = realpathSync(sourceDir);
  const outputReal = realpathSync(outputDir);
  const useStaging = sourceReal === outputReal;
  const stagingDir = useStaging ? mkdtempSync(resolve(tmpdir(), "soia-mpv-stage-")) : "";
  const effectiveSource = useStaging ? stagingDir : sourceDir;

  try {
    const sourceArtifacts = listFilesRecursive(sourceDir).filter(isRuntimeArtifact);
    if (sourceArtifacts.length === 0) {
      throw new Error("[ERROR] Runtime bundle has no DLL/LIB artifacts.");
    }

    if (useStaging) {
      for (const sourcePath of sourceArtifacts) {
        copyFileSync(sourcePath, resolve(stagingDir, basename(sourcePath)));
      }
    }

    const artifacts = listFilesRecursive(effectiveSource).filter(isRuntimeArtifact);
    const runtimeArtifact =
      sourceArtifacts.find((filePath) => basename(filePath).toLowerCase().includes("mpv")) ||
      sourceArtifacts[0] ||
      "";
    const runtimeDir = runtimeArtifact ? dirname(runtimeArtifact) : sourceDir;
    const configSource = resolve(runtimeDir, "config.data");
    ensureHasMpvDll(artifacts);

    const existing = readdirSync(outputDir, { withFileTypes: true });
    for (const entry of existing) {
      if (!entry.isFile()) continue;
      const lower = entry.name.toLowerCase();
      if (lower.endsWith(".dll") || lower.endsWith(".lib") || lower.endsWith(".dll.a")) {
        rmSync(resolve(outputDir, entry.name));
      }
    }

    const seenNames = new Set();
    let copied = 0;
    for (const sourcePath of artifacts) {
      const targetName = basename(sourcePath);
      const dedupeKey = targetName.toLowerCase();
      if (seenNames.has(dedupeKey)) {
        console.warn(`[WARN] Duplicate runtime artifact ignored: ${targetName}`);
        continue;
      }
      seenNames.add(dedupeKey);
      copyFileSync(sourcePath, resolve(outputDir, targetName));
      copied += 1;
    }

    if (copied === 0) {
      throw new Error("[ERROR] No runtime artifacts copied.");
    }

    if (existsSync(configSource)) {
      copyFileSync(configSource, resolve(outputDir, "config.data"));
    } else {
      console.log("[INFO] config.data not found in runtime bundle, skipped.");
    }

    ensureWindowsImportLibrary(outputDir);
    console.log(`[INFO] Installed runtime artifacts: ${copied} -> ${outputDir}`);
  } finally {
    if (stagingDir) {
      rmSync(stagingDir, { recursive: true, force: true });
    }
  }
}

function installFromLocalBundle(outputDir) {
  const localBundleDir = process.env.MPV_LOCAL_BUNDLE_DIR || "";
  if (!localBundleDir) {
    throw new Error("[ERROR] MPV_LOCAL_BUNDLE_DIR is empty.");
  }
  if (!existsSync(localBundleDir) || !statSync(localBundleDir).isDirectory()) {
    throw new Error(`[ERROR] MPV_LOCAL_BUNDLE_DIR does not exist: ${localBundleDir}`);
  }

  console.log(`[INFO] Using local mpv bundle: ${localBundleDir}`);
  copyArtifactsToRuntimeDir(localBundleDir, outputDir);
}

function findLinkedLocalBundle() {
  const nodeModulesDir = resolve(projectRoot, "node_modules");
  if (!existsSync(nodeModulesDir)) return "";

  const candidates = readdirSync(nodeModulesDir, { withFileTypes: true })
    .filter((entry) => entry.name.toLowerCase().startsWith("libmpv-win"))
    .map((entry) => resolve(nodeModulesDir, entry.name))
    .filter((candidate) => {
      try {
        if (!statSync(candidate).isDirectory()) return false;
        const files = listFilesRecursive(candidate);
        return files.some(
          (filePath) =>
            /\.dll$/i.test(filePath) &&
            /(?:^|[\\/])(?:lib)?mpv[^\\/]*\.dll$/i.test(filePath)
        );
      } catch {
        return false;
      }
    })
    .sort((a, b) => a.localeCompare(b));

  return candidates[0] || "";
}

async function installFromRelease(outputDir) {
  const releaseRepo =
    process.env.MPV_RELEASE_REPO || runtimeReleaseDefaults.MPV_RELEASE_REPO || "";
  const releaseTag =
    process.env.MPV_RELEASE_TAG || runtimeReleaseDefaults.MPV_RELEASE_TAG || "";
  if (!releaseRepo || !releaseTag) {
    throw new Error(
      `[ERROR] Missing MPV release defaults in ${runtimeReleaseConfigPath}. Expected MPV_RELEASE_REPO and MPV_RELEASE_TAG.`
    );
  }
  const releaseApiUrl = `https://api.github.com/repos/${releaseRepo}/releases/tags/${releaseTag}`;

  console.log(`[INFO] Resolving mpv release asset from ${releaseRepo}@${releaseTag}...`);
  const releaseJson = await fetchReleaseJson(releaseApiUrl);
  const assetUrl = selectAssetUrl(releaseJson, process.env.MPV_RELEASE_ASSET_URL || "");

  if (!assetUrl) {
    throw new Error(
      `[ERROR] Cannot find a downloadable Windows asset in release ${releaseTag}.`
    );
  }

  const tmpRoot = mkdtempSync(resolve(tmpdir(), "soia-mpv-"));
  const extractDir = resolve(tmpRoot, "extract");
  mkdirSync(extractDir, { recursive: true });

  try {
    const assetName = basename(assetUrl.split("?")[0]);
    const assetFile = resolve(tmpRoot, assetName);

    console.log(`[INFO] Downloading asset: ${assetName}`);
    await downloadAsset(assetUrl, assetFile);

    console.log("[INFO] Extracting asset...");
    extractAsset(assetFile, extractDir);

    console.log(`[INFO] Installing runtime artifacts to: ${outputDir}`);
    copyArtifactsToRuntimeDir(extractDir, outputDir);
  } finally {
    rmSync(tmpRoot, { recursive: true, force: true });
  }
}

function syncRuntimeManifest() {
  if (!existsSync(syncScript)) {
    console.error(`[ERROR] Missing script: ${syncScript}`);
    process.exit(1);
  }
  runOrExit(
    process.execPath,
    [syncScript, "--platform", "win32"],
    "[ERROR] Failed to sync runtime manifest for win32."
  );
}

async function main() {
  mkdirSync(runtimeDir, { recursive: true });

  if (!process.env.MPV_LOCAL_BUNDLE_DIR) {
    const linkedBundle = findLinkedLocalBundle();
    if (linkedBundle) {
      process.env.MPV_LOCAL_BUNDLE_DIR = linkedBundle;
      console.log(`[INFO] Using linked local mpv bundle: ${linkedBundle}`);
    }
  }

  if (process.env.MPV_LOCAL_BUNDLE_DIR) {
    installFromLocalBundle(runtimeDir);
  } else {
    await installFromRelease(runtimeDir);
  }

  syncRuntimeManifest();
  console.log(`[INFO] Windows runtime setup completed: ${runtimeDir}`);
}

main().catch((error) => {
  console.error(error.message || String(error));
  process.exit(1);
});
