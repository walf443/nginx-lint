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
            "nginx-lint:plugin/config-api.config": super::ConfigResource,
            "nginx-lint:plugin/config-api.directive": super::DirectiveResource,
        },
    });
}

use bindings::Plugin;
use bindings::nginx_lint::plugin::config_api;

/// Store data for component model execution
struct ComponentStoreData {
    limits: StoreLimits,
    table: ResourceTable,
}

impl wasmtime::component::HasData for ComponentStoreData {
    type Data<'a> = &'a mut ComponentStoreData;
}

/// Helper methods for resource table access.
///
/// These methods panic on invalid handles or table exhaustion. In the wasmtime
/// runtime context, panics in host functions are caught and converted to traps,
/// so the host process will not crash. The descriptive panic messages appear in
/// the trap error for debugging.
impl ComponentStoreData {
    fn get_directive(&self, self_: &Resource<DirectiveResource>) -> &ast::Directive {
        &self
            .table
            .get(self_)
            .expect("invalid directive resource handle")
            .directive
    }

    fn get_config(&self, self_: &Resource<ConfigResource>) -> &Arc<Config> {
        &self
            .table
            .get(self_)
            .expect("invalid config resource handle")
            .config
    }

    /// Push a directive into the resource table.
    ///
    /// Panics (trapped by wasmtime) if the resource table is full. This can
    /// happen if an untrusted plugin requests an excessive number of handles.
    /// The fuel limit should prevent this in practice.
    fn push_directive(&mut self, directive: ast::Directive) -> Resource<DirectiveResource> {
        self.table
            .push(DirectiveResource { directive })
            .expect("resource table full: too many directive handles allocated")
    }
}

// === Host trait implementations for config-api ===

impl bindings::nginx_lint::plugin::types::Host for ComponentStoreData {}

impl bindings::nginx_lint::plugin::data_types::Host for ComponentStoreData {}

impl config_api::Host for ComponentStoreData {}

impl config_api::HostConfig for ComponentStoreData {
    fn all_directives_with_context(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> Vec<config_api::DirectiveContext> {
        // TODO: Each directive (including its block subtree) is deep-cloned into
        // a DirectiveResource. For large configs this is O(n^2) memory/time.
        // Consider storing Arc<Directive> or an index into the original
        // Arc<Config> to avoid deep cloning.
        let config = self.get_config(&self_).clone();
        let mut collected = Vec::new();
        collect_directives_with_context(&config.items, &config.include_context, &mut collected);

        let mut results = Vec::new();
        for (directive, parent_stack, depth) in collected {
            let dir_resource = self.push_directive(directive.clone());
            results.push(config_api::DirectiveContext {
                directive: dir_resource,
                parent_stack,
                depth,
            });
        }
        results
    }

    fn all_directives(
        &mut self,
        self_: Resource<ConfigResource>,
    ) -> Vec<Resource<DirectiveResource>> {
        let config = self.get_config(&self_).clone();
        let mut collected = Vec::new();
        collect_all_directives(&config.items, &mut collected);

        let mut results = Vec::new();
        for directive in collected {
            let dir_resource = self.push_directive(directive.clone());
            results.push(dir_resource);
        }
        results
    }

    fn items(&mut self, self_: Resource<ConfigResource>) -> Vec<config_api::ConfigItem> {
        // Clone the Arc (cheap) to release the immutable borrow on self.table,
        // allowing convert_config_items_to_wit to borrow self.table mutably.
        let config = self.get_config(&self_).clone();
        convert_config_items_to_wit(&config.items, &mut self.table)
    }

    fn include_context(&mut self, self_: Resource<ConfigResource>) -> Vec<String> {
        self.get_config(&self_).include_context.clone()
    }

    fn is_included_from(&mut self, self_: Resource<ConfigResource>, context: String) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        ctx.iter().any(|c| c == &context)
    }

