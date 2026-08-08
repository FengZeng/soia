# Changelog

## [0.2.10] - 2026-08-08

### Features

- Expanded the browser-based Remote Controller with playlist browsing and playback.
- Added network source browsing and playback to the Remote Controller, including shared last-folder navigation with the desktop app.

### Improvements

- Reworked playlist and playback services around the shared core, enabling consistent desktop and Remote Controller behavior.

### Fixes

- Fixed a Windows issue where the app window could become fully transparent when switching videos or leaving playback.

## [0.2.9] - 2026-07-25

### Features

- Added a browser-based Remote Controller for local-network playback control, including play/pause, seeking, volume, playlist navigation, and audio/subtitle track selection.
- Added a QR-code entry point for the Remote Controller in the playback context menu.
- Preserve subtitle track selection when moving between episodes in the same series.

### Improvements

- Reworked the playback architecture around a shared, transport-neutral core so desktop and remote clients use a unified playback command and state flow.

### Fixes

- Fixed HDR brightness adjustment to use the correct HDR transfer handling. Thanks to [@cjohnsto-nz](https://github.com/cjohnsto-nz) for the contribution.

## [0.2.8] - 2026-07-11

### Features

- Rewrote SMB stream proxy to use async pipeline reads with server-negotiated max_read_size for improved throughput.
- Added configurable max stream resolution setting for yt-dlp.
- Added cookies-from-browser setting for yt-dlp with automatic retry on permission errors.
- Added support for YouTube `/show/` playlist URLs.
- Preferred avc1 codec over vp9 in yt-dlp format selection for broader hardware decoding compatibility.

### Fixes

- Fixed video aspect ratio distortion when resizing the window while paused on macOS.
- Fixed AppImage failing to launch on Fedora by excluding bundled libpulse from the Linux package.
- Fixed playback speed resetting when switching between videos.
- Fixed playback history not being saved before switching tracks.
- Fixed stream proxy not honoring user-registered User-Agent header.
- Fixed SMB stream backend creating duplicate connections on repeated URL rewrites.

## [0.2.7] - 2026-06-27

### Features

- Added YouTube playlist support via yt-dlp: opening a YouTube playlist URL now resolves all entries and queues them as a playback playlist.
- Added online subtitle search via OpenSubtitles and SubSource providers, with search results cache and cache-clear controls in Settings.
- Added an online subtitle search button in the subtitle track menu header to trigger a provider search from the current file context.
- Routed SMB and WebDAV media streams through a local proxy to improve compatibility and buffering reliability.

### Fixes

- Improved pointer-event detection in playback controls to prevent ghost hover states from blocking interactions.
- Decoupled context menu close from track selection so the menu dismisses at the correct moment.
- Fixed macOS window appearance to stay in sync with the active app theme (light/dark).
- Fixed Android build to source runtime libs from the correct mpv Android directory.

## [0.2.6] - 2026-06-13

### Features

- Added a macOS-only transparent glass window background for a lighter native desktop feel.
- Added a built-in Favorites Playlist that stays at the top of the playlist drawer.
- Added a playback context menu with an Add to Favorites action during playback.
- Added right-click editing actions for the URL input, including copy, cut, paste, delete, and select all.

### Fixes

- Fixed excessive playback progress disk writes by staging progress in memory and flushing it on player lifecycle events.

## [0.2.5] - 2026-06-06

### Features

- Added optional parallel WebDAV stream downloads for improved network playback buffering. This is disabled by default and can be enabled in Settings > Network.
- Added touch drag support for frameless windows so the header drag region works with touch input.
- Added fallback update endpoints to make release update checks more resilient.

### Fixes

- Fixed the Linux startup window behavior so hidden startup windows remain disabled on Linux builds.

## [0.2.4] - 2026-05-30

### Features

- Added Live playback handling for IPTV/M3U sources, including a Live indicator in playback controls and hidden seek progress for live streams.
- Added improved M3U/M3U8 parsing with support for `tvg-logo`/`logo` icons, HLS metadata detection, playlist type, target duration, and relative URL/path resolution.
- Added a playlist creation prompt before automatically creating a playback playlist from imported playlist sources, including editable playlist names.
- Added an experimental local remote-control server scaffold with HTTP and WebSocket mpv command endpoints.

### Improvements

- Improved Network browser breadcrumb navigation with available-width collapsing and an overflow menu for hidden folders.
- Preserve parsed playlist entry titles when creating playback playlists from M3U/M3U8 files.
- Refined the macOS compact top bar icon styling.
- Updated the Settings navigation icon.

### Fixes

- Fixed deep Network browser paths overflowing the header on narrow widths.
- Persist live playback state in play history and restore it when replaying history entries.
- Hide the main app window until the frontend finishes its first render to avoid a transparent startup shell.

## [0.2.3] - 2026-05-22

### Features

- Added native Windows ARM64 build support with help from [@talynone](https://github.com/talynone).
- Added playback volume controls with persisted volume state and keyboard shortcut support.

### Improvements

- Optimized external subtitle loading so subtitle files are queued in the background without blocking the track menu.
- Optimized Windows and Linux dynamic library dependencies to reduce installer/package size.
- Added manual platform build workflows for macOS, Windows, and Linux release builds.
- Updated the bundled mpv runtime used by release builds.

## [0.2.2] - 2026-05-15

### Features

- Added yt-dlp powered web stream resolution with proxy playback support.
- Added Rust-side network-adjacent source resolution for smoother next/previous playback across network media.
- Added automatic fuzzy matching for sibling subtitle files.
- Enriched audio and subtitle track menus with clearer labels and metadata.

### Fixes

- Preserved resolved yt-dlp titles in playback and history.
- Preserved IPTV playlist titles when opening playlist items.

## [0.2.1] - 2026-05-12

### Hotfixes

- Fixed Windows/Linux SMB connection issues.
- Fixed Linux video rendering issues.

## [0.2.0] - 2026-05-10

### Features

- Added persistent advanced subtitle appearance controls for font, color, size, and position.
- Added separate primary and secondary subtitle position controls for dual subtitles.
- Added SMB/Samba network share discovery, browsing, and playback support.

### Improvements

- Network discovery now streams discovered connections into the Network panel during scans.

### Fixes

- Fixed the Network panel so the active playback item is revealed in the browser.
- Polished connection modal interactions for more reliable editing.

## [0.1.9] - 2026-05-04

### Features

- Improved macOS SDR Rendering: Optimized color accuracy for a more precise visual experience on macOS.
- DLNA Auto-Play: Added support for automatically playing the next item in the queue when streaming via DLNA.
- M3U (IPTV) Parsing: You can now import M3U files to automatically parse channels and generate organized playlists.

### Fixes

- Fixed incorrect play/pause button state.
- Fixed inability to manually play the next item.

## [0.1.8] - 2026-04-30

### Features

- Added a Disable Subtitles option to force subtitles off on file load.
- Added persistent manual window state restoration.
- Added a link to the Soia subreddit in Settings.

### Improvements

- Added current playback highlighting and folder path context in the Network panel.
- Added a global video settings toggle with persisted profile support and per-file local adjustments.
- Default compact mode remains enabled to reduce accidental window decorations.
- Improved sibling Auto-Play Next behavior for local media and WebDAV media.

## [0.1.7] - 2026-04-24

### Features

- Added M3U playlist parsing support.

### Improvements

- Expanded macOS compatibility and now support macOS 13+.
- Stabilized WebDAV playback authentication and URL handling.

## [0.1.6] - 2026-04-18

### Features

- Added DLNA support for device discovery, media browsing, and stream playback.
- Added Playback Title display modes, including the option to hide the top title bar for a cleaner playback UI.

### Improvements

- Improved network browser header with Home navigation and better DLNA browsing paths.
- Redesigned the Home panel UI.
- Modularized protocol stack and unified browse/load command flow.

### Fixes

- Fixed an issue where the screen could sleep during active video playback.
- Improved robustness of the update check process and fixed update flow issues.

## [0.1.5] - 2026-04-11

### Features

- Added experimental support for Windows Portable and Linux AppImage builds.
- Added Anime-specialized mode with automatic detection and automatic shader application.
- Added per-playback shader toggle and active shader display.
- Added Factory Reset to quickly clear local data and restore default state.

### Improvements

- Improved Custom Shader settings with mode-aware behavior and cleaner controls.
- On Windows, Update now uses different behavior for Portable vs Setup installs.

## [0.1.4] - 2026-04-05

### Features

- Added dual subtitles support.
- Added custom shader support.
- Added experimental Wallpaper mode on Windows.
- Added image playback support for common formats, including playlist playback and Wallpaper mode combinations.

### Fixes

- Fixed a potential crash when quitting the app during playback.

## [0.1.2] - 2026-03-30

### Fixes

- Fixed an issue where the video window could appear transparent or fail to render on certain macOS versions (including macOS Tahoe).
- Added MoltenVK Vulkan ICD manifest setup for both development and packaged app runtime to ensure stable video output.

## [0.1.1] - 2026-03-28

First official release.

### Features

- MPV-backed local media playback.
- Picture in Picture (PiP) support on macOS and Windows.
- Borderless window mode on macOS, Windows, and Linux (Ubuntu Wayland).
- HDR and Dolby Vision playback on compatible media/hardware (not supported on Linux).
- Playback history with resume position and pin-to-top support.
- Multiple playlist support with sort modes and loop/shuffle behaviors.
- WebDAV media browsing and streaming.
- Buffering progress bar for network video playback.
- Download speed display when network video buffering is paused.
- Playback preferences (seek step, default speed, auto-play, skip intro).
- macOS platform integrations for media keys, Now Playing metadata, and artwork capture.

### Platform Notes

- Linux builds currently target Ubuntu Wayland sessions only (`X11` is not supported).
