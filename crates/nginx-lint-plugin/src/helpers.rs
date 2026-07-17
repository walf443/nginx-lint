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
/// use nginx_lint_plugin::helpers::is_domain_name;
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
/// use nginx_lint_plugin::helpers::is_ipv4_address;
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
/// use nginx_lint_plugin::helpers::extract_host_from_url;
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
    let after_protocol = {
        let pos = url.find("://")?;
        &url[pos + 3..]
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
/// use nginx_lint_plugin::helpers::extract_domain;
///
/// assert_eq!(extract_domain("example.com"), "example.com");
/// assert_eq!(extract_domain("example.com:8080"), "example.com");
/// assert_eq!(extract_domain("localhost:3000"), "localhost");
/// ```
pub fn extract_domain(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

/// Find byte offsets of `(` characters that open an unnamed PCRE capture group.
///
/// nginx stores unnamed captures in the shared `$1`..`$9` slots, so rules that
/// care about capture collisions need to tell a real capture group apart from a
/// paren that merely looks like one.
///
/// Skips: escapes (`\(`), `\Q...\E` literal spans, character classes (`[...]`,
/// including a leading `]` member), the `(?...)` family — non-capturing
/// `(?:...)`, named `(?<name>...)` / `(?P<name>...)`, lookarounds `(?=...)` /
/// `(?!...)` / `(?<=...)` / `(?<!...)`, atomic `(?>...)`, comments `(?#...)`,
/// inline modifiers `(?i)`, and conditionals `(?(1)...)` — and the `(*VERB)`
/// family — PCRE control verbs like `(*PRUNE)`, `(*SKIP)`, `(*FAIL)`,
/// `(*MARK:name)`, etc.
///
/// # Examples
///
/// ```
/// use nginx_lint_plugin::helpers::find_unnamed_capture_positions;
///
/// assert_eq!(find_unnamed_capture_positions("^/api/(.*)$"), vec![6]);
/// assert_eq!(find_unnamed_capture_positions("(a)(b)(?:c)(d)"), vec![0, 3, 11]);
///
/// // Not unnamed captures
/// assert!(find_unnamed_capture_positions("^/(?<name>.*)$").is_empty());
/// assert!(find_unnamed_capture_positions(r"\(literal\)").is_empty());
/// assert!(find_unnamed_capture_positions("[()]").is_empty());
/// assert!(find_unnamed_capture_positions("(*PRUNE)").is_empty());
///
/// // `]` first in a class is a member, so these parens are still inside it
/// assert!(find_unnamed_capture_positions("[]()]").is_empty());
/// assert!(find_unnamed_capture_positions("[^]()]").is_empty());
/// assert!(find_unnamed_capture_positions("[[:alpha:]()]").is_empty());
///
/// // `\Q...\E` makes its contents literal
/// assert!(find_unnamed_capture_positions(r"\Q(a)\E").is_empty());
///
/// // The inner paren of a conditional is syntax, not a group
/// assert_eq!(find_unnamed_capture_positions("(a)(?(1)x|y)"), vec![0]);
///
/// // A `(?#...)` comment body is opaque text — parens in it are not groups,
/// // and a `[` or `\Q` in it must not swallow the captures that follow
/// assert!(find_unnamed_capture_positions("^/a(?#()$").is_empty());
/// assert_eq!(find_unnamed_capture_positions("(?# [ )(a)"), vec![7]);
///
/// // Negated POSIX classes are classes too
/// assert!(find_unnamed_capture_positions("[[:^print:]()]").is_empty());
/// ```
pub fn find_unnamed_capture_positions(regex: &str) -> Vec<usize> {
    let bytes = regex.as_bytes();
    let mut positions = Vec::new();
    let mut i = 0;
    let mut in_char_class = false;

    // Byte offset just past `[` (and an optional negating `^`), where a `]` is
    // a literal member rather than the class terminator.
    let mut class_body_start = 0;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'\\' && i + 1 < bytes.len() {
            // `\Q...\E` quotes everything up to `\E` (or end of pattern).
            if bytes[i + 1] == b'Q' {
                i = find_literal_span_end(bytes, i + 2);
                continue;
            }
            // Any other escape — skip both bytes.
            i += 2;
            continue;
        }

        if in_char_class {
            // A POSIX class such as `[:alpha:]` nests inside the class, and its
            // `]` does not end the enclosing one.
            if b == b'['
                && bytes.get(i + 1) == Some(&b':')
                && let Some(end) = find_posix_class_end(bytes, i + 2)
            {
                i = end;
                continue;
            }
            // PCRE reads `]` as a member when it opens the class body, so only
            // a later one closes it. `[]()]`, `[^]()]` are single classes.
            if b == b']' && i > class_body_start {
                in_char_class = false;
            }
            i += 1;
            continue;
        }

        if b == b'[' {
            in_char_class = true;
            class_body_start = i + 1;
            if bytes.get(class_body_start) == Some(&b'^') {
                class_body_start += 1;
            }
            i += 1;
            continue;
        }

        if b == b'(' {
            match bytes.get(i + 1).copied() {
                // `(*VERB)` / `(*MARK:name)` — never a capture, and the name
                // is arbitrary text, so step over the whole thing rather than
                // scanning it as regex.
                Some(b'*') => {
                    i = find_unescaped_close_paren(bytes, i + 2);
                    continue;
                }
                Some(b'?') => match bytes.get(i + 2).copied() {
                    // `(?#...)` is a comment: its body is arbitrary text and
                    // runs to the very next `)`, with no escaping. Scanning it
                    // as regex both invents groups and — via a stray `[` or
                    // `\Q` — swallows the real ones after it.
                    Some(b'#') => {
                        i = find_unescaped_close_paren(bytes, i + 3);
                        continue;
                    }
                    // A conditional `(?(1)...)` / `(?(<name>)...)` nests a
                    // paren that is syntax, not a group.
                    Some(b'(') => {
                        i += 3;
                        continue;
                    }
                    // Every other `(?...)` construct is not a capture, but its
                    // body is regex and must still be scanned.
                    _ => {}
                },
                _ => positions.push(i),
            }
        }

        i += 1;
    }

    positions
}

/// Offset just past the next `)` at or after `from`, or the end of the input
/// when there is none. Used for constructs whose body is opaque text — a
/// `(?#...)` comment or a `(*VERB:name)` — which PCRE ends at the first `)`.
fn find_unescaped_close_paren(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b')' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// Offset just past the `:]` closing a POSIX class whose body starts at
/// `from`, or `None` when there is none — in which case the `[` was an
/// ordinary member and should be treated as such.
fn find_posix_class_end(bytes: &[u8], from: usize) -> Option<usize> {
    // `[[:^alpha:]]` negates the class; the `^` is part of the syntax, not the
    // name. Missing it ended the class at the `:]` and exposed everything after
    // it as regex.
    let mut i = from + usize::from(bytes.get(from) == Some(&b'^'));

    while i + 1 < bytes.len() {
        if bytes[i] == b':' && bytes[i + 1] == b']' {
            return Some(i + 2);
        }
        // A POSIX class name is alphabetic; anything else means this was not
        // one, e.g. a literal `[` followed by `:` in the class.
        if !bytes[i].is_ascii_alphabetic() {
            return None;
        }
        i += 1;
    }
    None
}

/// Offset just past the `\E` closing a `\Q` literal span that starts at
/// `from`, or the end of the pattern when it is unterminated.
fn find_literal_span_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' && bytes[i + 1] == b'E' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

/// Whether a regex source string contains at least one unnamed PCRE capture
/// group. See [`find_unnamed_capture_positions`] for what counts as one.
///
/// # Examples
///
/// ```
/// use nginx_lint_plugin::helpers::has_unnamed_capture;
///
/// assert!(has_unnamed_capture("^/old/(.*)$"));
/// assert!(!has_unnamed_capture("^/old/(?<rest>.*)$"));
/// assert!(!has_unnamed_capture("^/old/(?:.*)$"));
/// ```
pub fn has_unnamed_capture(regex: &str) -> bool {
    !find_unnamed_capture_positions(regex).is_empty()
}

/// Detect whether a regex source string contains any named capture group
/// (`(?<name>...)` or `(?P<name>...)`). Lookbehinds `(?<=...)` / `(?<!...)`
/// are NOT named captures and don't count.
///
/// # Examples
///
/// ```
/// use nginx_lint_plugin::helpers::regex_has_named_capture;
///
/// assert!(regex_has_named_capture("(?<name>.*)"));
/// assert!(regex_has_named_capture("(?P<name>.*)"));
///
/// assert!(!regex_has_named_capture("(.*)"));
/// assert!(!regex_has_named_capture("(?<=foo)bar"));
/// assert!(!regex_has_named_capture("(?<!foo)bar"));
/// ```
pub fn regex_has_named_capture(regex: &str) -> bool {
    let bytes = regex.as_bytes();
    let mut i = 0;
    let mut in_char_class = false;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }

        if in_char_class {
            if b == b']' {
                in_char_class = false;
            }
            i += 1;
            continue;
        }

        if b == b'[' {
            in_char_class = true;
            i += 1;
            continue;
        }

        if b == b'(' && i + 3 < bytes.len() && bytes[i + 1] == b'?' {
            // `(?<name>...)` — named iff the char after `<` is a name-start
            // byte. `(?<=...)` and `(?<!...)` are lookbehinds; not captures.
            if bytes[i + 2] == b'<' {
                let after_lt = bytes[i + 3];
                if after_lt != b'=' && after_lt != b'!' {
                    return true;
                }
            }
            // `(?'name'...)` — the quoted form. Missing it let callers treat a
            // named group as unnamed, which silently misnumbers the rest.
            if bytes[i + 2] == b'\'' {
                return true;
            }
            // `(?P<name>...)` — Python-style named capture.
            // (Only `bytes[i + 3]` is read; the outer `i + 3 < bytes.len()`
            // guard is already sufficient.)
            if bytes[i + 2] == b'P' && bytes[i + 3] == b'<' {
                return true;
            }
        }

        i += 1;
    }

    false
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

    /// PCRE reads `]` as a member when it opens the class body, so the class
    /// runs on past it. Terminating there made the following literal parens
    /// look like groups.
    #[test]
    fn class_with_leading_bracket_member_is_not_a_capture() {
        assert!(find_unnamed_capture_positions("[]()]").is_empty());
        assert!(find_unnamed_capture_positions("[^]()]").is_empty());
        assert!(find_unnamed_capture_positions("[[:alpha:]()]").is_empty());
        // A `]` that is not first still closes the class.
        assert_eq!(find_unnamed_capture_positions("[abc](d)"), vec![5]);
    }

    /// `\Q...\E` quotes its contents, so parens inside are literal.
    #[test]
    fn quoted_literal_span_is_not_scanned() {
        assert!(find_unnamed_capture_positions(r"\Q(a)\E").is_empty());
        assert_eq!(find_unnamed_capture_positions(r"\Q(a)\E(b)"), vec![7]);
        // Unterminated `\Q` quotes to the end.
        assert!(find_unnamed_capture_positions(r"\Q(a)").is_empty());
    }

    /// The paren nested in a conditional is syntax, not a group.
    #[test]
    fn conditional_inner_paren_is_not_a_capture() {
        assert_eq!(find_unnamed_capture_positions("(a)(?(1)x|y)"), vec![0]);
        assert!(find_unnamed_capture_positions("(?(<n>)x|y)").is_empty());
    }

    /// A `(?#...)` comment body is opaque text ending at the first `)`.
    /// Scanning it as regex invented groups, and a stray `[` or `\Q` in the
    /// prose swallowed every real capture after it.
    #[test]
    fn comment_body_is_skipped() {
        // `(` in the prose is not a group.
        assert!(find_unnamed_capture_positions("^/a(?#()$").is_empty());
        // A capture after such a comment is still found.
        assert_eq!(find_unnamed_capture_positions("^/a(?#()/(.*)$"), vec![9]);
        // `[` / `\Q` in the prose must not swallow what follows.
        assert_eq!(find_unnamed_capture_positions("(?# [ )(a)"), vec![7]);
        assert_eq!(find_unnamed_capture_positions(r"(?#\Q)(a)"), vec![6]);
        assert_eq!(find_unnamed_capture_positions("(?#c)(a)"), vec![5]);
    }

    /// `(*MARK:name)` carries arbitrary text; a `[` in it must not swallow the
    /// rest of the pattern.
    #[test]
    fn verb_name_is_skipped() {
        assert_eq!(find_unnamed_capture_positions("(*MARK:[)(a)"), vec![9]);
        assert!(find_unnamed_capture_positions("(*PRUNE)").is_empty());
    }

    /// `[[:^alpha:]]` negates the class — the `^` is syntax, not the name.
    /// Missing it closed the class at the `:]`.
    #[test]
    fn negated_posix_class_does_not_end_the_class() {
        assert!(find_unnamed_capture_positions("[[:^print:]()]").is_empty());
        assert!(find_unnamed_capture_positions("[[:^alpha:]]").is_empty());
        // Still finds a real capture after the class.
        assert_eq!(find_unnamed_capture_positions("[[:^print:]](a)"), vec![12]);
    }

    /// PCRE has three named-capture syntaxes; treating `(?'n'...)` as unnamed
    /// made callers renumber the groups around it.
    #[test]
    fn quoted_named_capture_is_recognised() {
        assert!(regex_has_named_capture("(?'a'x)"));
        assert!(regex_has_named_capture("(?'a'x)(y)"));
        // Not a named capture.
        assert!(!regex_has_named_capture("(x)"));
    }
}