    fn is_included_from_http(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        ctx.iter().any(|c| c == "http")
    }

    fn is_included_from_http_server(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        if let (Some(http_pos), Some(server_pos)) = (
            ctx.iter().position(|c| c == "http"),
            ctx.iter().position(|c| c == "server"),
        ) {
            http_pos < server_pos
        } else {
            false
        }
    }

    fn is_included_from_http_location(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        if let (Some(http_pos), Some(location_pos)) = (
            ctx.iter().position(|c| c == "http"),
            ctx.iter().position(|c| c == "location"),
        ) {
            http_pos < location_pos
        } else {
            false
        }
    }

    fn is_included_from_stream(&mut self, self_: Resource<ConfigResource>) -> bool {
        let ctx = &self.get_config(&self_).include_context;
        ctx.iter().any(|c| c == "stream")
    }

    fn immediate_parent_context(&mut self, self_: Resource<ConfigResource>) -> Option<String> {
        self.get_config(&self_).include_context.last().cloned()
    }

    fn drop(&mut self, rep: Resource<ConfigResource>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}

impl config_api::HostDirective for ComponentStoreData {
    fn data(&mut self, self_: Resource<DirectiveResource>) -> config_api::DirectiveData {
        let dir = self.get_directive(&self_);
        config_api::DirectiveData {
            name: dir.name.clone(),
            args: dir.args.iter().map(convert_argument_to_wit).collect(),
            line: dir.span.start.line as u32,
            column: dir.span.start.column as u32,
            start_offset: dir.span.start.offset as u32,
            end_offset: dir.span.end.offset as u32,
            end_line: dir.span.end.line as u32,
            end_column: dir.span.end.column as u32,
            leading_whitespace: dir.leading_whitespace.clone(),
            trailing_whitespace: dir.trailing_whitespace.clone(),
            space_before_terminator: dir.space_before_terminator.clone(),
            has_block: dir.block.is_some(),
            block_is_raw: dir.block.as_ref().is_some_and(|b| b.raw_content.is_some()),
            block_raw_content: dir.block.as_ref().and_then(|b| b.raw_content.clone()),
            closing_brace_leading_whitespace: dir
                .block
                .as_ref()
                .map(|b| b.closing_brace_leading_whitespace.clone()),
            block_trailing_whitespace: dir.block.as_ref().map(|b| b.trailing_whitespace.clone()),
            trailing_comment_text: dir.trailing_comment.as_ref().map(|c| c.text.clone()),
            name_end_column: dir.name_span.end.column as u32,
            name_end_offset: dir.name_span.end.offset as u32,
            block_start_line: dir.block.as_ref().map(|b| b.span.start.line as u32),
            block_start_column: dir.block.as_ref().map(|b| b.span.start.column as u32),
            block_start_offset: dir.block.as_ref().map(|b| b.span.start.offset as u32),
        }
    }

    fn name(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).name.clone()
    }

    fn is(&mut self, self_: Resource<DirectiveResource>, name: String) -> bool {
        self.get_directive(&self_).name == name
    }

    fn first_arg(&mut self, self_: Resource<DirectiveResource>) -> Option<String> {
        self.get_directive(&self_)
            .first_arg()
            .map(|s| s.to_string())
    }

    fn first_arg_is(&mut self, self_: Resource<DirectiveResource>, value: String) -> bool {
        self.get_directive(&self_).first_arg_is(&value)
    }

    fn arg_at(&mut self, self_: Resource<DirectiveResource>, index: u32) -> Option<String> {
        self.get_directive(&self_)
            .args
            .get(index as usize)
            .map(|a| a.as_str().to_string())
    }

    fn last_arg(&mut self, self_: Resource<DirectiveResource>) -> Option<String> {
        self.get_directive(&self_)
            .args
            .last()
            .map(|a| a.as_str().to_string())
    }

