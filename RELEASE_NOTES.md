# Release Notes

## Important (macOS)

macOS may show "Soia is damaged and can't be opened" or say it cannot verify the app is free of malware.
This happens because the app is not yet signed with an Apple Developer ID certificate, so macOS may block it on first launch.

Workaround (Recommended):
1. Right-click Soia.app
2. Click "Open"
3. Click "Open" again in the dialog

If that doesn't work, run:
```bash
sudo xattr -r -d com.apple.quarantine /Applications/Soia.app
```

You can also go to System Settings > Privacy & Security and click "Open Anyway" (it appears after a blocked launch attempt).

The app is open-source and its code is publicly available for anyone to inspect.

## [0.1.8] - 2026-04-30

This release expands playback controls, improves autoplay behavior across local and WebDAV media, and makes window state restoration more consistent.

### Highlights

- Added a **Disable Subtitles** option to force subtitles off when opening media.
- Improved **Auto-Play Next** handling for both **local files** and **WebDAV playback**.
- Added persistence for **manual window state**.
- Improved the **Network** panel with current playback highlighting and visible folder path context.
- Added a quick link to the **Soia subreddit** from Settings.

## [0.1.7] - 2026-04-24

This release improves macOS compatibility and stream playback reliability.

### Highlights

- Expanded **macOS compatibility** and now support **macOS 13+**.
- Stabilized **WebDAV playback authentication** and **URL handling**.
- Added **M3U playlist parsing** support.
