use crate::{mpv_command_checked, mpv_command_direct_checked, mpv_set_option_string_checked};
use std::sync::Mutex;

pub(crate) struct PlaybackLoadCoordinator {
    generation: Mutex<u64>,
}

impl PlaybackLoadCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            generation: Mutex::new(0),
        }
    }

    pub(crate) fn begin(&self) -> u64 {
        let mut generation = self
            .generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *generation = generation.saturating_add(1);
        *generation
    }

    pub(crate) fn execute_if_current<T>(
        &self,
        generation: u64,
        execute: impl FnOnce() -> T,
    ) -> Option<T> {
        let current = self
            .generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *current != generation {
            return None;
        }
        Some(execute())
    }
}

fn build_load_file_command_args(
    url: &str,
    resume_position: f64,
    load_options: &[String],
) -> Vec<String> {
    if resume_position <= 0.0 && load_options.is_empty() {
        return vec!["loadfile".to_string(), url.to_string()];
    }

    let mut options = Vec::new();
    if resume_position > 0.0 {
        options.push(format!("start={resume_position}"));
    }
    options.extend(load_options.iter().cloned());

    vec![
        "loadfile".to_string(),
        url.to_string(),
        "replace".to_string(),
        "0".to_string(),
        options.join(","),
    ]
}

#[derive(Clone, Copy)]
pub(crate) enum LoadCommandMode {
    Normal,
    Direct,
}

pub(crate) struct PlaybackLoadOptions {
    resume_position: f64,
    auto_play: bool,
    playback_speed: f64,
}

impl PlaybackLoadOptions {
    pub(crate) fn from_optional(
        resume_position: Option<f64>,
        auto_play: Option<bool>,
        playback_speed: Option<f64>,
    ) -> Result<Self, String> {
        let resume_position = resume_position.unwrap_or(0.0);
        if !resume_position.is_finite() || resume_position < 0.0 {
            return Err("resume position must be a non-negative finite number".to_string());
        }
        let playback_speed = playback_speed.unwrap_or(1.0);
        if !playback_speed.is_finite() || playback_speed <= 0.0 {
            return Err("playback speed must be a positive finite number".to_string());
        }
        Ok(Self {
            resume_position,
            auto_play: auto_play.unwrap_or(true),
            playback_speed,
        })
    }
}

pub(crate) fn load(
    mpv: &crate::mpv::MpvHandle,
    playback_url: &str,
    load_options: &[String],
    options: PlaybackLoadOptions,
    command_mode: LoadCommandMode,
) -> Result<(), String> {
    mpv_set_option_string_checked(mpv, "speed", &options.playback_speed.to_string())?;
    let command_args = build_load_file_command_args(
        playback_url,
        options.resume_position,
        load_options,
    );
    let command_refs: Vec<&str> = command_args.iter().map(String::as_str).collect();
    match command_mode {
        LoadCommandMode::Normal => mpv_command_checked(mpv, &command_refs)?,
        LoadCommandMode::Direct => mpv_command_direct_checked(mpv, &command_refs)?,
    }
    mpv_command_checked(
        mpv,
        &["set", "pause", if options.auto_play { "no" } else { "yes" }],
    )
}

#[cfg(test)]
mod tests {
    use super::{build_load_file_command_args, PlaybackLoadCoordinator, PlaybackLoadOptions};

    #[test]
    fn load_options_apply_defaults() {
        let options = PlaybackLoadOptions::from_optional(None, None, None)
            .expect("default load options should be valid");

        assert_eq!(options.resume_position, 0.0);
        assert!(options.auto_play);
        assert_eq!(options.playback_speed, 1.0);
    }

    #[test]
    fn load_options_reject_invalid_numbers() {
        assert!(PlaybackLoadOptions::from_optional(Some(-1.0), None, None).is_err());
        assert!(PlaybackLoadOptions::from_optional(None, None, Some(0.0)).is_err());
        assert!(PlaybackLoadOptions::from_optional(None, None, Some(f64::NAN)).is_err());
    }

    #[test]
    fn load_command_combines_resume_and_mpv_options() {
        let command = build_load_file_command_args(
            "https://example.test/video",
            42.5,
            &["force-media-title=Example".to_string()],
        );

        assert_eq!(
            command,
            vec![
                "loadfile",
                "https://example.test/video",
                "replace",
                "0",
                "start=42.5,force-media-title=Example",
            ],
        );
    }

    #[test]
    fn newer_load_generation_supersedes_older_work() {
        let coordinator = PlaybackLoadCoordinator::new();
        let first = coordinator.begin();
        let second = coordinator.begin();

        assert!(coordinator.execute_if_current(first, || true).is_none());
        assert_eq!(coordinator.execute_if_current(second, || 42), Some(42));
    }
}
