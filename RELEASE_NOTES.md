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

## [0.2.9] - 2026-07-25

### Highlights

* **Remote Controller**
  Added a browser-based Remote Controller for controlling playback from another device on your local network. Open it from Settings or scan the QR code in the playback context menu. It supports playback, seeking, volume, previous/next controls, and audio/subtitle track controls.

  More powerful Remote Controller features are planned for future updates.

* **Refined Playback Architecture**
  Reworked the playback architecture around a shared core, making desktop and remote controls more consistent and reliable.

* **Remembered Subtitle Choices for Series**
  Your subtitle track selection is now preserved when moving between episodes in the same series.

### Fixes

* Fixed HDR brightness adjustment so it is applied correctly to HDR content. Thanks to [@cjohnsto-nz](https://github.com/cjohnsto-nz) for the contribution.
