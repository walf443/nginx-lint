//! Component model based lint rule implementation
//!
//! This module implements the LintRule trait for WIT component model plugins.
//! Plugins communicate with the host via WIT resource handles for config access,
//! eliminating the need for JSON serialization.

use super::error::PluginError;
use crate::linter::{LintError, LintRule, Severity};
use crate::parser::ast::{self, Config};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::component::{Resource, ResourceTable};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

/// Host-side config resource, holding the parsed Config.
pub struct ConfigResource {
    config: Arc<Config>,
}

/// Host-side directive resource, holding a cloned Directive.
pub struct DirectiveResource {
    directive: ast::Directive,
}

/// Generated bindings from WIT file, isolated in a submodule to avoid name conflicts
mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/nginx-lint-plugin.wit",
        world: "plugin",
        with: {
            "nginx-lint:plugin/config-api/config": super::ConfigResource,
            "nginx-lint:plugin/config-api/directive": super::DirectiveResource,
        },
        trappable_imports: true,
    });
}

use bindings::Plugin;
use bindings::nginx_lint::plugin::config_api;

/// Store data for component model execution
struct ComponentStoreData {
    limits: StoreLimits,
    table: ResourceTable,
}

// === Host trait implementations for config-api ===

impl config_api::Host for ComponentStoreData {}

impl config_api::HostConfig for ComponentStoreData {
    fn all_directives_with_context(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<Vec<config_api::DirectiveContext>> {
        let config = self.table.get(&self_)?.config.clone();
        let mut collected = Vec::new();
        collect_directives_with_context(&config.items, &config.include_context, &mut collected);

        let mut results = Vec::new();
        for (directive, parent_stack, depth) in collected {
            let dir_resource = self.table.push(DirectiveResource {
                directive: directive.clone(),
            })?;
            results.push(config_api::DirectiveContext {
                directive: dir_resource,
                parent_stack,
                depth,
            });
        }
        Ok(results)
    }

    fn all_directives(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<Vec<Resource<DirectiveResource>>> {
        let config = self.table.get(&self_)?.config.clone();
        let mut collected = Vec::new();
        collect_all_directives(&config.items, &mut collected);

        let mut results = Vec::new();
        for directive in collected {
            let dir_resource = self.table.push(DirectiveResource {
                directive: directive.clone(),
            })?;
            results.push(dir_resource);
        }
        Ok(results)
    }

    fn items(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<Vec<config_api::ConfigItem>> {
        let items = { self.table.get(&self_)?.config.items.clone() };
        convert_config_items_to_wit(&items, &mut self.table)
    }

    fn include_context(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<Vec<String>> {
        Ok(self.table.get(&self_)?.config.include_context.clone())
    }

    fn is_included_from(
        &mut self,
        self_: Resource<ConfigResource>,
        context: String,
    ) -> wasmtime::Result<bool> {
        let ctx = &self.table.get(&self_)?.config.include_context;
        Ok(ctx.iter().any(|c| c == &context))
    }

    fn is_included_from_http(&mut self, self_: Resource<ConfigResource>) -> wasmtime::Result<bool> {
        let ctx = &self.table.get(&self_)?.config.include_context;
        Ok(ctx.iter().any(|c| c == "http"))
    }

    fn is_included_from_http_server(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<bool> {
        let ctx = &self.table.get(&self_)?.config.include_context;
        Ok(ctx.iter().any(|c| c == "http")
            && ctx.iter().any(|c| c == "server")
            && ctx.iter().position(|c| c == "http") < ctx.iter().position(|c| c == "server"))
    }

    fn is_included_from_http_location(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<bool> {
        let ctx = &self.table.get(&self_)?.config.include_context;
        Ok(ctx.iter().any(|c| c == "http")
            && ctx.iter().any(|c| c == "location")
            && ctx.iter().position(|c| c == "http") < ctx.iter().position(|c| c == "location"))
    }

    fn is_included_from_stream(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<bool> {
        let ctx = &self.table.get(&self_)?.config.include_context;
        Ok(ctx.iter().any(|c| c == "stream"))
    }

    fn immediate_parent_context(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> wasmtime::Result<Option<String>> {
        Ok(self
            .table
            .get(&self_)?
            .config
            .include_context
            .last()
            .cloned())
    }

    fn drop(&mut self, rep: Resource<ConfigResource>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}

impl config_api::HostDirective for ComponentStoreData {
    fn data(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<config_api::DirectiveData> {
        let dir = &self.table.get(&self_)?.directive;
        Ok(config_api::DirectiveData {
            name: dir.name.clone(),
            args: dir.args.iter().map(convert_argument_to_wit).collect(),
            line: dir.span.start.line as u32,
            column: dir.span.start.column as u32,
            start_offset: dir.span.start.offset as u32,
            end_offset: dir.span.end.offset as u32,
            leading_whitespace: dir.leading_whitespace.clone(),
            trailing_whitespace: dir.trailing_whitespace.clone(),
            space_before_terminator: dir.space_before_terminator.clone(),
            has_block: dir.block.is_some(),
            block_is_raw: dir
                .block
                .as_ref()
                .map_or(false, |b| b.raw_content.is_some()),
        })
    }

    fn name(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<String> {
        Ok(self.table.get(&self_)?.directive.name.clone())
    }

    fn is(&mut self, self_: Resource<DirectiveResource>, name: String) -> wasmtime::Result<bool> {
        Ok(self.table.get(&self_)?.directive.name == name)
    }

    fn first_arg(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<Option<String>> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .first_arg()
            .map(|s| s.to_string()))
    }

    fn first_arg_is(
        &mut self,
        self_: Resource<DirectiveResource>,
        value: String,
    ) -> wasmtime::Result<bool> {
        Ok(self.table.get(&self_)?.directive.first_arg_is(&value))
    }

    fn arg_at(
        &mut self,
        self_: Resource<DirectiveResource>,
        index: u32,
    ) -> wasmtime::Result<Option<String>> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .args
            .get(index as usize)
            .map(|a| a.as_str().to_string()))
    }

    fn last_arg(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<Option<String>> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .args
            .last()
            .map(|a| a.as_str().to_string()))
    }

    fn has_arg(
        &mut self,
        self_: Resource<DirectiveResource>,
        value: String,
    ) -> wasmtime::Result<bool> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .args
            .iter()
            .any(|a| a.as_str() == value))
    }

    fn arg_count(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&self_)?.directive.args.len() as u32)
    }

    fn args(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<Vec<config_api::ArgumentInfo>> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .args
            .iter()
            .map(convert_argument_to_wit)
            .collect())
    }

    fn line(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&self_)?.directive.span.start.line as u32)
    }

