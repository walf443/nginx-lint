//! Two rules in one component: the `plugin-rules` world from Rust.
//!
//! Each rule is an ordinary [`Plugin`]; `export_component_plugins!` lists
//! them, and the host loads the component as two rules. The rules here
//! are the security builtins `server-tokens-enabled` and
//! `autoindex-enabled` cut down to their explicit-`on` cases, under
//! `-rs` names so they can sit beside the builtins, the way the other
//! SDKs' examples do.
//!
//! Build with:
//! ```sh
//! make build
//! ```

use nginx_lint_plugin::prelude::*;

/// Warns on an explicit `server_tokens on`
#[derive(Default)]
pub struct ServerTokensEnabled;

impl Plugin for ServerTokensEnabled {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "server-tokens-enabled-rs",
            "security",
            "Detects when server_tokens is enabled (exposes nginx version)",
        )
        .with_severity("warning")
        .with_why(
            "With server_tokens on, nginx puts its version number in the Server response \
             header and on default error pages, which tells an attacker which published \
             vulnerabilities to try.",
        )
        .with_bad_example(include_str!("../examples/server_tokens/bad.conf").trim())
        .with_good_example(include_str!("../examples/server_tokens/good.conf").trim())
    }

    fn relevant_directives(&self) -> Option<&'static [&'static str]> {
        Some(&["server_tokens"])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let err = self.spec().error_builder();
        config
            .all_directives_with_context()
            .filter(|ctx| ctx.is_inside("http"))
            .filter(|ctx| ctx.directive.is("server_tokens") && ctx.directive.first_arg_is("on"))
            .map(|ctx| {
                err.warning_at(
                    "server_tokens should be 'off' to hide nginx version",
                    ctx.directive,
                )
                .with_fix(ctx.directive.replace_with("server_tokens off;"))
            })
            .collect()
    }
}

/// Warns on `autoindex on`
#[derive(Default)]
pub struct AutoindexEnabled;

impl Plugin for AutoindexEnabled {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "autoindex-enabled-rs",
            "security",
            "Detects when autoindex is enabled (can expose directory contents)",
        )
        .with_severity("warning")
        .with_why(
            "With autoindex on, a request for a directory without an index file gets a \
             listing of the directory, which can expose backups and other files that were \
             not meant to be public.",
        )
        .with_bad_example(include_str!("../examples/autoindex/bad.conf").trim())
        .with_good_example(include_str!("../examples/autoindex/good.conf").trim())
    }

    fn relevant_directives(&self) -> Option<&'static [&'static str]> {
        Some(&["autoindex"])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let err = self.spec().error_builder();
        config
            .all_directives_with_context()
            .filter(|ctx| ctx.is_inside("http"))
            .filter(|ctx| ctx.directive.is("autoindex") && ctx.directive.first_arg_is("on"))
            .map(|ctx| {
                err.warning_at(
                    "autoindex is enabled, which can expose directory contents",
                    ctx.directive,
                )
                .with_fix(ctx.directive.replace_with("autoindex off;"))
            })
            .collect()
    }
}

nginx_lint_plugin::export_component_plugins!(ServerTokensEnabled, AutoindexEnabled);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::*;

    #[test]
    fn server_tokens_on_is_reported_and_fixed() {
        let runner = PluginTestRunner::new(ServerTokensEnabled);
        let errors = runner.check_string("http { server_tokens on; }").unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].rule, "server-tokens-enabled-rs");
        runner.assert_no_errors("http { server_tokens off; }");
    }

    #[test]
    fn autoindex_on_is_reported_outside_of_stream() {
        let runner = PluginTestRunner::new(AutoindexEnabled);
        runner.assert_errors("http { server { location / { autoindex on; } } }", 1);
        runner.assert_no_errors("stream { autoindex on; }");
    }
}
