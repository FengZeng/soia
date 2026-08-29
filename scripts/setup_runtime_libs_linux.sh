#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RUNTIME_RELEASE_CONFIG_FILE="$SCRIPT_DIR/runtime_libs_release_config.env"

LIBS_DIR="$PROJECT_ROOT/src-tauri/libs"
LOCAL_BUNDLE_ARG="${1:-}"

if [[ $# -gt 1 ]]; then
  echo "[ERROR] Too many arguments."
  echo "Usage: $0 [local-mpv-bundle-dir]"
  exit 1
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "[ERROR] setup_runtime_libs_linux.sh requires a Linux host."
  exit 1
fi

if [[ -n "$LOCAL_BUNDLE_ARG" ]]; then
  MPV_LOCAL_BUNDLE_DIR="$LOCAL_BUNDLE_ARG"
fi

echo "📂 Project Root Directory: $PROJECT_ROOT"
mkdir -p "$LIBS_DIR"

get_runtime_release_config_value() {
  local key="$1"
  local fallback="$2"

  if [[ -f "$RUNTIME_RELEASE_CONFIG_FILE" ]]; then
    local value
    value="$(awk -v target="$key" '
      {
        line = $0
        sub(/^[[:space:]]+/, "", line)
        if (line == "" || line ~ /^#/) {
          next
        }

        pos = index(line, "=")
        if (pos == 0) {
          next
        }

        k = substr(line, 1, pos - 1)
        v = substr(line, pos + 1)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", k)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", v)

        if (k == target) {
          print v
          exit
        }
      }
    ' "$RUNTIME_RELEASE_CONFIG_FILE")"

    if [[ -n "$value" ]]; then
      echo "$value"
      return
    fi
  fi

  echo "$fallback"
}

fetch_release_json() {
  local api_url="$1"
  local auth_token="${MPV_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}"
  local -a curl_args

  curl_args=(
    -fsSL
    -H "Accept: application/vnd.github+json"
    -H "X-GitHub-Api-Version: 2022-11-28"
  )

  if [[ -n "$auth_token" ]]; then
    curl_args+=(-H "Authorization: Bearer $auth_token")
  fi

  if ! curl "${curl_args[@]}" "$api_url"; then
    echo "[ERROR] Failed to query release metadata: $api_url" >&2
    echo "[ERROR] If this happens in CI, set MPV_GITHUB_TOKEN or GITHUB_TOKEN to avoid anonymous GitHub API limits." >&2
    return 1
  fi
}

select_asset_url() {
  local release_json="$1"
  local explicit_url="${2:-}"
  local asset_urls
  local candidate
  local arch
  local arch_pattern
  local raw_target_arch

  if [[ -n "$explicit_url" ]]; then
    echo "$explicit_url"
    return
  fi

  asset_urls="$(printf '%s\n' "$release_json" | awk -F\" '/browser_download_url/ {print $4}')"
  if [[ -z "$asset_urls" ]]; then
    return
  fi

  raw_target_arch="${MPV_TARGET_ARCH:-}"
  if [[ -n "$raw_target_arch" ]]; then
    arch="$(printf '%s' "$raw_target_arch" | tr '[:upper:]' '[:lower:]')"
  else
    arch="$(uname -m)"
  fi

  case "$arch" in
    arm64|aarch64)
      arch='arm64'
      arch_pattern='(arm64|aarch64)'
      ;;
    x86_64|amd64|x64|intel)
      arch='x86_64'
      arch_pattern='(x86_64|amd64|x64|intel)'
      ;;
    *)
      arch_pattern="(${arch})"
      ;;
  esac

  if [[ -n "$raw_target_arch" ]]; then
    echo "🧭 Selecting Linux asset for architecture: $arch (from MPV_TARGET_ARCH=$raw_target_arch)" >&2
  else
    echo "🧭 Selecting Linux asset for architecture: $arch (from uname -m)" >&2
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(linux|gnu|musl).*(tar\.gz|tgz|zip|so(\..+)?)$' \
    | grep -Ei "$arch_pattern" \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(linux|gnu|musl).*(tar\.gz|tgz|zip|so(\..+)?)$' \
    | grep -Ei 'universal|all' \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(linux|gnu|musl).*(tar\.gz|tgz|zip|so(\..+)?)$' \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(tar\.gz|tgz|zip|so(\..+)?)$' \
    | grep -Ei "$arch_pattern" \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi

  candidate="$(printf '%s\n' "$asset_urls" \
    | grep -Ei '(tar\.gz|tgz|zip|so(\..+)?)$' \
    | head -n 1 || true)"
  if [[ -n "$candidate" ]]; then
    echo "$candidate"
    return
  fi
}