    fn column(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&self_)?.directive.span.start.column as u32)
    }

    fn start_offset(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&self_)?.directive.span.start.offset as u32)
    }

    fn end_offset(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&self_)?.directive.span.end.offset as u32)
    }

    fn leading_whitespace(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<String> {
        Ok(self.table.get(&self_)?.directive.leading_whitespace.clone())
    }

    fn trailing_whitespace(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<String> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .trailing_whitespace
            .clone())
    }

    fn space_before_terminator(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<String> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .space_before_terminator
            .clone())
    }

    fn has_block(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<bool> {
        Ok(self.table.get(&self_)?.directive.block.is_some())
    }

    fn block_items(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<Vec<config_api::ConfigItem>> {
        let items = {
            match &self.table.get(&self_)?.directive.block {
                Some(block) => block.items.clone(),
                None => Vec::new(),
            }
        };
        convert_config_items_to_wit(&items, &mut self.table)
    }

    fn block_is_raw(&mut self, self_: Resource<DirectiveResource>) -> wasmtime::Result<bool> {
        Ok(self
            .table
            .get(&self_)?
            .directive
            .block
            .as_ref()
            .map_or(false, |b| b.is_raw()))
    }

    fn replace_with(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> wasmtime::Result<bindings::nginx_lint::plugin::types::Fix> {
        let d = &self.table.get(&self_)?.directive;
        let start = d.span.start.offset - d.leading_whitespace.len();
        let end = d.span.end.offset;
        let fixed = format!("{}{}", d.leading_whitespace, new_text);
        Ok(make_range_fix(start, end, fixed))
    }

    fn delete_line_fix(
        &mut self,
        self_: Resource<DirectiveResource>,
    ) -> wasmtime::Result<bindings::nginx_lint::plugin::types::Fix> {
        let line = self.table.get(&self_)?.directive.span.start.line;
        Ok(bindings::nginx_lint::plugin::types::Fix {
            line: line as u32,
            old_text: None,
            new_text: String::new(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        })
    }

    fn insert_after(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> wasmtime::Result<bindings::nginx_lint::plugin::types::Fix> {
        let d = &self.table.get(&self_)?.directive;
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text = format!("\n{}{}", indent, new_text);
        let offset = d.span.end.offset;
        Ok(make_range_fix(offset, offset, fix_text))
    }

    fn insert_before(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> wasmtime::Result<bindings::nginx_lint::plugin::types::Fix> {
        let d = &self.table.get(&self_)?.directive;
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text = format!("{}{}\n", indent, new_text);
        let offset = d.span.start.offset - (d.span.start.column - 1);
        Ok(make_range_fix(offset, offset, fix_text))
    }

    fn insert_after_many(
        &mut self,
        self_: Resource<DirectiveResource>,
        lines: Vec<String>,
    ) -> wasmtime::Result<bindings::nginx_lint::plugin::types::Fix> {
        let d = &self.table.get(&self_)?.directive;
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("\n{}{}", indent, line))
            .collect();
        let offset = d.span.end.offset;
        Ok(make_range_fix(offset, offset, fix_text))
    }

    fn insert_before_many(
        &mut self,
        self_: Resource<DirectiveResource>,
        lines: Vec<String>,
    ) -> wasmtime::Result<bindings::nginx_lint::plugin::types::Fix> {
        let d = &self.table.get(&self_)?.directive;
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("{}{}\n", indent, line))
            .collect();
        let offset = d.span.start.offset - (d.span.start.column - 1);
        Ok(make_range_fix(offset, offset, fix_text))
    }

    fn drop(&mut self, rep: Resource<DirectiveResource>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}

// === Helper functions ===

/// Create a range-based WIT fix.
fn make_range_fix(
    start: usize,
    end: usize,
    new_text: String,
) -> bindings::nginx_lint::plugin::types::Fix {
    bindings::nginx_lint::plugin::types::Fix {
        line: 0,
        old_text: None,
        new_text,
        delete_line: false,
        insert_after: false,
        start_offset: Some(start as u32),
        end_offset: Some(end as u32),
    }
}

/// Recursively collect directives with parent context (depth-first).
fn collect_directives_with_context<'a>(
    items: &'a [ast::ConfigItem],
    parent_stack: &[String],
    results: &mut Vec<(&'a ast::Directive, Vec<String>, u32)>,
) {
    for item in items {
        if let ast::ConfigItem::Directive(directive) = item {
            results.push((directive, parent_stack.to_vec(), parent_stack.len() as u32));
            if let Some(block) = &directive.block {
                let mut child_stack = parent_stack.to_vec();
                child_stack.push(directive.name.clone());
                collect_directives_with_context(&block.items, &child_stack, results);
            }
        }
    }
}

/// Recursively collect all directives (depth-first).
fn collect_all_directives<'a>(items: &'a [ast::ConfigItem], results: &mut Vec<&'a ast::Directive>) {
    for item in items {
        if let ast::ConfigItem::Directive(directive) = item {
            results.push(directive);
            if let Some(block) = &directive.block {
                collect_all_directives(&block.items, results);
            }
        }
    }
}

/// Convert parser ConfigItems to WIT ConfigItems.
fn convert_config_items_to_wit(
    items: &[ast::ConfigItem],
    table: &mut ResourceTable,
) -> wasmtime::Result<Vec<config_api::ConfigItem>> {
    let mut results = Vec::new();
    for item in items {
        match item {
            ast::ConfigItem::Directive(directive) => {
                let dir_resource = table.push(DirectiveResource {
                    directive: directive.as_ref().clone(),
                })?;
                results.push(config_api::ConfigItem::DirectiveItem(dir_resource));
            }
            ast::ConfigItem::Comment(comment) => {
                results.push(config_api::ConfigItem::CommentItem(
                    config_api::CommentInfo {
                        text: comment.text.clone(),
                        line: comment.span.start.line as u32,
                        column: comment.span.start.column as u32,
                        leading_whitespace: comment.leading_whitespace.clone(),
                        trailing_whitespace: comment.trailing_whitespace.clone(),
                        start_offset: comment.span.start.offset as u32,
                        end_offset: comment.span.end.offset as u32,
                    },
                ));
            }
            ast::ConfigItem::BlankLine(blank) => {
                results.push(config_api::ConfigItem::BlankLineItem(
                    config_api::BlankLineInfo {
                        line: blank.span.start.line as u32,
                        content: blank.content.clone(),
                        start_offset: blank.span.start.offset as u32,
                    },
                ));
            }
        }
    }
    Ok(results)
}

/// Convert a parser Argument to WIT ArgumentInfo.
fn convert_argument_to_wit(arg: &ast::Argument) -> config_api::ArgumentInfo {
    config_api::ArgumentInfo {
        value: arg.as_str().to_string(),
        raw: arg.raw.clone(),
        arg_type: match &arg.value {
            ast::ArgumentValue::Literal(_) => config_api::ArgumentType::Literal,
            ast::ArgumentValue::QuotedString(_) => config_api::ArgumentType::QuotedString,
            ast::ArgumentValue::SingleQuotedString(_) => {
                config_api::ArgumentType::SingleQuotedString
            }
            ast::ArgumentValue::Variable(_) => config_api::ArgumentType::Variable,
        },
        line: arg.span.start.line as u32,
        column: arg.span.start.column as u32,
        start_offset: arg.span.start.offset as u32,
        end_offset: arg.span.end.offset as u32,
    }
}

// === WIT type conversion functions ===

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

// === ComponentLintRule ===

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
        let mut store = Store::new(
            engine,
            ComponentStoreData {
                limits,
                table: ResourceTable::new(),
            },
        );
        store.limiter(|data| &mut data.limits);
        if fuel_enabled {
            store.set_fuel(fuel_limit).map_err(|e| {
                PluginError::execution_error(path, format!("Failed to set fuel: {}", e))
            })?;
        }
        Ok(store)
    }

    /// Instantiate the component with config-api imports registered
    fn instantiate(
        engine: &Engine,
        component: &wasmtime::component::Component,
        store: &mut Store<ComponentStoreData>,
        path: &Path,
    ) -> Result<Plugin, PluginError> {
        let mut linker = wasmtime::component::Linker::<ComponentStoreData>::new(engine);

        // Register config-api host functions
        config_api::add_to_linker(&mut linker, |data: &mut ComponentStoreData| data).map_err(
            |e| {
                PluginError::instantiate_error(
                    path,
                    format!("Failed to add config-api to linker: {}", e),
                )
            },
        )?;

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

    /// Execute the check function using resource-based config access
    fn execute_check(
        &self,
        config: &Config,
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

        // Create config resource handle
        let config_resource = store
            .data_mut()
            .table
            .push(ConfigResource {
                config: Arc::new(config.clone()),
            })
            .map_err(|e| {
                PluginError::execution_error(
                    &self.path,
                    format!("Failed to create config resource: {}", e),
                )
            })?;

        let path_str = file_path.to_string_lossy().to_string();
        let wit_errors = plugin
            .call_check(&mut store, config_resource, &path_str)
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
        config: &Config,
        path: &Path,
        _serialized_config: &str,
    ) -> Vec<LintError> {
        // With resource-based approach, we don't use serialized config
        self.check(config, path)
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
