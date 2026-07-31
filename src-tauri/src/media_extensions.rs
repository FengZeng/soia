use log::error;
use std::sync::OnceLock;

const MEDIA_EXTENSIONS_JSON: &str = include_str!("../../src/constants/mediaExtensions.json");

static MEDIA_EXTENSIONS: OnceLock<Vec<String>> = OnceLock::new();
#[cfg(target_os = "macos")]
static MEDIA_ASSOCIATION_EXTENSIONS: OnceLock<Vec<String>> = OnceLock::new();

#[derive(serde::Deserialize)]
struct MediaExtensionEntry {
    ext: String,
    #[cfg(target_os = "macos")]
    kind: String,
}

fn parse_media_extensions() -> Vec<String> {
    match serde_json::from_str::<Vec<MediaExtensionEntry>>(MEDIA_EXTENSIONS_JSON) {
        Ok(values) => values
            .into_iter()
            .map(|value| value.ext.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect(),
        Err(error) => {
            error!("Failed to parse media extensions JSON: {error}");
            Vec::new()
        }
    }
}

pub(crate) fn all() -> &'static [String] {
    MEDIA_EXTENSIONS
        .get_or_init(parse_media_extensions)
        .as_slice()
}

#[cfg(target_os = "macos")]
pub(crate) fn media_association_extensions() -> &'static [String] {
    MEDIA_ASSOCIATION_EXTENSIONS
        .get_or_init(|| {
            match serde_json::from_str::<Vec<MediaExtensionEntry>>(MEDIA_EXTENSIONS_JSON) {
                Ok(values) => values
                    .into_iter()
                    .filter(|value| !value.kind.eq_ignore_ascii_case("image"))
                    .map(|value| value.ext.trim().to_ascii_lowercase())
                    .filter(|value| !value.is_empty())
                    .collect(),
                Err(error) => {
                    error!("Failed to parse media extensions JSON for media association: {error}");
                    Vec::new()
                }
            }
        })
        .as_slice()
}

pub(crate) fn contains_extension(extension: &str) -> bool {
    let normalized = extension.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    all().iter().any(|candidate| candidate == &normalized)
}
