//! Component model based lint rule implementation
//!
//! This module implements the LintRule trait for WIT component model plugins.

use super::error::PluginError;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::Config;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

/// Generated bindings from WIT file, isolated in a submodule to avoid name conflicts
mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/nginx-lint-plugin.wit",
        world: "plugin",
    });
}

use bindings::Plugin;

/// Store data for component model execution
struct ComponentStoreData {
    limits: StoreLimits,
}

/// Convert WIT Severity to crate Severity
fn convert_severity(severity: &bindings::nginx_lint::plugin::types::Severity) -> Severity {
    match severity {
        bindings::nginx_lint::plugin::types::Severity::Error => Severity::Error,
        bindings::nginx_lint::plugin::types::Severity::Warning => Severity::Warning,
    }
}

/// Convert WIT Fix to crate Fix
fn convert_fix(fix: &bindings::nginx_lint::plugin::types::Fix) -> crate::linter::Fix {
    crate::linter::Fix {
        line: fix.line as usize,
        old_text: fix.old_text.clone(),
        new_text: fix.new_text.clone(),
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|v| v as usize),
        end_offset: fix.end_offset.map(|v| v as usize),
    }
}

/// Convert WIT LintError to crate LintError
fn convert_lint_error(error: &bindings::nginx_lint::plugin::types::LintError) -> LintError {
    let severity = convert_severity(&error.severity);
    let mut lint_error = LintError::new(&error.rule, &error.category, &error.message, severity);

    if let (Some(line), Some(column)) = (error.line, error.column) {
        lint_error = lint_error.with_location(line as usize, column as usize);
    } else if let Some(line) = error.line {
        lint_error = lint_error.with_location(line as usize, 1);
    }

    for fix in &error.fixes {
        lint_error = lint_error.with_fix(convert_fix(fix));
    }

    lint_error
}

/// Convert WIT PluginSpec to the wasm_rule PluginSpec format
fn convert_plugin_spec(
    spec: &bindings::nginx_lint::plugin::types::PluginSpec,
) -> super::wasm_rule::PluginSpec {
    super::wasm_rule::PluginSpec {
        name: spec.name.clone(),
        category: spec.category.clone(),
        description: spec.description.clone(),
        api_version: spec.api_version.clone(),
        severity: spec.severity.clone(),
        why: spec.why.clone(),
        bad_example: spec.bad_example.clone(),
        good_example: spec.good_example.clone(),
        references: spec.references.clone(),
    }
}

/// A lint rule implemented as a WIT component model plugin
#[derive(Clone)]
pub struct ComponentLintRule {
    /// Path to the component file (for error reporting)
    path: PathBuf,
    /// Plugin metadata
    spec: super::wasm_rule::PluginSpec,
    /// Compiled component (shared across threads)
    component: Arc<wasmtime::component::Component>,
    /// WASM engine reference (shared across threads)
    engine: Engine,
    /// Memory limit in bytes
    memory_limit: u64,
    /// Fuel limit for CPU metering (0 = unlimited)
    fuel_limit: u64,
    /// Whether fuel metering is enabled
    fuel_enabled: bool,
    /// Leaked static strings for LintRule trait
    name: &'static str,
    category: &'static str,
    description: &'static str,
}

impl ComponentLintRule {
    /// Create a new component lint rule from compiled bytes
    pub fn new(
        engine: &Engine,
        path: PathBuf,
        component_bytes: &[u8],
        memory_limit: u64,
        fuel_limit: u64,
        fuel_enabled: bool,
    ) -> Result<Self, PluginError> {
        // Compile the component
        let component = wasmtime::component::Component::new(engine, component_bytes)
            .map_err(|e| PluginError::compile_error(&path, e.to_string()))?;

        // Get plugin spec
        let spec_wit = Self::get_plugin_spec_from_component(
            engine,
            &component,
            &path,
            memory_limit,
            fuel_limit,
            fuel_enabled,
        )?;
        let spec = convert_plugin_spec(&spec_wit);

        // Leak strings for 'static lifetime required by the LintRule trait.
        // These live for the entire program duration. Since plugins are loaded once
        // at startup and never unloaded, this is acceptable.
        let name: &'static str = Box::leak(spec.name.clone().into_boxed_str());
        let category: &'static str = Box::leak(spec.category.clone().into_boxed_str());
        let description: &'static str = Box::leak(spec.description.clone().into_boxed_str());

        Ok(Self {
            path,
            spec,
            component: Arc::new(component),
            engine: engine.clone(),
            memory_limit,
            fuel_limit,
            fuel_enabled,
            name,
            category,
            description,
        })
    }

