pub mod dlna;
pub mod smb;
pub mod webdav;

/// Returns true when the value already starts with an explicit `scheme://` prefix.
fn has_explicit_scheme(value: &str) -> bool {
    let Some(index) = value.find("://") else {
        return false;
    };
    let scheme = &value[..index];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Adds the default `http://` prefix to HTTP-based connection URLs typed without a scheme.
///
/// Users commonly enter `host:port/path`, which WHATWG URL parsing either rejects
/// (`192.168.31.25:5244/dav`) or misreads as an opaque URL whose scheme is the host name
/// (`nas:5244/dav`), swallowing the port into the path. Keep values that already carry a
/// scheme untouched so non-HTTP schemes still fail with their own protocol error.
pub fn normalize_http_base_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || has_explicit_scheme(trimmed) {
        return trimmed.to_string();
    }
    format!("http://{}", trimmed)
}

#[cfg(test)]
mod tests {
    use super::normalize_http_base_url;

    #[test]
    fn keeps_urls_that_already_have_a_scheme() {
        assert_eq!(
            normalize_http_base_url(" https://example.com/webdav "),
            "https://example.com/webdav"
        );
        assert_eq!(
            normalize_http_base_url("http://192.168.31.25:5244/dav"),
            "http://192.168.31.25:5244/dav"
        );
        assert_eq!(
            normalize_http_base_url("smb://nas/media"),
            "smb://nas/media"
        );
    }

    #[test]
    fn prefixes_scheme_less_hosts_so_the_port_survives_parsing() {
        assert_eq!(
            normalize_http_base_url("192.168.31.25:5244/dav"),
            "http://192.168.31.25:5244/dav"
        );
        assert_eq!(
            normalize_http_base_url("nas:5244/dav"),
            "http://nas:5244/dav"
        );
        assert_eq!(
            normalize_http_base_url("nas.local:5244"),
            "http://nas.local:5244"
        );
        assert_eq!(
            normalize_http_base_url("example.com/webdav"),
            "http://example.com/webdav"
        );
    }

    #[test]
    fn keeps_empty_values_empty() {
        assert_eq!(normalize_http_base_url("   "), "");
    }
}