extract_asset() {
  local asset_file="$1"
  local extract_dir="$2"
  local file_name

  file_name="$(basename "$asset_file")"

  case "$file_name" in
    *.tar.gz|*.tgz)
      tar -xzf "$asset_file" -C "$extract_dir"
      ;;
    *.zip)
      unzip -q "$asset_file" -d "$extract_dir"
      ;;
    *.so|*.so.*)
      cp -f "$asset_file" "$extract_dir/"
      ;;
    *)
      echo "[ERROR] Unsupported asset format: $file_name"
      exit 1
      ;;
  esac
}

copy_shared_objects_to_dir() {
  local extract_dir="$1"
  local output_dir="$2"
  local canonical_name="$3"
  local link_name="$4"
  local source_dir="$extract_dir"
  local source_real
  local output_real
  local staging_dir=""
  local so_count
  local root_lib

  mkdir -p "$output_dir"

  source_real="$(cd "$extract_dir" && pwd -P)"
  output_real="$(cd "$output_dir" && pwd -P)"

  if [[ "$source_real" == "$output_real" ]]; then
    staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/soia-mpv-stage.XXXXXX")"
    while IFS= read -r so_file; do
      local so_name
      so_name="$(basename "$so_file")"
      if is_excluded_runtime_so "$so_name"; then
        continue
      fi
      cp -f "$so_file" "$staging_dir/$(basename "$so_file")"
    done < <(find "$extract_dir" -type f -name '*.so*')
    source_dir="$staging_dir"
  fi

  so_count="$(
    find "$source_dir" -type f -name '*.so*' \
      | while IFS= read -r so_file; do
          so_name="$(basename "$so_file")"
          if is_excluded_runtime_so "$so_name"; then
            continue
          fi
          echo "$so_file"
        done \
      | wc -l \
      | tr -d ' '
  )"
  if [[ "$so_count" -eq 0 ]]; then
    echo "[ERROR] Release asset does not contain usable .so files after exclusions."
    rm -rf "$staging_dir" 2>/dev/null || true
    exit 1
  fi

  find "$output_dir" -maxdepth 1 \( -type f -o -type l \) -name '*.so*' -delete

  while IFS= read -r so_file; do
    so_name="$(basename "$so_file")"
    if is_excluded_runtime_so "$so_name"; then
      continue
    fi
    cp -f "$so_file" "$output_dir/$(basename "$so_file")"
  done < <(find "$source_dir" -type f -name '*.so*')

  root_lib="$output_dir/$canonical_name"
  if [[ ! -f "$root_lib" ]]; then
    root_lib="$(find "$output_dir" -maxdepth 1 -type f -name 'libmpv.so*' | head -n 1 || true)"
    if [[ -z "$root_lib" ]]; then
      echo "[ERROR] Cannot find expected shared library in source asset."
      rm -rf "$staging_dir" 2>/dev/null || true
      exit 1
    fi
    cp -f "$root_lib" "$output_dir/$canonical_name"
  fi

  ln -sf "$canonical_name" "$output_dir/$link_name"

  # The Rust FFI links against the conventional development names (-lavformat,
  # -lavcodec, ...), while release bundles usually contain only versioned
  # runtime files such as libavformat.so.62.12.100. Add local linker aliases
  # without changing the versioned files used at runtime.
  local ffmpeg_lib
  local ffmpeg_target
  for ffmpeg_lib in avformat avcodec avutil swscale; do
    ffmpeg_target="$(find "$output_dir" -maxdepth 1 -type f -name "lib${ffmpeg_lib}.so.*" | sort | head -n 1 || true)"
    if [[ -n "$ffmpeg_target" && ! -e "$output_dir/lib${ffmpeg_lib}.so" ]]; then
      ln -sf "$(basename "$ffmpeg_target")" "$output_dir/lib${ffmpeg_lib}.so"
    fi
  done

  rm -rf "$staging_dir" 2>/dev/null || true
}

copy_config_data_to_dir() {
  local source_dir="$1"
  local output_dir="$2"
  local config_source="$source_dir/config.data"

  if [[ ! -f "$config_source" ]]; then
    echo "[INFO] config.data not found in runtime bundle, skipped."
    return
  fi

  cp -f "$config_source" "$output_dir/config.data"
}

resolve_runtime_dir_from_shared_objects() {
  local source_dir="$1"
  local runtime_so=""

  runtime_so="$(find "$source_dir" -type f -name '*.so*' | head -n 1 || true)"
  if [[ -z "$runtime_so" ]]; then
    echo "$source_dir"
    return
  fi

  dirname "$runtime_so"
}

