/**
 * Current API version of the plugin interface.
 *
 * `defineRules` stamps it into every rule's spec that does not set
 * `apiVersion` itself. Informational only: compatibility is enforced
 * structurally by WIT import resolution (a plugin built against a newer SDK
 * fails to instantiate on an older host). Kept in sync with the Rust SDK's
 * `API_VERSION` in crates/nginx-lint-plugin/src/types.rs, which a host test
 * checks by reading this file.
 */
export const API_VERSION = "1.2";
