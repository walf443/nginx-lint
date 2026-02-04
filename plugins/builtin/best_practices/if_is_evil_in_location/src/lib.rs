//! if-is-evil-in-location-in-location plugin
//!
//! This plugin warns when `if` blocks inside `location` contain directives
//! other than the safe ones: `return`, `rewrite ... last`, and `set`.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint::plugin_sdk::prelude::*;

/// Check for unsafe `if` usage in location blocks
#[derive(Default)]
pub struct IfIsEvilInLocationPlugin;

impl IfIsEvilInLocationPlugin {
    /// Check a block for unsafe `if` usage
    fn check_block(&self, items: &[ConfigItem], in_location: bool, errors: &mut Vec<LintError>) {
        for item in items {
            if let ConfigItem::Directive(directive) = item {
                // Track if we're inside a location block
                let is_location = directive.is("location");
                let now_in_location = in_location || is_location;

                // Check if blocks inside location
                if directive.is("if") && in_location {
                    if let Some(block) = &directive.block {
                        self.check_if_block(block, directive, errors);
                    }
                }

                // Recursively check nested blocks
                if let Some(block) = &directive.block {
                    // Don't mark `if` as entering a location context
                    let next_in_location = if directive.is("if") {
                        in_location
                    } else {
                        now_in_location
                    };
                    self.check_block(&block.items, next_in_location, errors);
                }
            }
        }
    }

    /// Check an `if` block for unsafe directives
    fn check_if_block(&self, block: &Block, if_directive: &Directive, errors: &mut Vec<LintError>) {
        let mut unsafe_directives: Vec<&str> = Vec::new();

        for item in &block.items {
            if let ConfigItem::Directive(directive) = item {
                let name = directive.name.as_str();

                // Check if directive is safe
                if !Self::is_safe_directive(directive) {
                    unsafe_directives.push(name);
                }
            }
        }

        if !unsafe_directives.is_empty() {
            let unsafe_list = unsafe_directives.join(", ");
            errors.push(LintError::warning(
                "if-is-evil-in-location",
                "best-practices",
                &format!(
                    "Avoid using '{}' inside 'if' in location context. \
                     Only 'return', 'rewrite ... last/break', 'set', and 'break' are safe. \
                     Consider using a separate location block instead.",
                    unsafe_list
                ),
                if_directive.span.start.line,
                if_directive.span.start.column,
            ));
        }
    }

    /// Check if a directive is safe to use inside `if`
    fn is_safe_directive(directive: &Directive) -> bool {
        let name = directive.name.as_str();

        match name {
            "return" | "set" | "break" => true,
            "rewrite" => {
                // rewrite is safe with 'last' or 'break' flag
                // rewrite pattern replacement [flag];
                // flags: last, break, redirect, permanent
                // - last: stops and restarts location search
                // - break: stops rewrite processing
                // - redirect/permanent: external redirects (unsafe in if)
                directive
                    .args
                    .last()
                    .map(|arg| {
                        let flag = arg.as_str();
                        flag == "last" || flag == "break"
                    })
                    .unwrap_or(false)
            }
            _ => false,
        }
    }
}

