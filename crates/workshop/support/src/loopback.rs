//! Shared loopback HTTP(S) URL predicate for Gateway clients and cross-site guards.

/// Whether `url` parses as an `http` or `https` URL whose authority targets
/// loopback (`127.0.0.0/8`, `[::1]`, or `localhost` case-insensitively).
///
/// Non-HTTP schemes (`file:`, `ws:`, etc.), the opaque `null` origin, and
/// unparseable inputs return `false`.
#[must_use]
pub fn is_loopback_http_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    match parsed.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_loopback_http_url;

    #[test]
    fn loopback_http_and_https_urls_are_accepted() {
        for url in [
            "http://127.0.0.1:7910",
            "http://127.5.0.1",
            "http://localhost:7910",
            "http://LOCALHOST",
            "http://[::1]:7910",
            "https://127.0.0.1",
            "https://localhost:8443/v1/models",
        ] {
            assert!(is_loopback_http_url(url), "{url} must be loopback");
        }
    }

    #[test]
    fn foreign_non_http_and_malformed_urls_are_refused() {
        for url in [
            "https://evil.example",
            "http://192.168.1.10:7910",
            "https://localhost.evil.example",
            "file:///etc/passwd",
            "file://localhost/etc/passwd",
            "ws://127.0.0.1:7910/ws",
            "null",
            "",
        ] {
            assert!(!is_loopback_http_url(url), "{url} must be refused");
        }
    }
}
