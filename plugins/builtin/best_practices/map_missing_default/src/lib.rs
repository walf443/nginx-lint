//! map-missing-default plugin
//!
//! This plugin warns when a `map` block does not contain a `default` entry.
//! Without a `default`, unmatched values silently become empty strings,
//! which can cause hard-to-diagnose bugs.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use nginx_lint_plugin::prelude::*;

/// Check if map blocks have a default entry
#[derive(Default)]
pub struct MapMissingDefaultPlugin;

impl Plugin for MapMissingDefaultPlugin {
    fn spec(&self) -> PluginSpec {
        PluginSpec::new(
            "map-missing-default",
            "best-practices",
            "Warns when a map block does not have a default entry",
        )
        .with_severity("warning")
        .with_why(
            "Without a `default` entry in a `map` block, any value that doesn't match a listed \
             pattern will silently resolve to an empty string. This can lead to subtle bugs that \
             are hard to diagnose. Always specify a `default` to make the fallback behavior \
             explicit.",
        )
        .with_bad_example(include_str!("../examples/bad.conf").trim())
        .with_good_example(include_str!("../examples/good.conf").trim())
        .with_references(vec![
            "https://nginx.org/en/docs/http/ngx_http_map_module.html".to_string(),
            "https://github.com/walf443/nginx-lint/blob/main/plugins/builtin/best_practices/map_missing_default/tests/container_test.rs".to_string(),
        ])
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();
        let err = self.spec().error_builder();

        for ctx in config.all_directives_with_context() {
            if !ctx.directive.is("map") {
                continue;
            }

            let has_default = ctx
                .directive
                .block
                .as_ref()
                .map(|block| block.directives().any(|d| d.is("default")))
                .unwrap_or(false);

            if !has_default {
                errors
                    .push(err.warning_at("map block is missing a `default` entry", ctx.directive));
            }
        }

        errors
    }
}

nginx_lint_plugin::export_component_plugin!(MapMissingDefaultPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use nginx_lint_plugin::testing::PluginTestRunner;

    #[test]
    fn test_map_with_default() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);

        runner.assert_no_errors(
            r#"
http {
    map $uri $new {
        default /;
        /old /new;
        /foo /bar;
    }
}
"#,
        );
    }

    #[test]
    fn test_map_without_default() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);

        runner.assert_has_errors(
            r#"
http {
    map $uri $new {
        /old /new;
        /foo /bar;
    }
}
"#,
        );
    }

    #[test]
    fn test_multiple_maps() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);

        let errors = runner
            .check_string(
                r#"
http {
    map $uri $new {
        default /;
        /old /new;
    }
    map $host $backend {
        /foo /bar;
    }
}
"#,
            )
            .unwrap();

        assert_eq!(errors.len(), 1, "Expected 1 error, got: {:?}", errors);
    }

    #[test]
    fn test_map_in_stream_without_default() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);

        runner.assert_has_errors(
            r#"
stream {
    map $ssl_preread_server_name $backend {
        example.com upstream1;
        example.org upstream2;
    }
}
"#,
        );
    }

    #[test]
    fn test_map_in_stream_with_default() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);

        runner.assert_no_errors(
            r#"
stream {
    map $ssl_preread_server_name $backend {
        default upstream1;
        example.com upstream2;
    }
}
"#,
        );
    }

    #[test]
    fn test_examples() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);
        runner.test_examples(
            include_str!("../examples/bad.conf"),
            include_str!("../examples/good.conf"),
        );
    }

    #[test]
    fn test_fixtures() {
        let runner = PluginTestRunner::new(MapMissingDefaultPlugin);
        runner.test_fixtures(nginx_lint_plugin::fixtures_dir!());
    }
}