    fn has_arg(&mut self, self_: Resource<DirectiveResource>, value: String) -> bool {
        self.get_directive(&self_)
            .args
            .iter()
            .any(|a| a.as_str() == value)
    }

    fn arg_count(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).args.len() as u32
    }

    fn args(&mut self, self_: Resource<DirectiveResource>) -> Vec<config_api::ArgumentInfo> {
        self.get_directive(&self_)
            .args
            .iter()
            .map(convert_argument_to_wit)
            .collect()
    }

    fn line(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.start.line as u32
    }

    fn column(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.start.column as u32
    }

    fn start_offset(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.start.offset as u32
    }

    fn end_offset(&mut self, self_: Resource<DirectiveResource>) -> u32 {
        self.get_directive(&self_).span.end.offset as u32
    }

    fn leading_whitespace(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).leading_whitespace.clone()
    }

    fn trailing_whitespace(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).trailing_whitespace.clone()
    }

    fn space_before_terminator(&mut self, self_: Resource<DirectiveResource>) -> String {
        self.get_directive(&self_).space_before_terminator.clone()
    }

    fn has_block(&mut self, self_: Resource<DirectiveResource>) -> bool {
        self.get_directive(&self_).block.is_some()
    }

    fn block_items(&mut self, self_: Resource<DirectiveResource>) -> Vec<config_api::ConfigItem> {
        // Clone block items to release the immutable borrow on self.table,
        // allowing convert_config_items_to_wit to borrow self.table mutably.
        // TODO: Consider using Arc<Directive> in DirectiveResource to avoid
        // deep-cloning block subtrees on each call.
        let items = {
            let dir = self
                .table
                .get(&self_)
                .expect("invalid directive resource handle");
            match &dir.directive.block {
                Some(block) => block.items.clone(),
                None => return Vec::new(),
            }
        };
        convert_config_items_to_wit(&items, &mut self.table)
    }

    fn block_is_raw(&mut self, self_: Resource<DirectiveResource>) -> bool {
        self.get_directive(&self_)
            .block
            .as_ref()
            .is_some_and(|b| b.is_raw())
    }

    fn replace_with(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let start = d.span.start.offset - d.leading_whitespace.len();
        let end = d.span.end.offset;
        let fixed = format!("{}{}", d.leading_whitespace, new_text);
        make_range_fix(start, end, fixed)
    }

    fn delete_line_fix(&mut self, self_: Resource<DirectiveResource>) -> config_api::Fix {
        let line = self.get_directive(&self_).span.start.line;
        config_api::Fix {
            line: line as u32,
            old_text: None,
            new_text: String::new(),
            delete_line: true,
            insert_after: false,
            start_offset: None,
            end_offset: None,
        }
    }

    fn insert_after(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text = format!("\n{}{}", indent, new_text);
        let offset = d.span.end.offset;
        make_range_fix(offset, offset, fix_text)
    }

    fn insert_before(
        &mut self,
        self_: Resource<DirectiveResource>,
        new_text: String,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text = format!("{}{}\n", indent, new_text);
        let offset = d
            .span
            .start
            .offset
            .saturating_sub(d.span.start.column.saturating_sub(1));
        make_range_fix(offset, offset, fix_text)
    }

    fn insert_after_many(
        &mut self,
        self_: Resource<DirectiveResource>,
        lines: Vec<String>,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("\n{}{}", indent, line))
            .collect();
        let offset = d.span.end.offset;
        make_range_fix(offset, offset, fix_text)
    }

    fn insert_before_many(
        &mut self,
        self_: Resource<DirectiveResource>,
        lines: Vec<String>,
    ) -> config_api::Fix {
        let d = self.get_directive(&self_);
        let indent = " ".repeat(d.span.start.column.saturating_sub(1));
        let fix_text: String = lines
            .iter()
            .map(|line| format!("{}{}\n", indent, line))
            .collect();
        let offset = d
            .span
            .start
            .offset
            .saturating_sub(d.span.start.column.saturating_sub(1));
        make_range_fix(offset, offset, fix_text)
    }

    fn drop(&mut self, rep: Resource<DirectiveResource>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}

