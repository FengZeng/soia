use crate::mpv::MpvHandle;
use crate::mpv_command_checked;
use crate::store::storage_paths;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;

const INTERNAL_SHADER_DIRECTORY: &str = "internal-shaders";
const LUMINANCE_SHADER_FILE_NAME: &str = "soia-luminance.glsl";
const LUMINANCE_SHADER_SOURCE: &str = include_str!("../resources/shaders/soia-luminance.glsl");
const LUMINANCE_MAX_SCALE: f64 = 8.0;
const LUMINANCE_MIN_ADJUSTMENT: f64 = -100.0;
const LUMINANCE_MAX_ADJUSTMENT: f64 = 100.0;

#[derive(Default)]
struct ShaderPipelineRuntime {
    active_user_shaders: Vec<String>,
    luminance_adjustment: f64,
    is_hdr_content: bool,
    initialized: bool,
}

#[derive(Default)]
pub(crate) struct ShaderPipeline {
    runtime: Mutex<ShaderPipelineRuntime>,
}

fn luminance_scale_from_adjustment(value: f64) -> f64 {
    let adjustment = value.clamp(LUMINANCE_MIN_ADJUSTMENT, LUMINANCE_MAX_ADJUSTMENT);
    LUMINANCE_MAX_SCALE.powf(adjustment / LUMINANCE_MAX_ADJUSTMENT)
}

fn effective_luminance_scale(value: f64, is_hdr_content: bool) -> f64 {
    if is_hdr_content {
        luminance_scale_from_adjustment(value)
    } else {
        1.0
    }
}

fn internal_luminance_shader_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = storage_paths::app_data_dir(app)?.join(INTERNAL_SHADER_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(LUMINANCE_SHADER_FILE_NAME);

    let current = fs::read_to_string(&path).ok();
    if current.as_deref() != Some(LUMINANCE_SHADER_SOURCE) {
        fs::write(&path, LUMINANCE_SHADER_SOURCE).map_err(|error| error.to_string())?;
    }

    Ok(path)
}

fn compose_shader_stack(internal_shader: &str, user_shaders: &[String]) -> Vec<String> {
    let mut shaders = Vec::with_capacity(user_shaders.len() + 1);
    shaders.push(internal_shader.to_string());
    shaders.extend(user_shaders.iter().cloned());
    shaders
}

fn apply_shader_stack(
    mpv: &MpvHandle,
    internal_shader: &str,
    user_shaders: &[String],
) -> Result<(), String> {
    mpv_command_checked(mpv, &["change-list", "glsl-shaders", "clr", ""])?;
    for shader in compose_shader_stack(internal_shader, user_shaders) {
        mpv_command_checked(mpv, &["change-list", "glsl-shaders", "append", &shader])?;
    }
    Ok(())
}

impl ShaderPipeline {
    fn apply_luminance_scale(
        runtime: &mut ShaderPipelineRuntime,
        app: &AppHandle,
        mpv: &MpvHandle,
    ) -> Result<f64, String> {
        if !runtime.initialized {
            let internal_shader = internal_luminance_shader_path(app)?;
            let internal_shader = internal_shader.to_string_lossy();
            apply_shader_stack(mpv, internal_shader.as_ref(), &runtime.active_user_shaders)?;
            runtime.initialized = true;
        }

        let scale = effective_luminance_scale(runtime.luminance_adjustment, runtime.is_hdr_content);
        let shader_option = format!("soia-luminance/soia_luminance_scale={scale:.6}");
        // glsl-shader-opts is a key/value list. Appending replaces this named
        // key while preserving parameters owned by user shaders.
        mpv_command_checked(
            mpv,
            &["change-list", "glsl-shader-opts", "append", &shader_option],
        )?;
        Ok(scale)
    }

    pub(crate) fn apply_user_shaders(
        &self,
        app: &AppHandle,
        mpv: &MpvHandle,
        user_shaders: &[String],
    ) -> Result<(), String> {
        let mut runtime = self.runtime.lock().map_err(|error| error.to_string())?;
        let internal_shader = internal_luminance_shader_path(app)?;
        let internal_shader = internal_shader.to_string_lossy();
        apply_shader_stack(mpv, internal_shader.as_ref(), user_shaders)?;
        runtime.active_user_shaders = user_shaders.to_vec();
        runtime.initialized = true;
        Ok(())
    }

    pub(crate) fn set_luminance_adjustment(
        &self,
        app: &AppHandle,
        mpv: &MpvHandle,
        value: f64,
    ) -> Result<f64, String> {
        if !value.is_finite() {
            return Err("Luminance adjustment must be finite".to_string());
        }

        let mut runtime = self.runtime.lock().map_err(|error| error.to_string())?;
        runtime.luminance_adjustment =
            value.clamp(LUMINANCE_MIN_ADJUSTMENT, LUMINANCE_MAX_ADJUSTMENT);
        Self::apply_luminance_scale(&mut runtime, app, mpv)
    }

    pub(crate) fn set_hdr_content(
        &self,
        app: &AppHandle,
        mpv: &MpvHandle,
        is_hdr_content: bool,
    ) -> Result<f64, String> {
        let mut runtime = self.runtime.lock().map_err(|error| error.to_string())?;
        runtime.is_hdr_content = is_hdr_content;
        Self::apply_luminance_scale(&mut runtime, app, mpv)
    }
}

#[cfg(test)]
mod tests {
    use super::{compose_shader_stack, effective_luminance_scale, luminance_scale_from_adjustment};

    #[test]
    fn luminance_adjustment_is_symmetric_around_zero() {
        assert!((luminance_scale_from_adjustment(0.0) - 1.0).abs() < f64::EPSILON);
        assert!((luminance_scale_from_adjustment(100.0) - 8.0).abs() < f64::EPSILON);
        assert!((luminance_scale_from_adjustment(-100.0) - 0.125).abs() < f64::EPSILON);
        assert!(
            (luminance_scale_from_adjustment(50.0) * luminance_scale_from_adjustment(-50.0) - 1.0)
                .abs()
                < 0.000001
        );
    }

    #[test]
    fn luminance_adjustment_is_clamped() {
        assert!((luminance_scale_from_adjustment(500.0) - 8.0).abs() < f64::EPSILON);
        assert!((luminance_scale_from_adjustment(-500.0) - 0.125).abs() < f64::EPSILON);
    }

    #[test]
    fn luminance_adjustment_is_disabled_for_sdr_content() {
        assert!((effective_luminance_scale(100.0, false) - 1.0).abs() < f64::EPSILON);
        assert!((effective_luminance_scale(100.0, true) - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn internal_shader_is_prepended_without_reordering_user_shaders() {
        let user_shaders = vec!["first.glsl".to_string(), "second.glsl".to_string()];
        assert_eq!(
            compose_shader_stack("soia-luminance.glsl", &user_shaders),
            vec![
                "soia-luminance.glsl".to_string(),
                "first.glsl".to_string(),
                "second.glsl".to_string(),
            ]
        );
    }
}
