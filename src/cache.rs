//! Cache directory resolution
//!
//! nginx-lint keeps all cacheable artifacts under a single cache root so
//! that the cache location can be configured in one place (the `cache_dir`
//! config value or the `--cache-dir` CLI flag). Each cache consumer owns a
//! subdirectory beneath the root; currently the only consumer is the WASM
//! plugin compilation cache under [`PLUGIN_CACHE_SUBDIR`].

use std::path::{Path, PathBuf};

/// Subdirectory under the cache root for the WASM plugin compilation cache
pub const PLUGIN_CACHE_SUBDIR: &str = "plugins";

/// Per-user default cache root for nginx-lint:
///
/// - Linux and other Unix: `$XDG_CACHE_HOME/nginx-lint` or `~/.cache/nginx-lint`
/// - macOS: `~/Library/Caches/nginx-lint`
/// - Windows: `%LOCALAPPDATA%\nginx-lint`
///
/// Returns `None` when the relevant environment variables are unset, empty,
/// or relative.
pub fn default_cache_root() -> Option<PathBuf> {
    cache_root_from(platform_cache_base())
}

/// Platform-specific per-user cache base directory from the environment
fn platform_cache_base() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        // The XDG spec says a relative $XDG_CACHE_HOME must be ignored
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }
}

/// Append the app directory to the cache base, rejecting non-absolute bases.
///
/// An empty or relative base (e.g. `HOME=""`) would otherwise be resolved
/// against the current working directory later on, silently creating a
/// cache tree wherever nginx-lint happens to run.
fn cache_root_from(base: Option<PathBuf>) -> Option<PathBuf> {
    base.filter(|base| base.is_absolute())
        .map(|base| base.join("nginx-lint"))
}

/// WASM plugin compilation cache directory under the given cache root
pub fn plugin_cache_dir(root: &Path) -> PathBuf {
    root.join(PLUGIN_CACHE_SUBDIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_cache_dir_is_under_root() {
        let dir = plugin_cache_dir(Path::new("/var/cache/nginx-lint"));
        assert_eq!(dir, PathBuf::from("/var/cache/nginx-lint/plugins"));
    }

    #[test]
    fn test_default_cache_root_ends_with_app_name() {
        // HOME (or LOCALAPPDATA on Windows) is set in any normal test
        // environment, so a root should resolve and be nginx-lint specific
        let root = default_cache_root().expect("cache root should resolve");
        assert!(root.ends_with("nginx-lint"));
    }

    #[test]
    fn test_cache_root_from_absolute_base() {
        let root = cache_root_from(Some(PathBuf::from("/home/user/.cache")));
        assert_eq!(root, Some(PathBuf::from("/home/user/.cache/nginx-lint")));
    }

    #[test]
    fn test_cache_root_from_rejects_relative_base() {
        // e.g. HOME="" produces the relative base ".cache" or "Library/Caches"
        assert_eq!(cache_root_from(Some(PathBuf::from(".cache"))), None);
        assert_eq!(cache_root_from(Some(PathBuf::from(""))), None);
    }

    #[test]
    fn test_cache_root_from_none() {
        assert_eq!(cache_root_from(None), None);
    }
}
