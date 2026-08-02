use super::model::{
    ActiveAudioMode, AudioDevice, AudioOutputIssue, AudioOutputStatus, AudioSettings,
};
use super::policy::{codec_is_enabled, is_passthrough_format};
use serde_json::Value;
use std::sync::Mutex;

#[derive(Debug, Clone)]
struct RuntimeState {
    settings: AudioSettings,
    devices: Vec<AudioDevice>,
    status: AudioOutputStatus,
    speed: f64,
}

pub struct AudioOutputRuntime {
    inner: Mutex<RuntimeState>,
}

impl AudioOutputRuntime {
    pub fn new(settings: AudioSettings) -> Self {
        let settings = settings.normalized();
        let status = AudioOutputStatus {
            requested_mode: settings.mode,
            selected_device: settings.effective_output_device().to_string(),
            ..AudioOutputStatus::default()
        };
        Self {
            inner: Mutex::new(RuntimeState {
                settings,
                devices: Vec::new(),
                status,
                speed: 1.0,
            }),
        }
    }

    pub fn settings(&self) -> AudioSettings {
        self.inner
            .lock()
            .map(|state| state.settings.clone())
            .unwrap_or_default()
    }

    pub fn devices(&self) -> Vec<AudioDevice> {
        self.inner
            .lock()
            .map(|state| state.devices.clone())
            .unwrap_or_default()
    }

    pub fn status(&self) -> AudioOutputStatus {
        self.inner
            .lock()
            .map(|state| state.status.clone())
            .unwrap_or_default()
    }

    pub fn set_settings(&self, settings: AudioSettings) -> AudioOutputStatus {
        let settings = settings.normalized();
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.settings = settings.clone();
        state.status.requested_mode = settings.mode;
        state.status.selected_device = settings.effective_output_device().to_string();
        state.status.output_format = None;
        state.status.output_rate = None;
        state.status.output_channels = None;
        recompute_status(&mut state);
        state.status.clone()
    }

    pub fn update_devices(&self, value: &Value) -> (bool, Vec<AudioDevice>) {
        let devices = parse_devices(value);
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return (false, Vec::new()),
        };
        let changed = state.devices != devices;
        state.devices = devices;
        let selected = state.settings.effective_output_device();
        let selected_connected =
            selected == "auto" || state.devices.iter().any(|device| device.name == selected);
        if !selected_connected {
            state.status.output_issue = Some(AudioOutputIssue::DeviceDisconnected);
        } else if state.status.output_issue == Some(AudioOutputIssue::DeviceDisconnected) {
            state.status.output_issue = None;
        }
        (changed, state.devices.clone())
    }

    pub fn update_current_ao(&self, current_ao: Option<String>) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.status.current_ao = current_ao;
        recompute_status(&mut state);
        state.status.clone()
    }

    pub fn update_decoder(&self, decoder: Option<String>) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.status.decoder = decoder;
        state.status.clone()
    }

    pub fn update_selected_device(&self, device: String) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.status.selected_device = device;
        state.status.clone()
    }

    pub fn update_input_codec(&self, codec: Option<String>) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.status.input_codec = codec;
        recompute_status(&mut state);
        state.status.clone()
    }

    pub fn update_selected_track(
        &self,
        codec: Option<String>,
        profile: Option<String>,
        channels: Option<String>,
    ) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        if codec.is_some() {
            state.status.input_codec = codec;
        }
        state.status.input_profile = profile;
        state.status.input_channels = channels;
        recompute_status(&mut state);
        state.status.clone()
    }

    pub fn update_output_params(&self, value: &Value) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.status.output_format = value
            .get("format")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        state.status.output_rate = value.get("samplerate").and_then(Value::as_i64);
        state.status.output_channels = value
            .get("channels")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        recompute_status(&mut state);
        state.status.clone()
    }

    pub fn clear_output(&self) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.status.output_format = None;
        state.status.output_rate = None;
        state.status.output_channels = None;
        recompute_status(&mut state);
        state.status.clone()
    }

    pub fn update_speed(&self, speed: f64) -> AudioOutputStatus {
        let mut state = match self.inner.lock() {
            Ok(state) => state,
            Err(_) => return AudioOutputStatus::default(),
        };
        state.speed = speed;
        recompute_status(&mut state);
        state.status.clone()
    }
}

