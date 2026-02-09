//! Native builtin plugins compiled directly into the binary
//!
//! This module provides the same plugins as `builtin.rs` (WASM version),
//! but compiled as native Rust code for better performance.

use nginx_lint_common::linter::LintRule;
use nginx_lint_plugin::native::NativePluginRule;

/// Create all native builtin plugin rules
pub fn load_native_builtin_plugins() -> Vec<Box<dyn LintRule>> {
    vec![
        // Security plugins
        Box::new(NativePluginRule::<
            server_tokens_enabled_plugin::ServerTokensEnabledPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            autoindex_enabled_plugin::AutoindexEnabledPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            deprecated_ssl_protocol_plugin::DeprecatedSslProtocolPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            weak_ssl_ciphers_plugin::WeakSslCiphersPlugin,
        >::new()),
        // Style plugins
        Box::new(NativePluginRule::<
            space_before_semicolon_plugin::SpaceBeforeSemicolonPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            trailing_whitespace_plugin::TrailingWhitespacePlugin,
        >::new()),
        // Syntax plugins
        Box::new(NativePluginRule::<
            duplicate_directive_plugin::DuplicateDirectivePlugin,
        >::new()),
        Box::new(NativePluginRule::<
            invalid_directive_context_plugin::InvalidDirectiveContextPlugin,
        >::new()),
        // Best practices plugins
        Box::new(NativePluginRule::<
            add_header_inheritance_plugin::AddHeaderInheritancePlugin,
        >::new()),
        Box::new(NativePluginRule::<
            alias_location_slash_mismatch_plugin::AliasLocationSlashMismatchPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            client_max_body_size_not_set_plugin::ClientMaxBodySizeNotSetPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            gzip_not_enabled_plugin::GzipNotEnabledPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            if_is_evil_in_location_plugin::IfIsEvilInLocationPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            map_missing_default_plugin::MapMissingDefaultPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            missing_error_log_plugin::MissingErrorLogPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            proxy_keepalive_plugin::ProxyKeepalivePlugin,
        >::new()),
        Box::new(NativePluginRule::<
            proxy_missing_host_header_plugin::ProxyMissingHostHeaderPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            proxy_pass_domain_plugin::ProxyPassDomainPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            proxy_pass_with_uri_plugin::ProxyPassWithUriPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            proxy_set_header_inheritance_plugin::ProxySetHeaderInheritancePlugin,
        >::new()),
        Box::new(NativePluginRule::<
            root_in_location_plugin::RootInLocationPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            try_files_with_proxy_plugin::TryFilesWithProxyPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            unreachable_location_plugin::UnreachableLocationPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            upstream_server_no_resolve_plugin::UpstreamServerNoResolvePlugin,
        >::new()),
        // Deprecation plugins
        Box::new(NativePluginRule::<
            listen_http2_deprecated_plugin::ListenHttp2DeprecatedPlugin,
        >::new()),
        Box::new(NativePluginRule::<
            ssl_on_deprecated_plugin::SslOnDeprecatedPlugin,
        >::new()),
    ]
}
