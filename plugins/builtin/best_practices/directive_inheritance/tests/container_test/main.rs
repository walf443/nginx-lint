//! Container-based integration tests for the directive-inheritance rule.
//!
//! Verifies that directives in child blocks completely override parent block
//! directives - they are NOT inherited in nginx.
//!
//! Run with:
//!   cargo test -p directive-inheritance-plugin --test container_test -- --ignored
//!
//! Specify nginx version via environment variable (default: "1.27"):
//!   NGINX_VERSION=1.26 cargo test -p directive-inheritance-plugin --test container_test -- --ignored

mod add_header;
mod fastcgi_param;
mod proxy_set_header;