fn parse_devices(value: &Value) -> Vec<AudioDevice> {
    let mut devices = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            let description = entry
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(name);
            Some(AudioDevice {
                name: name.to_string(),
                description: description.to_string(),
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        left.description
            .to_ascii_lowercase()
            .cmp(&right.description.to_ascii_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    devices
}

fn recompute_status(state: &mut RuntimeState) {
    let current_ao_is_null = state.status.current_ao.as_deref() == Some("null");
    let passthrough_active =
        is_passthrough_format(state.status.output_format.as_deref()) && !current_ao_is_null;
    let passthrough_requested_for_codec = state
        .status
        .input_codec
        .as_deref()
        .is_some_and(|codec| codec_is_enabled(&state.settings, codec));
    state.status.passthrough_active = passthrough_active;
    state.status.active_mode = if passthrough_active {
        ActiveAudioMode::Passthrough
    } else if current_ao_is_null || state.status.current_ao.is_none() {
        ActiveAudioMode::NullOutput
    } else {
        ActiveAudioMode::DecodedPcm
    };

    if state.speed.is_finite()
        && (state.speed - 1.0).abs() > f64::EPSILON
        && state.settings.passthrough_enabled()
    {
        state.status.output_issue = Some(AudioOutputIssue::SpeedIncompatible);
    } else if current_ao_is_null {
        state.status.output_issue = Some(AudioOutputIssue::OutputUnavailable);
    } else if passthrough_active {
        state.status.output_issue = None;
    } else if passthrough_requested_for_codec && state.status.output_format.is_some() {
        state.status.output_issue = Some(AudioOutputIssue::PassthroughOpenFailed);
    } else if state.status.output_issue == Some(AudioOutputIssue::SpeedIncompatible)
        || state.status.output_issue == Some(AudioOutputIssue::OutputUnavailable)
        || state.status.output_issue == Some(AudioOutputIssue::PassthroughOpenFailed)
    {
        state.status.output_issue = None;
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::AudioMode;
    use super::*;

    #[test]
    fn parses_and_sorts_mpv_audio_devices() {
        let runtime = AudioOutputRuntime::new(AudioSettings::default());
        let (_, devices) = runtime.update_devices(&serde_json::json!([
            {"name": "b", "description": "Z receiver"},
            {"name": "a", "description": "A receiver"}
        ]));
        assert_eq!(devices[0].name, "a");
        assert_eq!(devices[1].name, "b");
    }

    #[test]
    fn negotiated_spdif_is_the_only_confirmed_passthrough() {
        let runtime = AudioOutputRuntime::new(AudioSettings {
            mode: AudioMode::Passthrough,
            ..AudioSettings::default()
        });
        runtime.update_current_ao(Some("wasapi".to_string()));
        let status = runtime.update_output_params(&serde_json::json!({
            "format": "spdif-truehd",
            "samplerate": 192000,
            "channels": "7.1"
        }));
        assert!(status.passthrough_active);
        assert_eq!(status.active_mode, ActiveAudioMode::Passthrough);
    }

    #[test]
    fn disconnected_explicit_device_is_reported_without_changing_the_selection() {
        let runtime = AudioOutputRuntime::new(AudioSettings {
            output_device: "wasapi/receiver".to_string(),
            ..AudioSettings::default()
        });

        runtime.update_devices(&serde_json::json!([
            {"name": "wasapi/speakers", "description": "PC speakers"}
        ]));

        assert_eq!(runtime.settings().output_device, "wasapi/receiver");
        assert_eq!(runtime.status().selected_device, "wasapi/receiver");
        assert_eq!(
            runtime.status().output_issue,
            Some(AudioOutputIssue::DeviceDisconnected)
        );
    }

    #[test]
    fn decoded_pcm_is_an_issue_only_for_a_passthrough_eligible_codec() {
        let runtime = AudioOutputRuntime::new(AudioSettings {
            mode: AudioMode::Passthrough,
            ..AudioSettings::default()
        });
        runtime.update_current_ao(Some("wasapi".to_string()));
        runtime.update_input_codec(Some("aac".to_string()));
        let pcm_status = runtime.update_output_params(&serde_json::json!({
            "format": "float",
            "samplerate": 48000,
            "channels": "stereo"
        }));
        assert_eq!(pcm_status.active_mode, ActiveAudioMode::DecodedPcm);
        assert_eq!(pcm_status.output_issue, None);

        let passthrough_status = runtime.update_input_codec(Some("truehd".to_string()));
        assert_eq!(
            passthrough_status.output_issue,
            Some(AudioOutputIssue::PassthroughOpenFailed)
        );
    }

    #[test]
    fn speed_change_reports_that_passthrough_is_incompatible() {
        let runtime = AudioOutputRuntime::new(AudioSettings {
            mode: AudioMode::Passthrough,
            ..AudioSettings::default()
        });
        runtime.update_current_ao(Some("wasapi".to_string()));
        let status = runtime.update_speed(1.25);
        assert_eq!(
            status.output_issue,
            Some(AudioOutputIssue::SpeedIncompatible)
        );
    }

    #[test]
    fn changing_settings_clears_stale_negotiated_output() {
        let runtime = AudioOutputRuntime::new(AudioSettings {
            mode: AudioMode::Passthrough,
            output_device: "wasapi/receiver-a".to_string(),
            ..AudioSettings::default()
        });
        runtime.update_current_ao(Some("wasapi".to_string()));
        runtime.update_input_codec(Some("truehd".to_string()));
        runtime.update_output_params(&serde_json::json!({
            "format": "spdif-truehd",
            "samplerate": 192000,
            "channels": "7.1"
        }));

        let status = runtime.set_settings(AudioSettings {
            mode: AudioMode::Passthrough,
            output_device: "wasapi/receiver-b".to_string(),
            ..AudioSettings::default()
        });

        assert_eq!(status.selected_device, "wasapi/receiver-b");
        assert_eq!(status.output_format, None);
        assert_eq!(status.output_rate, None);
        assert_eq!(status.output_channels, None);
        assert!(!status.passthrough_active);
        assert_eq!(status.output_issue, None);
    }
}
