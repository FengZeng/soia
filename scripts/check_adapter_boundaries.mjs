import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceExtensions = new Set([".ts", ".vue"]);
const violations = [];

const collectFiles = async (relativeDirectory) => {
  const absoluteDirectory = path.join(projectRoot, relativeDirectory);
  try {
    const entries = await readdir(absoluteDirectory, { withFileTypes: true });
    const nested = await Promise.all(entries.map(async (entry) => {
      const relativePath = path.join(relativeDirectory, entry.name);
      if (entry.isDirectory()) return collectFiles(relativePath);
      return sourceExtensions.has(path.extname(entry.name)) ? [relativePath] : [];
    }));
    return nested.flat();
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return [];
    }
    throw error;
  }
};

const readSource = async (relativePath) =>
  readFile(path.join(projectRoot, relativePath), "utf8");

const assertNoMatch = (relativePath, source, pattern, message) => {
  if (pattern.test(source)) violations.push(`${relativePath}: ${message}`);
};

const tauriAdapterFiles = new Set([
  "src/core-client/tauriCoreClient.ts",
  "src/core-client/tauriPlaylistClient.ts",
  "src/core-client/tauriPlaylistSourceClient.ts",
  "src/core-client/tauriCastingClient.ts",
]);

for (const relativePath of await collectFiles("src/core-client")) {
  const source = await readSource(relativePath);
  if (/@tauri-apps\//.test(source) && !tauriAdapterFiles.has(relativePath)) {
    violations.push(`${relativePath}: Tauri imports belong in an explicit Tauri client adapter`);
  }
  if (/\bnew\s+WebSocket\b/.test(source) && relativePath !== "src/core-client/webSocketCoreClient.ts") {
    violations.push(`${relativePath}: WebSocket construction belongs in WebSocketCoreClient`);
  }
}

for (const relativePath of await collectFiles("src/remote")) {
  const source = await readSource(relativePath);
  if (relativePath === "src/remote/remoteCoreClient.ts") continue;
  assertNoMatch(relativePath, source, /\bnew\s+WebSocket\b/, "Remote UI must not construct WebSocket instances");
  assertNoMatch(relativePath, source, /\.send\s*\(/, "Remote UI must not send WebSocket messages directly");
  assertNoMatch(relativePath, source, /core-client\/webSocketProtocol/, "Remote UI must use client interfaces, not wire protocol helpers");
  assertNoMatch(relativePath, source, /core-client\/webSocketCoreClient/, "Remote UI must use the composition-root client");
}

for (const relativePath of await collectFiles("src/features")) {
  const source = await readSource(relativePath);
  assertNoMatch(relativePath, source, /@tauri-apps\//, "Shared features must not import Tauri APIs");
  assertNoMatch(relativePath, source, /\bnew\s+WebSocket\b|\.send\s*\(/, "Shared features must not use WebSocket transport directly");
  assertNoMatch(relativePath, source, /\bwindow\.|\bdocument\./, "Shared features must not depend on the DOM");
}

assert.deepEqual(violations, [], `Adapter boundary violations:\n${violations.join("\n")}`);