    /// Create a store with limits and fuel
    fn create_store(
        engine: &Engine,
        memory_limit: u64,
        fuel_limit: u64,
        fuel_enabled: bool,
        path: &Path,
    ) -> Result<Store<ComponentStoreData>, PluginError> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_limit as usize)
            .build();
        let mut store = Store::new(engine, ComponentStoreData { limits });
        store.limiter(|data| &mut data.limits);
        if fuel_enabled {
            store.set_fuel(fuel_limit).map_err(|e| {
                PluginError::execution_error(path, format!("Failed to set fuel: {}", e))
            })?;
        }
        Ok(store)
    }

    /// Instantiate the component
    fn instantiate(
        engine: &Engine,
        component: &wasmtime::component::Component,
        store: &mut Store<ComponentStoreData>,
        path: &Path,
    ) -> Result<Plugin, PluginError> {
        let linker = wasmtime::component::Linker::<ComponentStoreData>::new(engine);
        Plugin::instantiate(store, component, &linker)
            .map_err(|e| PluginError::instantiate_error(path, e.to_string()))
    }

    /// Get plugin spec by instantiating the component and calling spec()
    fn get_plugin_spec_from_component(
        engine: &Engine,
        component: &wasmtime::component::Component,
        path: &Path,
        memory_limit: u64,
        fuel_limit: u64,
        fuel_enabled: bool,
    ) -> Result<bindings::nginx_lint::plugin::types::PluginSpec, PluginError> {
        let mut store = Self::create_store(engine, memory_limit, fuel_limit, fuel_enabled, path)?;
        let plugin = Self::instantiate(engine, component, &mut store, path)?;

        plugin
            .call_spec(&mut store)
            .map_err(|e| PluginError::execution_error(path, format!("spec() call failed: {}", e)))
    }

    /// Execute the check function
    fn execute_check(
        &self,
        config: &Config,
        file_path: &Path,
    ) -> Result<Vec<LintError>, PluginError> {
        let config_json = serde_json::to_string(config).map_err(|e| {
            PluginError::execution_error(&self.path, format!("Failed to serialize config: {}", e))
        })?;
        self.execute_check_with_serialized(&config_json, file_path)
    }

    /// Execute the check function with pre-serialized config JSON
    fn execute_check_with_serialized(
        &self,
        config_json: &str,
        file_path: &Path,
    ) -> Result<Vec<LintError>, PluginError> {
        let mut store = Self::create_store(
            &self.engine,
            self.memory_limit,
            self.fuel_limit,
            self.fuel_enabled,
            &self.path,
        )?;
        let plugin = Self::instantiate(&self.engine, &self.component, &mut store, &self.path)?;

        let path_str = file_path.to_string_lossy().to_string();
        let wit_errors = plugin
            .call_check(&mut store, config_json, &path_str)
            .map_err(|e| {
                if e.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel) {
                    PluginError::timeout(&self.path)
                } else {
                    PluginError::execution_error(&self.path, format!("check() failed: {}", e))
                }
            })?;

        Ok(wit_errors.iter().map(convert_lint_error).collect())
    }
}

