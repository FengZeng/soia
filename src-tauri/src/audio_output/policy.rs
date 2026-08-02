use super::model::AudioSettings;

const PASSTHROUGH_CODECS: &str = "ac3,eac3,truehd,dts-hd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MpvAudioOptions {
    pub audio_spdif: String,
    pub audio_device: String,
    pub audio_exclusive: bool,
    pub audio_channels: &'static str,
    pub passthrough: bool,
}

pub(crate) fn build_mpv_options(settings: &AudioSettings) -> MpvAudioOptions {
    let settings = settings.clone().normalized();
    let passthrough = settings.passthrough_enabled();
    MpvAudioOptions {
        audio_spdif: if passthrough {
            PASSTHROUGH_CODECS.to_string()
        } else {
            String::new()
        },
        audio_device: settings.effective_output_device().to_string(),
        audio_exclusive: cfg!(target_os = "windows") && passthrough,
        audio_channels: "auto-safe",
        passthrough,
    }
}

pub(crate) fn is_passthrough_format(format: Option<&str>) -> bool {
    format.is_some_and(|value| value.starts_with("spdif-"))
}

pub(crate) fn codec_is_enabled(settings: &AudioSettings, codec: &str) -> bool {
    if !settings.passthrough_enabled() {
        return false;
    }
    matches!(
        codec.trim().to_ascii_lowercase().as_str(),
        "ac3" | "eac3" | "truehd" | "mlp" | "dts" | "dca"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_output::model::AudioMode;

    #[test]
    fn pcm_mode_never_enables_spdif() {
        let settings = AudioSettings::default();
        assert!(build_mpv_options(&settings).audio_spdif.is_empty());
    }

    #[test]
    fn passthrough_enables_lossless_hdmi_codecs() {
        let settings = AudioSettings {
            mode: AudioMode::Passthrough,
            ..AudioSettings::default()
        };
        assert_eq!(
            build_mpv_options(&settings).audio_spdif,
            "ac3,eac3,truehd,dts-hd"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn passthrough_is_automatically_exclusive_on_windows() {
        let settings = AudioSettings {
            mode: AudioMode::Passthrough,
            ..AudioSettings::default()
        };
        assert!(build_mpv_options(&settings).audio_exclusive);
    }

    #[test]
    fn legacy_settings_names_migrate_to_the_minimal_contract() {
        let settings: AudioSettings = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "mode": "source_direct",
            "pcmDevice": "wasapi/receiver",
            "transport": "spdif",
            "codecs": {"truehd": false}
        }))
        .expect("legacy audio settings");
        assert_eq!(
            build_mpv_options(&settings).audio_spdif,
            "ac3,eac3,truehd,dts-hd"
        );
        assert_eq!(settings.output_device, "wasapi/receiver");
    }

    #[test]
    fn recognizes_only_negotiated_spdif_formats_as_passthrough() {
        assert!(is_passthrough_format(Some("spdif-truehd")));
        assert!(!is_passthrough_format(Some("s32")));
        assert!(!is_passthrough_format(None));
    }
}
