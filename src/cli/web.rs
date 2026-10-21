use std::process::ExitCode;

#[cfg(feature = "web-server")]
pub fn run_web(host: &str, port: u16, open_browser: bool) -> ExitCode {
    use tiny_http::{Response, Server};

    // Embedded web HTML
    const INDEX_HTML: &str = include_str!("../../web/index.html");
    const RULES_HTML: &str = include_str!("../../web/rules.html");

    // When web-server-embed-wasm feature is enabled, embed the WASM files
    #[cfg(feature = "web-server-embed-wasm")]
    const NGINX_LINT_JS: &str = include_str!("../../web/pkg/nginx_lint.js");
    #[cfg(feature = "web-server-embed-wasm")]
    const NGINX_LINT_WASM: &[u8] = include_bytes!("../../web/pkg/nginx_lint_bg.wasm");

    // Check that the files served from disk exist (only when not embedding)
    #[cfg(not(feature = "web-server-embed-wasm"))]
    {
        if let Some(missing) = [JS_PATH, WASM_PATH]
            .into_iter()
            .find(|path| !std::path::Path::new(path).is_file())
        {
            eprintln!("Error: {} not found.", missing);
            eprintln!();
            eprintln!("Please build the WASM package first:");
            eprintln!(
                "  wasm-pack build --target web --out-dir web/pkg --no-default-features --features wasm"
            );
            eprintln!();
            eprintln!("Then run again, or embed it in the binary:");
            eprintln!("  cargo build --features web-server-embed-wasm");
            return ExitCode::from(2);
        }
    }

    let addr = socket_addr(host, port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error starting server: {}", e);
            return ExitCode::from(2);
        }
    };

    let url = browser_url(host, port);
    if !is_loopback(host) {
        eprintln!(
            "Warning: listening on {}; anyone who can reach this address can use the server",
            addr
        );
    }
    eprintln!("Starting nginx-lint web server at {}", url);
    #[cfg(feature = "web-server-embed-wasm")]
    eprintln!("(WASM embedded in binary)");
    eprintln!("Press Ctrl+C to stop");

    if open_browser {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &url])
            .spawn();
    }

    for request in server.incoming_requests() {
        let response = match route(request.url()) {
            Some(Asset::Index) => Response::from_string(INDEX_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            ),
            Some(Asset::Rules) => Response::from_string(RULES_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            ),
            Some(Asset::Js) => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    Response::from_string(NGINX_LINT_JS).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/javascript"[..],
                        )
                        .unwrap(),
                    )
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    serve_file_from_disk(JS_PATH, "application/javascript")
                }
            }
            Some(Asset::Wasm) => {
                #[cfg(feature = "web-server-embed-wasm")]
                {
                    Response::from_data(NGINX_LINT_WASM.to_vec()).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/wasm"[..],
                        )
                        .unwrap(),
                    )
                }
                #[cfg(not(feature = "web-server-embed-wasm"))]
                {
                    serve_file_from_disk(WASM_PATH, "application/wasm")
                }
            }
            None => Response::from_string("Not Found").with_status_code(404),
        };

        let _ = request.respond(response);
    }

    ExitCode::SUCCESS
}

/// Where the pkg files are read from when they are not embedded.
#[cfg(all(feature = "web-server", not(feature = "web-server-embed-wasm")))]
const JS_PATH: &str = "web/pkg/nginx_lint.js";
#[cfg(all(feature = "web-server", not(feature = "web-server-embed-wasm")))]
const WASM_PATH: &str = "web/pkg/nginx_lint_bg.wasm";

/// The files the server can return.
#[cfg_attr(not(feature = "web-server"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
enum Asset {
    Index,
    Rules,
    Js,
    Wasm,
}

/// Map a request target to the asset it names, or None for 404.
///
/// A fixed table rather than a path joined onto the filesystem: tiny_http
/// hands over the raw request target, with no decoding or dot-segment
/// removal, so `/pkg/../..` would reach any file the user can read.
#[cfg_attr(not(feature = "web-server"), allow(dead_code))]
fn route(target: &str) -> Option<Asset> {
    match target {
        "/" | "/index.html" => Some(Asset::Index),
        "/rules" | "/rules.html" => Some(Asset::Rules),
        "/pkg/nginx_lint.js" => Some(Asset::Js),
        "/pkg/nginx_lint_bg.wasm" => Some(Asset::Wasm),
        _ => None,
    }
}

