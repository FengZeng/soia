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

## [0.2.11] - 2026-08-29

### Highlights

* **Cast to DLNA and Chromecast**
  Cast the currently playing video to a DLNA receiver or Chromecast, whether the source is a local file, a DLNA/SMB/WebDAV stream, or an online video.
  **Keep Soia running while casting:** Soia relays the media stream between the source and the receiver.

* **Enhanced YouTube playback**
  Improved YouTube stream parsing and selection for more reliable online playback.

* **Audio output and passthrough**
  Choose the audio output device and enable passthrough for supported formats.

* **More playback controls**
  Adjust playback speed continuously in 0.1 increments, click the video to play or pause when Compact Mode is disabled, and fine-tune the picture with zoom and crop controls.