// === Helper functions ===

/// Create a range-based WIT fix.
fn make_range_fix(start: usize, end: usize, new_text: String) -> config_api::Fix {
    config_api::Fix {
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
) -> Vec<config_api::ConfigItem> {
    let mut results = Vec::new();
    for item in items {
        match item {
            ast::ConfigItem::Directive(directive) => {
                let dir_resource = table
                    .push(DirectiveResource {
                        directive: directive.as_ref().clone(),
                    })
                    .expect("resource table full: too many directive handles allocated");
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
    results
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

        // Register all host functions (types + config-api)
        Plugin::add_to_linker::<ComponentStoreData, ComponentStoreData>(&mut linker, |data| data)
            .map_err(|e| {
            PluginError::instantiate_error(path, format!("Failed to add imports to linker: {}", e))
        })?;

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

        // Note: The config resource and any directive resources created during
        // the check are cleaned up when `store` is dropped at function exit.
        // A fresh store is created for each execute_check call, so resources
        // do not leak across lint runs.

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

    // === Host trait method tests ===

    /// Create a ComponentStoreData with a config resource for testing host methods.
    fn setup_store_with_config(
        include_context: Vec<String>,
        items: Vec<ast::ConfigItem>,
    ) -> (ComponentStoreData, Resource<ConfigResource>) {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let config = Arc::new(Config {
            items,
            include_context,
        });
        let resource = data
            .table
            .push(ConfigResource { config })
            .expect("push config");
        (data, resource)
    }

    /// Create a directive with the given name and span.
    fn make_directive(
        name: &str,
        line: usize,
        column: usize,
        start_offset: usize,
        end_offset: usize,
    ) -> ast::Directive {
        ast::Directive {
            name: name.to_string(),
            name_span: ast::Span::new(
                ast::Position::new(line, column, start_offset),
                ast::Position::new(line, column + name.len(), start_offset + name.len()),
            ),
            args: vec![],
            block: None,
            span: ast::Span::new(
                ast::Position::new(line, column, start_offset),
                ast::Position::new(line, column, end_offset),
            ),
            trailing_comment: None,
            leading_whitespace: " ".repeat(column.saturating_sub(1)),
            space_before_terminator: String::new(),
            trailing_whitespace: "\n".to_string(),
        }
    }

    #[test]
    fn test_is_included_from_http_server_correct_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["http".to_string(), "server".to_string()], vec![]);
        assert!(config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_reversed_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["server".to_string(), "http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_missing_server() {
        let (mut data, resource) = setup_store_with_config(vec!["http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_missing_http() {
        let (mut data, resource) = setup_store_with_config(vec!["server".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_server_empty_context() {
        let (mut data, resource) = setup_store_with_config(vec![], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_server(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_correct_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["http".to_string(), "location".to_string()], vec![]);
        assert!(config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_reversed_order() {
        let (mut data, resource) =
            setup_store_with_config(vec!["location".to_string(), "http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_missing_location() {
        let (mut data, resource) = setup_store_with_config(vec!["http".to_string()], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_empty_context() {
        let (mut data, resource) = setup_store_with_config(vec![], vec![]);
        assert!(!config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_is_included_from_http_location_with_server_in_between() {
        let (mut data, resource) = setup_store_with_config(
            vec![
                "http".to_string(),
                "server".to_string(),
                "location".to_string(),
            ],
            vec![],
        );
        assert!(config_api::HostConfig::is_included_from_http_location(
            &mut data, resource
        ));
    }

    #[test]
    fn test_insert_before_column_1() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let dir = make_directive("listen", 2, 1, 10, 20);
        let resource = data
            .table
            .push(DirectiveResource { directive: dir })
            .unwrap();

        let fix =
            config_api::HostDirective::insert_before(&mut data, resource, "new_line;".to_string());
        // Column 1: offset should be offset - 0 = 10
        assert_eq!(fix.start_offset, Some(10));
        assert_eq!(fix.end_offset, Some(10));
        assert!(fix.new_text.contains("new_line;"));
    }

    #[test]
    fn test_insert_before_indented() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let dir = make_directive("listen", 2, 5, 15, 25);
        let resource = data
            .table
            .push(DirectiveResource { directive: dir })
            .unwrap();

        let fix =
            config_api::HostDirective::insert_before(&mut data, resource, "new_line;".to_string());
        // Column 5: offset should be 15 - 4 = 11
        assert_eq!(fix.start_offset, Some(11));
        assert_eq!(fix.end_offset, Some(11));
        // Should include indentation
        assert!(fix.new_text.starts_with("    "));
    }

    #[test]
    fn test_insert_before_many_multiple_lines() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let dir = make_directive("listen", 2, 5, 15, 25);
        let resource = data
            .table
            .push(DirectiveResource { directive: dir })
            .unwrap();

        let fix = config_api::HostDirective::insert_before_many(
            &mut data,
            resource,
            vec!["line1;".to_string(), "line2;".to_string()],
        );
        assert_eq!(fix.start_offset, Some(11));
        assert!(fix.new_text.contains("line1;"));
        assert!(fix.new_text.contains("line2;"));
    }

    #[test]
    fn test_items_with_mixed_content() {
        let items = vec![
            ast::ConfigItem::Directive(Box::new(make_directive("http", 1, 1, 0, 10))),
            ast::ConfigItem::Comment(ast::Comment {
                text: "# comment".to_string(),
                span: ast::Span::new(ast::Position::new(2, 1, 11), ast::Position::new(2, 10, 20)),
                leading_whitespace: String::new(),
                trailing_whitespace: "\n".to_string(),
            }),
            ast::ConfigItem::BlankLine(ast::BlankLine {
                span: ast::Span::new(ast::Position::new(3, 1, 21), ast::Position::new(3, 1, 22)),
                content: "\n".to_string(),
            }),
        ];
        let (mut data, resource) = setup_store_with_config(vec![], items);
        let wit_items = config_api::HostConfig::items(&mut data, resource);
        assert_eq!(wit_items.len(), 3);
        // Check types: directive, comment, blank line
        assert!(matches!(
            wit_items[0],
            config_api::ConfigItem::DirectiveItem(_)
        ));
        assert!(matches!(
            wit_items[1],
            config_api::ConfigItem::CommentItem(_)
        ));
        assert!(matches!(
            wit_items[2],
            config_api::ConfigItem::BlankLineItem(_)
        ));
    }

    #[test]
    fn test_block_items_no_block() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let dir = make_directive("listen", 1, 1, 0, 10);
        let resource = data
            .table
            .push(DirectiveResource { directive: dir })
            .unwrap();

        let items = config_api::HostDirective::block_items(&mut data, resource);
        assert!(items.is_empty());
    }

    #[test]
    fn test_block_items_with_block() {
        let mut data = ComponentStoreData {
            limits: StoreLimitsBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let mut dir = make_directive("http", 1, 1, 0, 30);
        dir.block = Some(ast::Block {
            items: vec![ast::ConfigItem::Directive(Box::new(make_directive(
                "server", 2, 5, 10, 25,
            )))],
            span: ast::Span::new(ast::Position::new(1, 6, 5), ast::Position::new(3, 1, 30)),
            raw_content: None,
            closing_brace_leading_whitespace: String::new(),
            trailing_whitespace: "\n".to_string(),
        });
        let resource = data
            .table
            .push(DirectiveResource { directive: dir })
            .unwrap();

        let items = config_api::HostDirective::block_items(&mut data, resource);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], config_api::ConfigItem::DirectiveItem(_)));
    }
}
