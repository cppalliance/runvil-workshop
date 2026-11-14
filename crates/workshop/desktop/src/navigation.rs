//! The navigation policy: the webview may load only the server's own
//! origin; every other http(s) target opens in the system browser.
//!
//! A clicked link never navigates the app away from itself, and no other
//! loopback server - same machine, different port, or a different
//! loopback spelling - gets the desktop app's Tauri API surface, folder
//! picker, or file-drop bridge. Other schemes (`about:blank`, `data:`)
//! and unparseable values are left to the webview.

/// What the app does with a URL the webview wants to navigate to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Navigation {
    /// The webview loads the URL itself.
    Allow,
    /// The URL opens in the system browser; the webview stays put.
    OpenExternally,
}

/// What the webview does when a top-level page load begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopLevelLoadAction {
    /// The top frame is loading the trusted main origin: reset capability state
    /// so the newly loaded main document can perform its single bind.
    AllowMainAndResetCapability,
    /// Non-HTTP scheme (`about:blank`, etc.): ignore without resetting or redirecting.
    IgnoreNonHttp,
    /// Top-level load targets a non-main HTTP(S) origin (such as the config
    /// origin): refuse staying there and bounce back to the main origin.
    RejectAndReturnMain,
}

/// Classifies a navigation target against both the main server origin
/// and the dedicated Gateway config-panel origin.
///
/// Only the main origin may load arbitrarily; the dedicated config origin
/// is admitted strictly at its docked panel entry URL (`/?mode=panel&bridge=<main>`),
/// so bare or user-driven navigations to the config origin open externally.
#[must_use]
pub(crate) fn classify_navigation_with_config(
    server: &url::Origin,
    config_origin: Option<&url::Origin>,
    target: &str,
) -> Navigation {
    let Ok(url) = url::Url::parse(target) else {
        return Navigation::Allow;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Navigation::Allow;
    }
    if url.origin() == *server {
        return Navigation::Allow;
    }
    if config_origin.is_some_and(|allowed| is_panel_entry_url(server, allowed, &url)) {
        return Navigation::Allow;
    }
    Navigation::OpenExternally
}

fn is_panel_entry_url(server: &url::Origin, config_origin: &url::Origin, url: &url::Url) -> bool {
    if url.origin() != *config_origin || url.path() != "/" {
        return false;
    }
    let expected_bridge = server.ascii_serialization();
    let mut saw_panel_mode = false;
    let mut saw_bridge = false;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "mode" if value == "panel" => saw_panel_mode = true,
            "bridge" if value == expected_bridge => saw_bridge = true,
            _ => return false,
        }
    }
    saw_panel_mode && saw_bridge
}

/// Classifies a top-level `PageLoadEvent::Started` URL observed in Rust.
#[must_use]
pub(crate) fn classify_top_level_load(
    server: &url::Origin,
    loaded_url: &url::Url,
) -> TopLevelLoadAction {
    if !matches!(loaded_url.scheme(), "http" | "https") {
        return TopLevelLoadAction::IgnoreNonHttp;
    }
    if loaded_url.origin() == *server {
        TopLevelLoadAction::AllowMainAndResetCapability
    } else {
        TopLevelLoadAction::RejectAndReturnMain
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Navigation, TopLevelLoadAction, classify_navigation_with_config, classify_top_level_load,
    };

    /// The origin of the in-process server the test window was opened on.
    fn server_origin(url: &str) -> url::Origin {
        match url::Url::parse(url) {
            Ok(parsed) => parsed.origin(),
            Err(error) => panic!("the test server URL must parse: {error}"),
        }
    }

    #[test]
    fn same_origin_urls_load_in_the_webview() {
        let server = server_origin("http://127.0.0.1:7910/");
        for url in [
            "http://127.0.0.1:7910/",
            "http://127.0.0.1:7910/ws",
            "http://127.0.0.1:7910/settings?tab=keys#top",
        ] {
            assert_eq!(
                classify_navigation_with_config(&server, None, url),
                Navigation::Allow,
                "{url}"
            );
        }
    }

    #[test]
    fn a_default_port_normalizes_into_the_origin() {
        let server = server_origin("https://127.0.0.1/");
        assert_eq!(
            classify_navigation_with_config(&server, None, "https://127.0.0.1:443/"),
            Navigation::Allow
        );
    }

    #[test]
    fn other_loopback_origins_open_in_the_system_browser() {
        let server = server_origin("http://127.0.0.1:7910/");
        for url in [
            // Another loopback server is not the workshop, whatever the
            // port, scheme, or loopback spelling.
            "http://127.0.0.1:4000/",
            "https://127.0.0.1:7910/",
            "http://localhost:7910/",
            "http://[::1]:7910/",
        ] {
            assert_eq!(
                classify_navigation_with_config(&server, None, url),
                Navigation::OpenExternally,
                "{url}"
            );
        }
    }

    #[test]
    fn external_urls_open_in_the_system_browser() {
        let server = server_origin("http://127.0.0.1:7910/");
        for url in [
            "https://example.com/",
            "http://192.168.1.10/",
            "https://localhost.evil.example/",
        ] {
            assert_eq!(
                classify_navigation_with_config(&server, None, url),
                Navigation::OpenExternally,
                "{url}"
            );
        }
    }

    #[test]
    fn configured_iframe_origin_loads_while_top_level_and_other_ports_open_externally() {
        let server = server_origin("http://127.0.0.1:7910/");
        let config = server_origin("http://127.0.0.1:7911/");
        assert_eq!(
            classify_navigation_with_config(
                &server,
                Some(&config),
                "http://127.0.0.1:7911/?mode=panel&bridge=http%3A%2F%2F127.0.0.1%3A7910"
            ),
            Navigation::Allow
        );
        for refused in [
            "http://127.0.0.1:7911/",
            "http://127.0.0.1:7911/other",
            "http://127.0.0.1:7911/?mode=panel&bridge=https%3A%2F%2Fevil.example",
            "http://127.0.0.1:7912/",
        ] {
            assert_eq!(
                classify_navigation_with_config(&server, Some(&config), refused),
                Navigation::OpenExternally,
                "{refused}"
            );
        }
    }

    #[test]
    fn top_level_page_load_resets_only_for_main_origin_and_rejects_config_origin() {
        let server = server_origin("http://127.0.0.1:7910/");
        let main_url = url::Url::parse("http://127.0.0.1:7910/").expect("main URL");
        let config_url = url::Url::parse(
            "http://127.0.0.1:7911/?mode=panel&bridge=http%3A%2F%2F127.0.0.1%3A7910",
        )
        .expect("config URL");
        let blank_url = url::Url::parse("about:blank").expect("blank URL");

        assert_eq!(
            classify_top_level_load(&server, &main_url),
            TopLevelLoadAction::AllowMainAndResetCapability
        );
        assert_eq!(
            classify_top_level_load(&server, &config_url),
            TopLevelLoadAction::RejectAndReturnMain
        );
        assert_eq!(
            classify_top_level_load(&server, &blank_url),
            TopLevelLoadAction::IgnoreNonHttp
        );
    }

    #[test]
    fn non_http_and_unparseable_targets_are_left_to_the_webview() {
        let server = server_origin("http://127.0.0.1:7910/");
        for url in [
            "about:blank",
            "data:text/html,<p>hi</p>",
            "not a url",
            "/relative/path",
        ] {
            assert_eq!(
                classify_navigation_with_config(&server, None, url),
                Navigation::Allow,
                "{url}"
            );
        }
    }
}
