mod model;
mod policy;
mod runtime;

#[cfg(test)]
pub(crate) use model::AudioMode;
pub(crate) use model::{AudioDevice, AudioOutputStatus, AudioSettings};
pub(crate) use policy::{build_mpv_options, codec_is_enabled};
pub(crate) use runtime::AudioOutputRuntime;

use crate::mpv::MpvHandle;
use tauri::{AppHandle, Emitter};

pub(crate) const AUDIO_DEVICES_EVENT: &str = "soia:audio-devices";
pub(crate) const AUDIO_OUTPUT_STATUS_EVENT: &str = "soia:audio-output-status";

pub(crate) fn apply_to_mpv(
    mpv: &MpvHandle,
    settings: &AudioSettings,
    reset_speed: bool,
) -> Result<(), String> {
    let options = build_mpv_options(settings);
    set_property(mpv, "audio-fallback-to-null", "yes")?;
    set_property(mpv, "audio-spdif", &options.audio_spdif)?;
    set_property(mpv, "audio-device", &options.audio_device)?;
    set_property(
        mpv,
        "audio-exclusive",
        if options.audio_exclusive { "yes" } else { "no" },
    )?;
    set_property(mpv, "audio-channels", options.audio_channels)?;
    set_property(mpv, "ad-lavc-downmix", "yes")?;
    set_property(mpv, "audio-pitch-correction", "yes")?;

    if options.passthrough {
        set_property(mpv, "af", "")?;
        if reset_speed {
            command(mpv, &["set", "speed", "1"])?;
        }
    }
    Ok(())
}

pub(crate) fn emit_devices(app: &AppHandle, devices: Vec<AudioDevice>) {
    if let Err(error) = app.emit(AUDIO_DEVICES_EVENT, devices) {
        log::warn!("failed to emit audio device update: {error}");
    }
}

pub(crate) fn emit_status(app: &AppHandle, status: AudioOutputStatus) {
    if let Err(error) = app.emit(AUDIO_OUTPUT_STATUS_EVENT, status) {
        log::warn!("failed to emit audio output status: {error}");
    }
}

fn set_property(mpv: &MpvHandle, name: &str, value: &str) -> Result<(), String> {
    command(mpv, &["set", name, value])
}

fn command(mpv: &MpvHandle, args: &[&str]) -> Result<(), String> {
    let result = mpv.command(args);
    if result >= 0 {
        Ok(())
    } else {
        Err(format!(
            "mpv audio command {:?} failed with error {result}",
            args
        ))
    }
}
