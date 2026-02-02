//! Example nginx-lint plugin
//!
//! This plugin checks for the use of `debug_connection` directive,
//! which should not be used in production.
//!
//! Build with:
//! ```sh
//! cargo build --target wasm32-unknown-unknown --release
//! ```
//!
//! The output will be at:
//! `target/wasm32-unknown-unknown/release/example_plugin.wasm`

use nginx_lint::plugin_sdk::prelude::*;

/// Example plugin that warns about debug_connection directive
#[derive(Default)]
pub struct DebugConnectionRule;

impl Plugin for DebugConnectionRule {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "debug-connection-in-production".to_string(),
            category: "best_practices".to_string(),
            description: "Warns about debug_connection directive which should not be used in production".to_string(),
        }
    }

    fn check(&self, config: &Config, _path: &str) -> Vec<LintError> {
        let mut errors = Vec::new();

        for directive in config.all_directives() {
            if directive.is("debug_connection") {
                errors.push(LintError::warning(
                    "debug-connection-in-production",
                    "best_practices",
                    "debug_connection should not be used in production; it can expose sensitive information",
                    directive.span.start.line,
                    directive.span.start.column,
                ));
            }
        }

        errors
    }
}

// Export the plugin
nginx_lint::export_plugin!(DebugConnectionRule);