impl LintRule for ComponentLintRule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn category(&self) -> &'static str {
        self.category
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn check(&self, config: &Config, path: &Path) -> Vec<LintError> {
        match self.execute_check(config, path) {
            Ok(errors) => errors,
            Err(e) => {
                vec![LintError::new(
                    self.name,
                    self.category,
                    &format!("Plugin execution failed: {}", e),
                    Severity::Error,
                )]
            }
        }
    }

    fn check_with_serialized_config(
        &self,
        _config: &Config,
        path: &Path,
        serialized_config: &str,
    ) -> Vec<LintError> {
        match self.execute_check_with_serialized(serialized_config, path) {
            Ok(errors) => errors,
            Err(e) => {
                vec![LintError::new(
                    self.name,
                    self.category,
                    &format!("Plugin execution failed: {}", e),
                    Severity::Error,
                )]
            }
        }
    }

    fn why(&self) -> Option<&str> {
        self.spec.why.as_deref()
    }

    fn bad_example(&self) -> Option<&str> {
        self.spec.bad_example.as_deref()
    }

    fn good_example(&self) -> Option<&str> {
        self.spec.good_example.as_deref()
    }

    fn references(&self) -> Option<Vec<String>> {
        self.spec.references.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_severity_error() {
        let wit_severity = bindings::nginx_lint::plugin::types::Severity::Error;
        assert!(matches!(convert_severity(&wit_severity), Severity::Error));
    }

    #[test]
    fn test_convert_severity_warning() {
        let wit_severity = bindings::nginx_lint::plugin::types::Severity::Warning;
        assert!(matches!(convert_severity(&wit_severity), Severity::Warning));
    }

    #[test]
    fn test_convert_fix_basic() {
        let wit_fix = bindings::nginx_lint::plugin::types::Fix {
            line: 10,
            old_text: Some("old".to_string()),
            new_text: "new".to_string(),
            delete_line: false,
            insert_after: true,
            start_offset: Some(5),
            end_offset: Some(8),
        };
        let fix = convert_fix(&wit_fix);
        assert_eq!(fix.line, 10);
        assert_eq!(fix.old_text.as_deref(), Some("old"));
        assert_eq!(fix.new_text, "new");
        assert!(!fix.delete_line);
        assert!(fix.insert_after);
        assert_eq!(fix.start_offset, Some(5));
        assert_eq!(fix.end_offset, Some(8));
    }

    #[test]
    fn test_convert_fix_optional_fields_none() {
        let wit_fix = bindings::nginx_lint::plugin::types::Fix {
            line: 1,
            old_text: None,
            new_text: "text".to_string(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        };
        let fix = convert_fix(&wit_fix);
        assert!(fix.old_text.is_none());
        assert!(fix.delete_line);
        assert!(fix.start_offset.is_none());
        assert!(fix.end_offset.is_none());
    }

    #[test]
    fn test_convert_lint_error_with_location() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "test-rule".to_string(),
            category: "test-cat".to_string(),
            message: "test message".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(42),
            column: Some(10),
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.rule, "test-rule");
        assert_eq!(error.category, "test-cat");
        assert_eq!(error.message, "test message");
        assert!(matches!(error.severity, Severity::Warning));
        assert_eq!(error.line, Some(42));
        assert_eq!(error.column, Some(10));
    }

    #[test]
    fn test_convert_lint_error_line_only() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Error,
            line: Some(5),
            column: None,
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.line, Some(5));
        assert_eq!(error.column, Some(1)); // defaults to column 1
    }

    #[test]
    fn test_convert_lint_error_no_location() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Error,
            line: None,
            column: None,
            fixes: vec![],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.line, None);
        assert_eq!(error.column, None);
    }

    #[test]
    fn test_convert_lint_error_with_fixes() {
        let wit_error = bindings::nginx_lint::plugin::types::LintError {
            rule: "rule".to_string(),
            category: "cat".to_string(),
            message: "msg".to_string(),
            severity: bindings::nginx_lint::plugin::types::Severity::Warning,
            line: Some(1),
            column: Some(1),
            fixes: vec![bindings::nginx_lint::plugin::types::Fix {
                line: 1,
                old_text: Some("bad".to_string()),
                new_text: "good".to_string(),
                delete_line: false,
                insert_after: false,
                start_offset: None,
                end_offset: None,
            }],
        };
        let error = convert_lint_error(&wit_error);
        assert_eq!(error.fixes.len(), 1);
        assert_eq!(error.fixes[0].new_text, "good");
    }

    #[test]
    fn test_convert_plugin_spec() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "test-plugin".to_string(),
            category: "security".to_string(),
            description: "A test plugin".to_string(),
            api_version: "1.0".to_string(),
            severity: Some("warning".to_string()),
            why: Some("because".to_string()),
            bad_example: Some("bad".to_string()),
            good_example: Some("good".to_string()),
            references: Some(vec!["https://example.com".to_string()]),
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "test-plugin");
        assert_eq!(spec.category, "security");
        assert_eq!(spec.description, "A test plugin");
        assert_eq!(spec.api_version, "1.0");
        assert_eq!(spec.severity.as_deref(), Some("warning"));
        assert_eq!(spec.why.as_deref(), Some("because"));
        assert_eq!(spec.bad_example.as_deref(), Some("bad"));
        assert_eq!(spec.good_example.as_deref(), Some("good"));
        assert_eq!(
            spec.references,
            Some(vec!["https://example.com".to_string()])
        );
    }

    #[test]
    fn test_convert_plugin_spec_optional_none() {
        let wit_spec = bindings::nginx_lint::plugin::types::PluginSpec {
            name: "minimal".to_string(),
            category: "test".to_string(),
            description: "Minimal".to_string(),
            api_version: "1.0".to_string(),
            severity: None,
            why: None,
            bad_example: None,
            good_example: None,
            references: None,
        };
        let spec = convert_plugin_spec(&wit_spec);
        assert_eq!(spec.name, "minimal");
        assert!(spec.severity.is_none());
        assert!(spec.why.is_none());
        assert!(spec.references.is_none());
    }

    #[test]
    fn test_new_with_invalid_bytes() {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).unwrap();
        let result = ComponentLintRule::new(
            &engine,
            PathBuf::from("test.wasm"),
            b"not a wasm component",
            256 * 1024 * 1024,
            10_000_000,
            true,
        );
        assert!(matches!(result, Err(PluginError::CompileError { .. })));
    }
}