/// `--host` without the brackets an IPv6 address may be written in.
#[cfg_attr(not(feature = "web-server"), allow(dead_code))]
fn bare_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host)
}

/// `host:port`, bracketing an IPv6 address.
#[cfg_attr(not(feature = "web-server"), allow(dead_code))]
fn socket_addr(host: &str, port: u16) -> String {
    let host = bare_host(host);
    if host.contains(':') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

/// Whether `host` only accepts connections from this machine.
#[cfg_attr(not(feature = "web-server"), allow(dead_code))]
fn is_loopback(host: &str) -> bool {
    let host = bare_host(host);
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}

/// The URL to print and open: localhost when listening on every address,
/// otherwise the address listened on, which may be the only one that works.
#[cfg_attr(not(feature = "web-server"), allow(dead_code))]
fn browser_url(host: &str, port: u16) -> String {
    let unspecified = bare_host(host)
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_unspecified());
    if unspecified {
        format!("http://localhost:{}", port)
    } else {
        format!("http://{}", socket_addr(host, port))
    }
}

#[cfg(all(feature = "web-server", not(feature = "web-server-embed-wasm")))]
fn serve_file_from_disk(
    file_path: &str,
    content_type: &str,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    use tiny_http::Response;
    match std::fs::read(file_path) {
        Ok(content) => Response::from_data(content).with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
        ),
        Err(_) => Response::from_string("Not Found").with_status_code(404),
    }
}

#[cfg(not(feature = "web-server"))]
pub fn run_web(_host: &str, _port: u16, _open_browser: bool) -> ExitCode {
    eprintln!("Error: Web server feature is not enabled.");
    eprintln!();
    eprintln!("Rebuild with the web-server feature:");
    eprintln!("  cargo build --features web-server");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_serves_only_the_fixed_assets() {
        assert_eq!(route("/"), Some(Asset::Index));
        assert_eq!(route("/index.html"), Some(Asset::Index));
        assert_eq!(route("/rules"), Some(Asset::Rules));
        assert_eq!(route("/pkg/nginx_lint.js"), Some(Asset::Js));
        assert_eq!(route("/pkg/nginx_lint_bg.wasm"), Some(Asset::Wasm));

        for target in [
            "/pkg/package.json",
            "/pkg/nginx_lint.d.ts",
            "/pkg/../index.html",
            "/pkg/../../Cargo.toml",
            "/pkg/%2e%2e/%2e%2e/Cargo.toml",
            "/pkg/..%2f..%2fCargo.toml",
            "/pkg/nginx_lint.js/../../../Cargo.toml",
            "/web/pkg/nginx_lint.js",
        ] {
            assert_eq!(route(target), None, "{}", target);
        }
    }

    #[test]
    fn socket_addr_brackets_ipv6_once() {
        assert_eq!(socket_addr("127.0.0.1", 8080), "127.0.0.1:8080");
        assert_eq!(socket_addr("localhost", 8080), "localhost:8080");
        assert_eq!(socket_addr("::1", 8080), "[::1]:8080");
        assert_eq!(socket_addr("[::1]", 8080), "[::1]:8080");
    }

    #[test]
    fn is_loopback_parses_the_address() {
        for host in [
            "127.0.0.1",
            "127.0.0.2",
            "::1",
            "[::1]",
            "0:0:0:0:0:0:0:1",
            "localhost",
            "LOCALHOST",
        ] {
            assert!(is_loopback(host), "{}", host);
        }
        for host in ["0.0.0.0", "::", "[::]", "192.168.1.5", "example.com"] {
            assert!(!is_loopback(host), "{}", host);
        }
    }

    #[test]
    fn browser_url_names_the_address_listened_on() {
        assert_eq!(browser_url("127.0.0.1", 8080), "http://127.0.0.1:8080");
        assert_eq!(browser_url("192.168.1.5", 8080), "http://192.168.1.5:8080");
        assert_eq!(browser_url("[::1]", 8080), "http://[::1]:8080");
        assert_eq!(browser_url("0.0.0.0", 8080), "http://localhost:8080");
        assert_eq!(browser_url("::", 8080), "http://localhost:8080");
    }
}
