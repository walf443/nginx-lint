//! WASM plugin integration tests
//!
//! These tests build WASM plugins as component model binaries and verify
//! that they work correctly when loaded through the host-side PluginLoader.
//!
//! Prerequisites:
//!   - `make build-plugins` must be run before these tests
//!   - Requires `--features plugins` to compile
//!
//! These tests are automatically skipped if WASM plugin binaries are not found.

#![cfg(feature = "plugins")]

use nginx_lint::parse_string;
use nginx_lint::plugin::PluginLoader;
use std::fs;
use std::path::{Path, PathBuf};

/// A discovered WASM plugin with its associated test files.
struct WasmPluginTestCase {
    /// Human-readable plugin name (e.g., "server_tokens_enabled")
    name: String,
    /// Path to the .wasm component binary
    wasm_path: PathBuf,
    /// Path to the plugin source directory (contains examples/, tests/)
    plugin_dir: PathBuf,
}

/// Discover all WASM plugins that have been built as component model binaries.
///
/// Scans `plugins/builtin/*/*/` for each plugin directory and looks for the
/// corresponding `.wasm.component.wasm` file in its target directory.
/// Returns only plugins that have been built (skips others silently).
fn discover_wasm_plugins() -> Vec<WasmPluginTestCase> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plugins_base = project_root.join("plugins").join("builtin");

    let mut plugins = Vec::new();

    // Scan plugins/builtin/{category}/{plugin_name}/
    let categories = match fs::read_dir(&plugins_base) {
        Ok(entries) => entries,
        Err(_) => return plugins,
    };

    for category_entry in categories.flatten() {
        if !category_entry.path().is_dir() {
            continue;
        }
        let category_plugins = match fs::read_dir(category_entry.path()) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for plugin_entry in category_plugins.flatten() {
            let plugin_dir = plugin_entry.path();
            if !plugin_dir.is_dir() || !plugin_dir.join("Cargo.toml").exists() {
                continue;
            }

            let plugin_name = plugin_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            // Convert plugin name to the WASM binary name (hyphens → underscores)
            let wasm_name = format!("{}_plugin", plugin_name.replace('-', "_"));
            let wasm_path = plugin_dir
                .join("target")
                .join("wasm32-unknown-unknown")
                .join("release")
                .join(format!("{wasm_name}.wasm.component.wasm"));

            if wasm_path.exists() {
                plugins.push(WasmPluginTestCase {
                    name: plugin_name,
                    wasm_path,
                    plugin_dir,
                });
            }
        }
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

/// Skip the test if no WASM plugins have been built.
/// Returns (loader, plugins) if available.
fn setup() -> Option<(PluginLoader, Vec<WasmPluginTestCase>)> {
    let plugins = discover_wasm_plugins();
    if plugins.is_empty() {
        eprintln!("SKIP: No WASM plugin binaries found. Run `make build-plugins` first.");
        return None;
    }
    let loader = PluginLoader::new().expect("Failed to create PluginLoader");
    Some((loader, plugins))
}

#[test]
fn test_wasm_plugins_load_successfully() {
    let Some((loader, plugins)) = setup() else {
        return;
    };

    let mut loaded = 0;
    for case in &plugins {
        let result = loader.load_plugin_dynamic(&case.wasm_path);
        match result {
            Ok(_) => {}
            Err(e) => panic!("Failed to load WASM plugin '{}': {}", case.name, e),
        }
        loaded += 1;
    }
    eprintln!("Loaded {loaded} WASM plugins successfully.");
}

#[test]
fn test_wasm_plugins_detect_errors_in_bad_examples() {
    let Some((loader, plugins)) = setup() else {
        return;
    };

    let mut tested = 0;
    for case in &plugins {
        let bad_conf_path = case.plugin_dir.join("examples").join("bad.conf");
        if !bad_conf_path.exists() {
            continue;
        }

        let rule = loader
            .load_plugin_dynamic(&case.wasm_path)
            .unwrap_or_else(|e| panic!("Failed to load '{}': {}", case.name, e));

        let bad_conf = fs::read_to_string(&bad_conf_path)
            .unwrap_or_else(|e| panic!("Failed to read bad.conf for '{}': {}", case.name, e));

        let config = parse_string(&bad_conf)
            .unwrap_or_else(|e| panic!("Failed to parse bad.conf for '{}': {}", case.name, e));

        let errors = rule.check(&config, Path::new("bad.conf"));
        assert!(
            !errors.is_empty(),
            "Plugin '{}': bad.conf should produce at least one error, but got none.\n\
             Config:\n{}",
            case.name,
            bad_conf,
        );
        tested += 1;
    }
    eprintln!("Tested {tested} plugins with bad.conf examples.");
}

#[test]
fn test_wasm_plugins_no_errors_in_good_examples() {
    let Some((loader, plugins)) = setup() else {
        return;
    };

    let mut tested = 0;
    for case in &plugins {
        let good_conf_path = case.plugin_dir.join("examples").join("good.conf");
        if !good_conf_path.exists() {
            continue;
        }

        let rule = loader
            .load_plugin_dynamic(&case.wasm_path)
            .unwrap_or_else(|e| panic!("Failed to load '{}': {}", case.name, e));

        let good_conf = fs::read_to_string(&good_conf_path)
            .unwrap_or_else(|e| panic!("Failed to read good.conf for '{}': {}", case.name, e));

        let config = parse_string(&good_conf)
            .unwrap_or_else(|e| panic!("Failed to parse good.conf for '{}': {}", case.name, e));

        let errors = rule.check(&config, Path::new("good.conf"));

        // Filter to only errors from this rule (ignore unrelated warnings)
        let rule_name = rule.name();
        let relevant_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();

        assert!(
            relevant_errors.is_empty(),
            "Plugin '{}': good.conf should produce no errors, but got {} error(s):\n{:#?}",
            case.name,
            relevant_errors.len(),
            relevant_errors,
        );
        tested += 1;
    }
    eprintln!("Tested {tested} plugins with good.conf examples.");
}

#[test]
fn test_wasm_plugins_fixtures_detect_errors() {
    let Some((loader, plugins)) = setup() else {
        return;
    };

    let mut tested = 0;
    for case in &plugins {
        let fixtures_dir = case.plugin_dir.join("tests").join("fixtures");
        if !fixtures_dir.exists() {
            continue;
        }

        let rule = loader
            .load_plugin_dynamic(&case.wasm_path)
            .unwrap_or_else(|e| panic!("Failed to load '{}': {}", case.name, e));

        let rule_name = rule.name().to_string();

        let fixture_cases = match fs::read_dir(&fixtures_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for fixture_entry in fixture_cases.flatten() {
            let fixture_dir = fixture_entry.path();
            if !fixture_dir.is_dir() {
                continue;
            }

            let fixture_name = fixture_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            // Test error/nginx.conf produces errors
            let error_conf_path = fixture_dir.join("error").join("nginx.conf");
            if error_conf_path.exists() {
                let error_conf = fs::read_to_string(&error_conf_path).unwrap_or_else(|e| {
                    panic!(
                        "Failed to read error fixture for '{}/{}': {}",
                        case.name, fixture_name, e
                    )
                });

                let config = parse_string(&error_conf).unwrap_or_else(|e| {
                    panic!(
                        "Failed to parse error fixture for '{}/{}': {}",
                        case.name, fixture_name, e
                    )
                });

                let errors = rule.check(&config, Path::new("nginx.conf"));
                let relevant_errors: Vec<_> =
                    errors.iter().filter(|e| e.rule == rule_name).collect();

                assert!(
                    !relevant_errors.is_empty(),
                    "Plugin '{}' fixture '{}': error/nginx.conf should produce errors, but got none.",
                    case.name,
                    fixture_name,
                );
                tested += 1;
            }
        }
    }
    eprintln!("Tested {tested} fixture cases for error detection.");
}

#[test]
fn test_wasm_plugins_fixtures_fix_application() {
    let Some((loader, plugins)) = setup() else {
        return;
    };

    let mut tested = 0;
    for case in &plugins {
        let fixtures_dir = case.plugin_dir.join("tests").join("fixtures");
        if !fixtures_dir.exists() {
            continue;
        }

        let rule = loader
            .load_plugin_dynamic(&case.wasm_path)
            .unwrap_or_else(|e| panic!("Failed to load '{}': {}", case.name, e));

        let rule_name = rule.name().to_string();

        let fixture_cases = match fs::read_dir(&fixtures_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for fixture_entry in fixture_cases.flatten() {
            let fixture_dir = fixture_entry.path();
            if !fixture_dir.is_dir() {
                continue;
            }

            let fixture_name = fixture_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            let error_conf_path = fixture_dir.join("error").join("nginx.conf");

            if !error_conf_path.exists() {
                continue;
            }

            let error_conf = fs::read_to_string(&error_conf_path).unwrap();

            let config = parse_string(&error_conf).unwrap();
            let errors = rule.check(&config, Path::new("nginx.conf"));
            let relevant_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();

            // Collect fixes from relevant errors
            let fixes: Vec<_> = relevant_errors
                .iter()
                .flat_map(|e| e.fixes.iter())
                .collect();

            if fixes.is_empty() {
                continue;
            }

            let (result, fix_count) = nginx_lint::apply_fixes_to_content(&error_conf, &fixes);

            assert!(
                fix_count > 0,
                "Plugin '{}' fixture '{}': expected fixes to be applied, but none were.",
                case.name,
                fixture_name,
            );

            // Verify the fixed config no longer triggers the rule
            let fixed_config = parse_string(&result).unwrap_or_else(|e| {
                panic!(
                    "Plugin '{}' fixture '{}': fixed config failed to parse: {}\n\
                     Fixed content:\n{}",
                    case.name, fixture_name, e, result,
                )
            });
            let remaining_errors = rule.check(&fixed_config, Path::new("nginx.conf"));
            let remaining_relevant: Vec<_> = remaining_errors
                .iter()
                .filter(|e| e.rule == rule_name)
                .collect();

            assert!(
                remaining_relevant.is_empty(),
                "Plugin '{}' fixture '{}': after applying fixes, rule still produces {} error(s):\n{:#?}\n\
                 Fixed content:\n{}",
                case.name,
                fixture_name,
                remaining_relevant.len(),
                remaining_relevant,
                result,
            );
            tested += 1;
        }
    }
    eprintln!("Tested {tested} fixture cases for fix application.");
}

#[test]
fn test_wasm_plugins_example_fix_produces_good() {
    let Some((loader, plugins)) = setup() else {
        return;
    };

    let mut tested = 0;
    for case in &plugins {
        let bad_conf_path = case.plugin_dir.join("examples").join("bad.conf");
        let good_conf_path = case.plugin_dir.join("examples").join("good.conf");

        if !bad_conf_path.exists() || !good_conf_path.exists() {
            continue;
        }

        let rule = loader
            .load_plugin_dynamic(&case.wasm_path)
            .unwrap_or_else(|e| panic!("Failed to load '{}': {}", case.name, e));

        let rule_name = rule.name().to_string();

        let bad_conf = fs::read_to_string(&bad_conf_path).unwrap();
        let good_conf = fs::read_to_string(&good_conf_path).unwrap();

        let config = parse_string(&bad_conf).unwrap();
        let errors = rule.check(&config, Path::new("bad.conf"));
        let relevant_errors: Vec<_> = errors.iter().filter(|e| e.rule == rule_name).collect();

        let fixes: Vec<_> = relevant_errors
            .iter()
            .flat_map(|e| e.fixes.iter())
            .collect();

        if fixes.is_empty() {
            continue;
        }

        let (result, fix_count) = nginx_lint::apply_fixes_to_content(&bad_conf, &fixes);

        assert!(
            fix_count > 0,
            "Plugin '{}': expected fixes to be applied to bad.conf, but none were.",
            case.name,
        );

        // Verify that the fixed config no longer triggers the rule
        let fixed_config = parse_string(&result).unwrap_or_else(|e| {
            panic!(
                "Plugin '{}': fixed bad.conf failed to parse: {}\nFixed content:\n{}",
                case.name, e, result,
            )
        });
        let remaining_errors = rule.check(&fixed_config, Path::new("bad.conf"));
        let remaining_relevant: Vec<_> = remaining_errors
            .iter()
            .filter(|e| e.rule == rule_name)
            .collect();

        assert!(
            remaining_relevant.is_empty(),
            "Plugin '{}': after applying fixes to bad.conf, rule still produces {} error(s):\n{:#?}\n\
             Fixed content:\n{}",
            case.name,
            remaining_relevant.len(),
            remaining_relevant,
            result,
        );

        // Check if the result exactly matches good.conf
        if result != good_conf {
            eprintln!(
                "INFO: Plugin '{}': fixed bad.conf does not exactly match good.conf \
                 (good.conf may contain additional examples). Errors are resolved.",
                case.name,
            );
        }
        tested += 1;
    }
    eprintln!("Tested {tested} plugins with bad.conf fix verification.");
}
