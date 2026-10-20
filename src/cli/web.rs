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

    // Check if pkg directory exists (only when not embedding)
    #[cfg(not(feature = "web-server-embed-wasm"))]
    {
        let pkg_dir = std::path::Path::new("web/pkg");
        if !pkg_dir.exists() {
            eprintln!("Error: web/pkg/ directory not found.");
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

    // A bare IPv6 address needs brackets to be followed by a port.
    let addr = if host.contains(':') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    };
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error starting server: {}", e);
            return ExitCode::from(2);
        }
    };

    let url = format!("http://localhost:{}", port);
    if host != "127.0.0.1" && host != "localhost" && host != "::1" {
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
        let url = request.url();
        let response = match url {
            "/" | "/index.html" => Response::from_string(INDEX_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            ),
            "/rules" | "/rules.html" => Response::from_string(RULES_HTML).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap(),
            ),
            "/pkg/nginx_lint.js" => {
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
                    serve_file_from_disk("./web/pkg/nginx_lint.js", "application/javascript")
                }
            }
            "/pkg/nginx_lint_bg.wasm" => {
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
                    serve_file_from_disk("./web/pkg/nginx_lint_bg.wasm", "application/wasm")
                }
            }
            _ => Response::from_string("Not Found").with_status_code(404),
        };

        let _ = request.respond(response);
    }

    ExitCode::SUCCESS
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
