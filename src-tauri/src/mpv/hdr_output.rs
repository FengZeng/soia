use super::MpvHandle;
use log::warn;

#[cfg(target_os = "macos")]
const HDR_OUTPUT_OPTIONS: &[(&str, &str)] = &[
    ("target-colorspace-hint", "yes"),
    ("target-prim", "display-p3"),
    ("target-trc", "pq"),
    ("tone-mapping", "auto"),
    ("hdr-compute-peak", "yes"),
    ("gamut-mapping-mode", "perceptual"),
    ("cocoa-cb-output-csp", "display-p3-pq"),
];

#[cfg(not(target_os = "macos"))]
const HDR_OUTPUT_OPTIONS: &[(&str, &str)] = &[
    ("target-colorspace-hint", "yes"),
    ("target-prim", "bt.2020"),
    ("target-trc", "pq"),
    ("tone-mapping", "auto"),
    ("hdr-compute-peak", "yes"),
    ("gamut-mapping-mode", "perceptual"),
];

#[derive(Default)]
pub(super) struct HdrOutputController {
    enabled: bool,
    previous_options: Vec<(&'static str, String)>,
}

impl HdrOutputController {
    pub(super) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn update(&mut self, mpv: &MpvHandle, enabled: bool) -> bool {
        if self.enabled == enabled {
            return true;
        }

        if enabled {
            self.previous_options.clear();
            let mut success = true;
            for &(name, value) in HDR_OUTPUT_OPTIONS {
                match mpv.get_property_string(name) {
                    Ok(previous_value) => self.previous_options.push((name, previous_value)),
                    Err(error) => {
                        warn!("Failed to preserve mpv HDR option {name}: {error}");
                        success = false;
                        continue;
                    }
                }
                if !set_property(mpv, name, value) {
                    success = false;
                }
            }
            if !success {
                for (name, value) in self.previous_options.drain(..) {
                    let _ = set_property(mpv, name, &value);
                }
                return false;
            }
        } else {
            for (name, value) in self.previous_options.drain(..) {
                let _ = set_property(mpv, name, &value);
            }
        }

        self.enabled = enabled;
        true
    }
}

fn set_property(mpv: &MpvHandle, name: &str, value: &str) -> bool {
    let result = mpv.command(&["set", name, value]);
    if result < 0 {
        warn!("Failed to set mpv HDR option {name}={value}: {result}");
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::HDR_OUTPUT_OPTIONS;

    #[test]
    fn hdr_output_options_match_required_color_pipeline() {
        assert!(HDR_OUTPUT_OPTIONS.contains(&("target-colorspace-hint", "yes")));
        #[cfg(target_os = "macos")]
        assert!(HDR_OUTPUT_OPTIONS.contains(&("target-prim", "display-p3")));
        #[cfg(not(target_os = "macos"))]
        assert!(HDR_OUTPUT_OPTIONS.contains(&("target-prim", "bt.2020")));
        assert!(HDR_OUTPUT_OPTIONS.contains(&("target-trc", "pq")));
        assert!(HDR_OUTPUT_OPTIONS.contains(&("tone-mapping", "auto")));
        assert!(HDR_OUTPUT_OPTIONS.contains(&("hdr-compute-peak", "yes")));
        assert!(HDR_OUTPUT_OPTIONS.contains(&("gamut-mapping-mode", "perceptual")));
        #[cfg(target_os = "macos")]
        assert!(HDR_OUTPUT_OPTIONS.contains(&("cocoa-cb-output-csp", "display-p3-pq")));
    }
}
