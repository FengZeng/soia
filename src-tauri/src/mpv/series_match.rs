use log::debug;

const IGNORED_TOKENS: &[&str] = &[
    "1080p", "2160p", "720p", "480p", "4k", "8k", "web", "webrip", "webdl", "bluray", "bdrip",
    "hdrip", "dvdrip", "hdtv", "x264", "x265", "h264", "h265", "hevc", "avc", "aac", "dts",
    "ddp", "atmos", "proper", "repack", "remux", "extended", "internal", "multi", "flac", "10bit",
    "hdr", "sdr",
];

/// Manages title state for series detection across file transitions.
pub(super) struct SeriesMatcher {
    reference_title: Option<String>,
    awaiting_title_match: bool,
}

impl SeriesMatcher {
    pub fn new() -> Self {
        Self {
            reference_title: None,
            awaiting_title_match: false,
        }
    }

    /// Called when START_FILE fires, before properties for the new file arrive.
    pub fn on_file_started(&mut self) {
        self.awaiting_title_match = true;
    }

    pub fn on_media_title_change(&mut self, new_title: &str) -> bool {
        if self.awaiting_title_match {
            self.awaiting_title_match = false;

            if let Some(reference) = &self.reference_title {
                if is_same_series(reference, new_title) {
                    debug!(
                        "track carry-over: series match ({:?} ~ {:?})",
                        truncate_for_log(reference),
                        truncate_for_log(new_title),
                    );
                    self.reference_title = Some(new_title.to_string());
                    return true;
                } else {
                    debug!(
                        "track carry-over: series mismatch ({:?} vs {:?})",
                        truncate_for_log(reference),
                        truncate_for_log(new_title),
                    );
                }
            }
        }

        if !new_title.is_empty() {
            self.reference_title = Some(new_title.to_string());
        }

        false
    }
}

fn truncate_for_log(s: &str) -> &str {
    s.char_indices()
        .nth(60)
        .map(|(index, _)| &s[..index])
        .unwrap_or(s)
}

// --- Series matching logic ---

fn normalize_text(value: &str) -> String {
    let mut result = String::new();
    let mut prev_space = false;
    let lowercase = value.to_lowercase();
    let chars: Vec<char> = lowercase.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '第' {
            let mut cursor = index + 1;
            while cursor < chars.len() && chars[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor > index + 1
                && cursor < chars.len()
                && matches!(chars[cursor], '集' | '话' | '期')
            {
                if !prev_space && !result.is_empty() {
                    result.push(' ');
                    prev_space = true;
                }
                index = cursor + 1;
                continue;
            }
        }
        if ch == '\'' || ch == '"' {
            index += 1;
            continue;
        }
        if ch.is_alphanumeric() {
            result.push(ch);
            prev_space = false;
        } else if !prev_space && !result.is_empty() {
            result.push(' ');
            prev_space = true;
        }
        index += 1;
    }
    result.trim_end().to_string()
}

fn tokenize(normalized: &str) -> Vec<&str> {
    normalized
        .split_whitespace()
        .filter(|token| !IGNORED_TOKENS.contains(token))
        .collect()
}

fn extract_sxxexx_keys(value: &str) -> Vec<String> {
    let compact: String = value
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect();
    let chars: Vec<char> = compact.chars().collect();
    let mut results: Vec<String> = Vec::new();

    for i in 0..chars.len() {
        if chars[i] != 's' {
            continue;
        }
        let mut cursor = i + 1;
        let season_start = cursor;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && cursor - season_start < 4 {
            cursor += 1;
        }
        if cursor == season_start || cursor >= chars.len() || chars[cursor] != 'e' {
            continue;
        }
        cursor += 1;
        let ep_start = cursor;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && cursor - ep_start < 4 {
            cursor += 1;
        }
        if cursor == ep_start {
            continue;
        }
        let key: String = chars[i..cursor].iter().collect();
        if !results.contains(&key) {
            results.push(key);
        }
    }
    results
}

fn extract_series_identity(title: &str) -> String {
    let normalized = normalize_text(title);
    let tokens = tokenize(&normalized);
    if tokens.is_empty() {
        return String::new();
    }

    let sxxexx_keys = extract_sxxexx_keys(title);
    let filtered: Vec<&str> = tokens
        .into_iter()
        .filter(|token| {
            for key in &sxxexx_keys {
                if key.contains(*token) {
                    return false;
                }
            }
            if token.len() > 1
                && token.starts_with('e')
                && token[1..].chars().all(|c| c.is_ascii_digit())
            {
                return false;
            }
            if token.len() > 2
                && token.starts_with("ep")
                && token[2..].chars().all(|c| c.is_ascii_digit())
            {
                return false;
            }
            if token.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
            true
        })
        .collect();

    filtered.join(" ")
}

fn is_same_series(title_a: &str, title_b: &str) -> bool {
    if title_a.is_empty() || title_b.is_empty() {
        return false;
    }

    let identity_a = extract_series_identity(title_a);
    let identity_b = extract_series_identity(title_b);

    if identity_a.is_empty() || identity_b.is_empty() {
        return false;
    }

    if identity_a == identity_b {
        return true;
    }

    let tokens_a: Vec<&str> = identity_a.split_whitespace().collect();
    let tokens_b: Vec<&str> = identity_b.split_whitespace().collect();
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return false;
    }

    let shared = tokens_a.iter().filter(|t| tokens_b.contains(t)).count();
    if shared == 0 {
        return false;
    }

    let dice = (2.0 * shared as f64) / (tokens_a.len() + tokens_b.len()) as f64;

    if tokens_a.len() <= 2 || tokens_b.len() <= 2 {
        dice >= 0.8 && shared >= tokens_a.len().min(tokens_b.len())
    } else {
        dice >= 0.7 && shared >= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_series_sxxexx() {
        assert!(is_same_series(
            "Breaking Bad S01E01 720p BluRay x264",
            "Breaking Bad S01E02 720p BluRay x264"
        ));
    }

    #[test]
    fn test_same_series_numbered() {
        assert!(is_same_series("The Office - 01", "The Office - 02"));
    }

    #[test]
    fn test_same_series_dotted() {
        assert!(is_same_series(
            "Show.Name.S02E05.BluRay.x264.mkv",
            "Show.Name.S02E06.BluRay.x264.mkv"
        ));
    }

    #[test]
    fn test_different_series() {
        assert!(!is_same_series(
            "Breaking Bad S01E01",
            "Better Call Saul S01E01"
        ));
    }

    #[test]
    fn test_same_series_chinese() {
        assert!(is_same_series("三体 第1集", "三体 第2集"));
        assert!(is_same_series("三体第01话", "三体第02话"));
    }

    #[test]
    fn test_same_series_brackets() {
        assert!(is_same_series(
            "[SubGroup] My Show - 03 [1080p]",
            "[SubGroup] My Show - 04 [1080p]"
        ));
    }

    #[test]
    fn test_empty_titles() {
        assert!(!is_same_series("", "Something"));
        assert!(!is_same_series("Something", ""));
    }

    #[test]
    fn test_truncate_for_log_preserves_utf8_boundaries() {
        let title = format!("{}中文", "a".repeat(59));
        assert_eq!(truncate_for_log(&title), format!("{}中", "a".repeat(59)));
    }
}
