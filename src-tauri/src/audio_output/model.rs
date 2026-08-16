use serde::{Deserialize, Serialize};

pub const AUDIO_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioMode {
    #[serde(rename = "pcm", alias = "compatible_pcm")]
    #[default]
    Pcm,
    #[serde(rename = "passthrough", alias = "source_direct", alias = "custom")]
    Passthrough,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioSettings {
    pub schema_version: u32,
    pub mode: AudioMode,
    #[serde(alias = "pcmDevice")]
    pub output_device: String,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            schema_version: AUDIO_SETTINGS_SCHEMA_VERSION,
            mode: AudioMode::Pcm,
            output_device: "auto".to_string(),
        }
    }
}

impl AudioSettings {
    pub fn normalized(mut self) -> Self {
        self.schema_version = AUDIO_SETTINGS_SCHEMA_VERSION;
        self.output_device = normalize_device_name(&self.output_device, "auto");
        self
    }

    pub fn passthrough_enabled(&self) -> bool {
        self.mode == AudioMode::Passthrough
    }

    pub fn effective_output_device(&self) -> &str {
        &self.output_device
    }
}

fn normalize_device_name(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActiveAudioMode {
    Passthrough,
    DecodedPcm,
    #[default]
    NullOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioOutputIssue {
    SpeedIncompatible,
    DeviceDisconnected,
    PassthroughOpenFailed,
    OutputUnavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioOutputStatus {
    pub requested_mode: AudioMode,
    pub active_mode: ActiveAudioMode,
    pub input_codec: Option<String>,
    pub input_profile: Option<String>,
    pub input_channels: Option<String>,
    pub selected_device: String,
    pub current_ao: Option<String>,
    pub decoder: Option<String>,
    pub output_format: Option<String>,
    pub output_rate: Option<i64>,
    pub output_channels: Option<String>,
    pub passthrough_active: bool,
    pub output_issue: Option<AudioOutputIssue>,
}

impl Default for AudioOutputStatus {
    fn default() -> Self {
        Self {
            requested_mode: AudioMode::Pcm,
            active_mode: ActiveAudioMode::NullOutput,
            input_codec: None,
            input_profile: None,
            input_channels: None,
            selected_device: "auto".to_string(),
            current_ao: None,
            decoder: None,
            output_format: None,
            output_rate: None,
            output_channels: None,
            passthrough_active: false,
            output_issue: None,
        }
    }
}
