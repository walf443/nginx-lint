//! Helper functions for plugin development
//!
//! This module provides common utilities for nginx configuration linting.

/// Check if the given host is a domain name (not an IP address or special value)
///
/// Returns `true` for domain names like `example.com`, `api.backend.internal`, `localhost`
/// Returns `false` for IP addresses, unix sockets, variables, or upstream names without dots
///
/// # Examples
///
/// ```
/// use nginx_lint::plugin_sdk::helpers::is_domain_name;
///
/// // Domain names
/// assert!(is_domain_name("example.com"));
/// assert!(is_domain_name("api.example.com"));
/// assert!(is_domain_name("localhost"));
/// assert!(is_domain_name("backend.internal"));
/// assert!(is_domain_name("example.com:8080"));
///
/// // Not domain names
/// assert!(!is_domain_name("127.0.0.1"));
/// assert!(!is_domain_name("127.0.0.1:8080"));
/// assert!(!is_domain_name("[::1]"));
/// assert!(!is_domain_name("[::1]:8080"));
/// assert!(!is_domain_name("$backend"));
/// assert!(!is_domain_name("unix:/var/run/app.sock"));
/// assert!(!is_domain_name("backend")); // upstream name without dots
/// ```
pub fn is_domain_name(host: &str) -> bool {
    // Empty host
    if host.is_empty() {
        return false;
    }

    // Variable reference (e.g., $backend)
    if host.starts_with('$') {
        return false;
    }

    // Unix socket
    if host.starts_with("unix:") {
        return false;
    }

    // IPv6 address (e.g., [::1])
    if host.starts_with('[') && host.contains(']') {
        return false;
    }

    // IPv4 address (all parts are numeric)
    let host_without_port = host.split(':').next().unwrap_or(host);
    if is_ipv4_address(host_without_port) {
        return false;
    }

    // localhost or contains a dot (domain name)
    host_without_port == "localhost" || host_without_port.contains('.')
}

/// Check if the string is a valid IPv4 address
///
/// # Examples
///
/// ```
/// use nginx_lint::plugin_sdk::helpers::is_ipv4_address;
///
/// assert!(is_ipv4_address("127.0.0.1"));
/// assert!(is_ipv4_address("192.168.1.1"));
/// assert!(is_ipv4_address("0.0.0.0"));
/// assert!(is_ipv4_address("255.255.255.255"));
///
/// assert!(!is_ipv4_address("example.com"));
/// assert!(!is_ipv4_address("127.0.0.1.1"));
/// assert!(!is_ipv4_address("256.0.0.1"));
/// assert!(!is_ipv4_address("localhost"));
/// ```
pub fn is_ipv4_address(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// Extract host from a proxy_pass URL
///
/// Extracts the host (and optional port) from a URL like `http://example.com:8080/path`
///
/// # Examples
///
/// ```
/// use nginx_lint::plugin_sdk::helpers::extract_host_from_url;
///
/// assert_eq!(extract_host_from_url("http://example.com"), Some("example.com"));
/// assert_eq!(extract_host_from_url("http://example.com:8080"), Some("example.com:8080"));
/// assert_eq!(extract_host_from_url("http://example.com/path"), Some("example.com"));
/// assert_eq!(extract_host_from_url("https://api.example.com:443/api/v1"), Some("api.example.com:443"));
/// assert_eq!(extract_host_from_url("http://[::1]:8080/path"), Some("[::1]:8080"));
/// assert_eq!(extract_host_from_url("http://unix:/var/run/app.sock"), Some("unix:/var/run/app.sock"));
///
/// // No protocol
/// assert_eq!(extract_host_from_url("example.com"), None);
/// assert_eq!(extract_host_from_url("backend"), None);
/// ```
pub fn extract_host_from_url(url: &str) -> Option<&str> {
    // Remove protocol
    let after_protocol = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        return None;
    };

    // Handle unix socket URLs (e.g., "unix:/var/run/app.sock")
    // The entire "unix:/path/to/socket" is the host
    if after_protocol.starts_with("unix:") {
        return Some(after_protocol);
    }

    // Remove path
    let host_and_port = if let Some(pos) = after_protocol.find('/') {
        &after_protocol[..pos]
    } else {
        after_protocol
    };

    Some(host_and_port)
}

/// Extract domain name (without port) from a host string
///
/// # Examples
///
/// ```
/// use nginx_lint::plugin_sdk::helpers::extract_domain;
///
/// assert_eq!(extract_domain("example.com"), "example.com");
/// assert_eq!(extract_domain("example.com:8080"), "example.com");
/// assert_eq!(extract_domain("localhost:3000"), "localhost");
/// ```
pub fn extract_domain(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_domain_name() {
        // Should be detected as domain
        assert!(is_domain_name("example.com"));
        assert!(is_domain_name("api.example.com"));
        assert!(is_domain_name("localhost"));
        assert!(is_domain_name("backend.internal"));
        assert!(is_domain_name("example.com:8080"));

        // Should NOT be detected as domain
        assert!(!is_domain_name("127.0.0.1"));
        assert!(!is_domain_name("127.0.0.1:8080"));
        assert!(!is_domain_name("[::1]"));
        assert!(!is_domain_name("[::1]:8080"));
        assert!(!is_domain_name("$backend"));
        assert!(!is_domain_name("unix:/var/run/app.sock"));
        assert!(!is_domain_name("backend")); // upstream name without dots
        assert!(!is_domain_name(""));
    }

    #[test]
    fn test_is_ipv4_address() {
        assert!(is_ipv4_address("127.0.0.1"));
        assert!(is_ipv4_address("192.168.1.1"));
        assert!(is_ipv4_address("0.0.0.0"));
        assert!(is_ipv4_address("255.255.255.255"));

        assert!(!is_ipv4_address("example.com"));
        assert!(!is_ipv4_address("127.0.0.1.1"));
        assert!(!is_ipv4_address("256.0.0.1"));
        assert!(!is_ipv4_address("localhost"));
        assert!(!is_ipv4_address(""));
    }

    #[test]
    fn test_extract_host_from_url() {
        assert_eq!(
            extract_host_from_url("http://example.com"),
            Some("example.com")
        );
        assert_eq!(
            extract_host_from_url("http://example.com:8080"),
            Some("example.com:8080")
        );
        assert_eq!(
            extract_host_from_url("http://example.com/path"),
            Some("example.com")
        );
        assert_eq!(
            extract_host_from_url("https://api.example.com:443/api/v1"),
            Some("api.example.com:443")
        );
        assert_eq!(
            extract_host_from_url("http://[::1]:8080/path"),
            Some("[::1]:8080")
        );
        assert_eq!(
            extract_host_from_url("http://unix:/var/run/app.sock"),
            Some("unix:/var/run/app.sock")
        );

        // No protocol
        assert_eq!(extract_host_from_url("example.com"), None);
        assert_eq!(extract_host_from_url("backend"), None);
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("example.com"), "example.com");
        assert_eq!(extract_domain("example.com:8080"), "example.com");
        assert_eq!(extract_domain("localhost:3000"), "localhost");
        assert_eq!(extract_domain("127.0.0.1:80"), "127.0.0.1");
    }
}