impl Plugin for IfIsEvilInLocationPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo::new(
            "if-is-evil-in-location",
            "best-practices",
            "Warns when 'if' blocks in location context contain unsafe directives",
        )
        .with_severity("warning")
        .with_why(
            "The 'if' directive in nginx has unexpected behavior when used with most directives \
             inside location blocks. This is a well-known issue documented in the nginx wiki \
             as 'If Is Evil'.\n\n\
             Only the following are 100% safe inside 'if' in location context:\n\
             - return ...\n\
             - rewrite ... last\n\
             - rewrite ... break\n\
             - set $var value\n\
             - break\n\n\
             Other directives like proxy_pass, try_files, fastcgi_pass, etc. can cause \
             unpredictable behavior. Use separate location blocks or map directive instead.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://github.com/nginxinc/nginx-wiki/blob/master/source/start/topics/depth/ifisevil.rst".to_string(),
            "https://www.getpagespeed.com/server-setup/nginx/nginx-if-is-evil-in-location".to_string(),
            "https://nginx.org/en/docs/http/ngx_http_rewrite_module.html#if".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        self.check_block(&config.items, false, &mut errors);
        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(IfIsEvilInLocationPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint::plugin_sdk::testing::PluginTestRunner;
    use nginx_lint::parse_string;

    #[test]
    fn test_unsafe_proxy_pass_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($request_uri ~* "\.php$") {
                proxy_pass http://php_backend;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
        assert!(errors[0].message.contains("proxy_pass"));
    }

    #[test]
    fn test_unsafe_try_files_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($slow) {
                try_files $uri $uri/ =404;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("try_files"));
    }

    #[test]
    fn test_safe_return_in_if() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            if ($request_method = POST) {
                return 405;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_safe_rewrite_last_in_if() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            if ($host ~* ^www\.) {
                rewrite ^(.*)$ https://example.com$1 last;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_safe_rewrite_break_in_if() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        // rewrite with 'break' is also safe
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            if ($host ~* ^www\.) {
                rewrite ^(.*)$ /new-path$1 break;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_safe_break_directive_in_if() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        // standalone 'break' directive is safe
        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            if ($uri ~* "^/stop") {
                set $stop 1;
                break;
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_safe_set_in_if() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            set $backend "default";
            if ($uri ~* ^/api/) {
                set $backend "api_server";
            }
            proxy_pass http://$backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_if_in_server_context_is_ok() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        // if in server context (not location) should not trigger warning
        runner.assert_no_errors(
            r#"
http {
    server {
        if ($host = 'www.example.com') {
            return 301 https://example.com$request_uri;
        }

        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_multiple_unsafe_directives() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($slow) {
                limit_rate 10k;
                proxy_pass http://backend;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("limit_rate"));
        assert!(errors[0].message.contains("proxy_pass"));
    }

    #[test]
    fn test_nested_location_with_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            location /nested {
                if ($slow) {
                    fastcgi_pass unix:/var/run/php-fpm.sock;
                }
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("fastcgi_pass"));
    }

    #[test]
    fn test_safe_combination_return_and_set() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            if ($http_x_debug) {
                set $debug 1;
                return 200 "debug mode";
            }
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    // =========================================================================
    // Additional unsafe directive tests
    // =========================================================================

    #[test]
    fn test_unsafe_fastcgi_pass_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location ~ \.php$ {
            if ($request_uri ~* "admin") {
                fastcgi_pass unix:/var/run/php-fpm-admin.sock;
            }
            fastcgi_pass unix:/var/run/php-fpm.sock;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("fastcgi_pass"));
    }

    #[test]
    fn test_unsafe_uwsgi_pass_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($arg_version = "v2") {
                uwsgi_pass unix:/var/run/uwsgi-v2.sock;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("uwsgi_pass"));
    }

    #[test]
    fn test_unsafe_grpc_pass_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($http_x_grpc_service = "service-b") {
                grpc_pass grpc://service-b:50051;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("grpc_pass"));
    }

    #[test]
    fn test_unsafe_add_header_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($request_uri ~* "^/api/") {
                add_header X-API true;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("add_header"));
    }

    #[test]
    fn test_unsafe_root_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($http_host ~* "^mobile\.") {
                root /var/www/mobile;
            }
            root /var/www/html;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("root"));
    }

    #[test]
    fn test_unsafe_alias_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location /images/ {
            if ($http_accept ~* "webp") {
                alias /var/www/images-webp/;
            }
            alias /var/www/images/;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("alias"));
    }

    #[test]
    fn test_unsafe_index_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($http_accept_language ~* "^ja") {
                index index.ja.html;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("index"));
    }

    #[test]
    fn test_unsafe_expires_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($request_uri ~* "\.(jpg|png|gif)$") {
                expires 30d;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("expires"));
    }

    #[test]
    fn test_unsafe_access_log_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($http_user_agent ~* "bot") {
                access_log off;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("access_log"));
    }

    #[test]
    fn test_unsafe_auth_basic_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($remote_addr !~* "^192\.168\.") {
                auth_basic "Restricted";
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("auth_basic"));
    }

    #[test]
    fn test_unsafe_proxy_set_header_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($http_x_forwarded_proto = "https") {
                proxy_set_header X-Forwarded-Proto https;
            }
            proxy_pass http://backend;
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("proxy_set_header"));
    }

    // =========================================================================
    // Rewrite flag variations
    // =========================================================================

    #[test]
    fn test_unsafe_rewrite_permanent_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($host = "old.example.com") {
                rewrite ^(.*)$ https://new.example.com$1 permanent;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("rewrite"));
    }

    #[test]
    fn test_unsafe_rewrite_redirect_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($scheme = "http") {
                rewrite ^(.*)$ https://$host$1 redirect;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("rewrite"));
    }

    #[test]
    fn test_unsafe_rewrite_no_flag_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($uri ~* "^/old/") {
                rewrite ^/old/(.*)$ /new/$1;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("rewrite"));
    }

    // =========================================================================
    // Multiple if blocks and edge cases
    // =========================================================================

    #[test]
    fn test_multiple_if_blocks_with_unsafe() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($slow) {
                limit_rate 10k;
            }
            if ($arg_nocache) {
                add_header Cache-Control "no-cache";
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        assert_eq!(errors.len(), 2, "Expected 2 errors, got: {:?}", errors);
    }

    #[test]
    fn test_if_in_server_with_unsafe_is_ok() {
        // if in server context (not location) should not trigger warning
        // even with "unsafe" directives
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        if ($host = 'www.example.com') {
            # These would be unsafe in location, but OK in server context
            add_header X-Redirect true;
            return 301 https://example.com$request_uri;
        }

        location / {
            proxy_pass http://backend;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_safe_return_with_body() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location /health {
            if ($arg_format = "json") {
                return 200 '{"status": "ok"}';
            }
            return 200 "OK";
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_safe_multiple_set_directives() {
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            set $backend "default";
            set $timeout "30s";

            if ($uri ~* ^/api/) {
                set $backend "api_server";
                set $timeout "60s";
            }

            proxy_pass http://$backend;
            proxy_read_timeout $timeout;
        }
    }
}
"#,
        );
    }

    #[test]
    fn test_mixed_safe_and_unsafe_in_if() {
        let config = parse_string(
            r#"
http {
    server {
        location / {
            if ($slow) {
                set $rate_limit 1;
                limit_rate 10k;
                return 200;
            }
        }
    }
}
"#,
        )
        .unwrap();

        let plugin = IfIsEvilInLocationPlugin;
        let errors = plugin.check(&config, "test.conf");

        // Should only report limit_rate, not set or return
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("limit_rate"));
        // Check that 'set' and 'return' are not in the unsafe directives list (before "inside 'if'")
        let unsafe_part = errors[0].message.split("inside 'if'").next().unwrap();
        assert!(!unsafe_part.contains("'set'"), "set should not be reported as unsafe");
        assert!(!unsafe_part.contains("'return'"), "return should not be reported as unsafe");
    }

    #[test]
    fn test_limit_except_inside_location_not_confused_with_if() {
        // limit_except is a block directive, not if, so should not trigger
        let runner = PluginTestRunner::new(IfIsEvilInLocationPlugin);

        runner.assert_no_errors(
            r#"
http {
    server {
        location / {
            limit_except GET {
                deny all;
            }
        }
    }
}
"#,
        );
    }
}