is_excluded_runtime_so() {
  local so_name="$1"
  case "$so_name" in
    libEGL.so*|libGL.so*|libGLX.so*|libGLdispatch.so*|libGLES*.so*|libgbm.so*|libdrm.so*|libwayland*.so*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

sync_tauri_linux_manifest() {
  local sync_script="$PROJECT_ROOT/scripts/sync_runtime_libs.mjs"

  if [[ ! -f "$sync_script" ]]; then
    echo "[ERROR] Missing script: $sync_script"
    exit 1
  fi

  if ! command -v node >/dev/null 2>&1; then
    echo "[ERROR] node command not found. Cannot sync Linux runtime manifest."
    exit 1
  fi

  node "$sync_script" --platform linux
}

install_mpv_from_local_bundle() {
  local output_dir="$1"
  local local_bundle_dir="${MPV_LOCAL_BUNDLE_DIR:-}"

  if [[ -z "$local_bundle_dir" ]]; then
    echo "[ERROR] MPV_LOCAL_BUNDLE_DIR is empty."
    exit 1
  fi
  if [[ ! -d "$local_bundle_dir" ]]; then
    echo "[ERROR] MPV_LOCAL_BUNDLE_DIR does not exist: $local_bundle_dir"
    exit 1
  fi

  echo "📦 Using local mpv bundle: $local_bundle_dir"
  local runtime_dir
  runtime_dir="$(resolve_runtime_dir_from_shared_objects "$local_bundle_dir")"
  copy_shared_objects_to_dir "$local_bundle_dir" "$output_dir" "libmpv.so.2" "libmpv.so"
  copy_config_data_to_dir "$runtime_dir" "$output_dir"
  sync_tauri_linux_manifest
  echo "✅ mpv local bundle installed: $output_dir"
}

download_mpv() {
  local output_dir="$LIBS_DIR/mpv"
  local default_release_repo
  local default_release_tag
  default_release_repo="$(get_runtime_release_config_value "MPV_RELEASE_REPO" "")"
  default_release_tag="$(get_runtime_release_config_value "MPV_RELEASE_TAG" "")"
  if [[ -z "$default_release_repo" || -z "$default_release_tag" ]]; then
    echo "[ERROR] Missing MPV release defaults in $RUNTIME_RELEASE_CONFIG_FILE"
    echo "[ERROR] Expected keys: MPV_RELEASE_REPO and MPV_RELEASE_TAG"
    exit 1
  fi
  local release_repo="${MPV_RELEASE_REPO:-$default_release_repo}"
  local release_tag="${MPV_RELEASE_TAG:-$default_release_tag}"
  local release_api_url="https://api.github.com/repos/${release_repo}/releases/tags/${release_tag}"
  local release_json
  local asset_url
  local tmp_dir
  local asset_name
  local asset_file
  local extract_dir

  if [[ -n "${MPV_LOCAL_BUNDLE_DIR:-}" ]]; then
    install_mpv_from_local_bundle "$output_dir"
    return
  fi

  echo "📥 Resolving mpv release asset from ${release_repo}@${release_tag}..."
  release_json="$(fetch_release_json "$release_api_url")"
  asset_url="$(select_asset_url "$release_json" "${MPV_RELEASE_ASSET_URL:-}")"

  if [[ -z "$asset_url" ]]; then
    echo "[ERROR] Cannot find a downloadable Linux asset in release ${release_tag}."
    echo "You can override via MPV_RELEASE_ASSET_URL=<direct-download-url> pnpm setup:libs"
    exit 1
  fi

  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/soia-mpv.XXXXXX")"
  trap "rm -rf '$tmp_dir'" EXIT

  asset_name="$(basename "${asset_url%%\?*}")"
  asset_file="$tmp_dir/$asset_name"
  extract_dir="$tmp_dir/extract"

  mkdir -p "$extract_dir"

  echo "⬇️ Downloading asset: $asset_name"
  curl -fL --retry 3 --retry-delay 1 -o "$asset_file" "$asset_url"

  echo "📦 Extracting asset..."
  extract_asset "$asset_file" "$extract_dir"

  echo "🧩 Installing shared objects to $output_dir"
  local runtime_dir
  runtime_dir="$(resolve_runtime_dir_from_shared_objects "$extract_dir")"
  copy_shared_objects_to_dir "$extract_dir" "$output_dir" "libmpv.so.2" "libmpv.so"
  copy_config_data_to_dir "$runtime_dir" "$output_dir"
  sync_tauri_linux_manifest
  echo "✅ mpv completed: $output_dir"
}

download_all() {
  download_mpv
}

download_all
