<h1 align="center">
  <img src="./src-tauri/icons/icon.png" alt="Soia" width="128" />
  <br>
  Soia
  <br>
</h1>

<p align="center">
🎬 HDR & Dolby Vision · 🌐 WebDAV + DLNA + SMB Streaming · 🖥 Cross-platform
</p>

<p align="center">
<b><a href="https://github.com/FengZeng/soia/releases">⬇️ Download Latest Release</a> · <a href="https://github.com/FengZeng/soia/issues">🐞 Report a Bug</a></b>
</p>

![Soia App Preview](https://github.com/user-attachments/assets/9896ae38-d082-413e-8a01-bdb28e687bf7)
> A modern mpv frontend focused on performance and clean design.

**Soia** is a high-performance video player built on mpv, designed for smooth playback of everything from local Dolby Vision content to remote WebDAV, DLNA, and SMB streams — all in one fast, elegant, cross-platform experience.

## ✨ Key Features

### High-Performance Playback

- **mpv-powered playback** with hardware acceleration (4K, HDR, Dolby Vision*)
- Picture in Picture (PiP) on macOS and Windows
- Dual subtitles for bilingual viewing
- Fuzzy subtitle matching for both local and network media
- Online subtitle search via OpenSubtitles and SubSource
- Advanced subtitle appearance controls for font, color, size, and position
- Custom shaders for high-quality scaling and rendering
- Anime mode with auto-detection and shader auto-apply

<sub style="padding-left: 2em;">*Dolby Vision is not currently supported on Linux*</sub>

### Streaming & Remote Media

- WebDAV browsing and streaming
- DLNA and SMB/Samba discovery, browsing, and playback
- M3U (IPTV) parsing and playback
- Smart buffering with real-time speed indicators
- Resume playback with history tracking

### Native Experience

- Native media keys and Now Playing integration (macOS)
- Borderless window across macOS, Windows, and Linux (Wayland)
- Experimental Wallpaper Mode (Windows)
- Flexible playback preferences (speed, seek, auto-play, skip intro)

## Install

Download from the [release page](https://github.com/FengZeng/soia/releases).

On macOS, you can install it with Homebrew:

```bash
brew tap FengZeng/soia
brew install --cask soia
```

On Windows, you can install it with WinGet:

```powershell
winget install soia
```

Or you can build it yourself. Support macOS 13+, Windows, and Linux.
Linux builds have been tested on Ubuntu and Fedora Wayland sessions (`X11` is not currently supported).

## FAQ

Q: macOS says "Soia is damaged and can't be opened" or cannot verify it is free of malware.

A: This happens because the app is not yet signed with an Apple Developer ID certificate, so macOS may block it on first launch.

Easy fix (recommended):
1. Right-click Soia.app
2. Click "Open"
3. Click "Open" again in the dialog

If that doesn't work, run:
```bash
sudo xattr -r -d com.apple.quarantine /Applications/Soia.app
```

You can also go to System Settings -> Privacy & Security and click "Open Anyway" (it appears after a blocked launch attempt).

The app is open-source and its code is publicly available for anyone to inspect.

## Tech Stack

- Frontend: Vue 3 + TypeScript + Vite
- App runtime: Tauri v2
- Backend: Rust
- Playback engine: libmpv
- Persistence: SQLite (`media.db`) + JSON state files

## Getting Started

1. Prerequisites

   Ensure you have the following installed:

   - Node.js 18+ & pnpm 10.x
   - Rust (stable toolchain)
   - Tauri build prerequisites for your specific platform

2. Setup

   ```bash
   # Automatically prepares runtime libs
   pnpm install
   ```

3. Run

   ```bash
   # Launches with auto-injected library paths
   pnpm tauri dev
   ```
## Build and Bundle

Common release build commands:

```bash
pnpm bundle:mac:release
pnpm bundle:linux:release
pnpm bundle:win:release
```

## Keyboard Shortcuts

- `Space`: play/pause
- `Left / Right`: seek backward/forward (step from settings)
- `I`: toggle playback info panel
- Double-click video area: toggle fullscreen
- Middle-click during playback: hide or show controls; mouse movement stays suppressed for 3 seconds after hiding

## Data Storage

App data is stored in Tauri's local app data directory and includes:

- `media.db`: default playlist entries, playback history, and local installation/device/sync metadata
- `state.json`: UI state and settings (for example active panel, multiple-playlist metadata, and preferences)
- `network_connections.json`: saved network connections
- `thumbnails/`: captured artwork for Now Playing

## Security Note

Saved network credentials are currently persisted in `network_connections.json` as plain text. Avoid using sensitive production credentials on shared machines.

## Troubleshooting

- If Linux build fails with `glib-2.0` / `gdk-3.0` / `*.pc` not found, install the Ubuntu deps:

```bash
sudo apt update
sudo apt install -y \
    build-essential \
    curl \
    wget \
    file \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    pkg-config \
    libwebkit2gtk-4.1-dev
```

- Linux runtime note: current bundle targets Ubuntu Wayland sessions only; launching under pure `X11` is not supported.

- If build fails with `Cannot find libmpv`, run:

```bash
pnpm setup:libs
```

- If `pnpm setup:libs` fails, confirm release access to:
  - `https://github.com/FengZeng/mpv/releases/tag/v0.41.0-r15`
  - or set `MPV_RELEASE_ASSET_URL` to a direct asset URL and retry.

- If Linux/Windows bundle scripts report missing runtime manifest, generate it on the target platform:

```bash
pnpm sync:runtime:linux
pnpm sync:runtime:win
```

- If you have a local bundled `mpv + dependencies` directory for dev testing, use:

```bash
pnpm setup:libs /absolute/path/to/mpv-bundle
```

---

## Author & Maintainer

**Soia** is an independent project developed and maintained by **[@FengZeng](https://github.com/FengZeng)**.

While development is driven independently, issues and feedback are actively reviewed and addressed whenever possible.

If you find Soia useful, consider giving it a ⭐ Star — it helps the project grow and reach more users.

## License

This project is licensed under the GNU General Public License v3.0 only (`GPL-3.0-only`).
See [`LICENSE`](LICENSE) for the full text.
