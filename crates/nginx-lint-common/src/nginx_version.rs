//! nginx version parsing and comparison.
//!
//! Supports the canonical `major.minor.patch` form (e.g. `"1.30.1"`).
//! Used by the linter to filter rules whose declared
//! [`min_nginx_version`](crate::linter::LintRule::min_nginx_version) /
//! [`max_nginx_version`](crate::linter::LintRule::max_nginx_version) range
//! does not include the user-configured
//! [`target_nginx_version`](crate::config::LintConfig).

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A parsed nginx version triple (`major.minor.patch`).
///
/// nginx releases follow `major.minor.patch` (e.g. `1.30.1`); this struct
/// stores those three components and orders them lexicographically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NginxVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl NginxVersion {
    /// Construct a version from its three components.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a version string like `"1.30.1"`.
    pub fn parse(s: &str) -> Result<Self, NginxVersionParseError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(NginxVersionParseError::InvalidFormat(s.to_string()));
        }
        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| NginxVersionParseError::InvalidComponent(s.to_string()))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| NginxVersionParseError::InvalidComponent(s.to_string()))?;
        let patch = parts[2]
            .parse::<u32>()
            .map_err(|_| NginxVersionParseError::InvalidComponent(s.to_string()))?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl Ord for NginxVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl PartialOrd for NginxVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for NginxVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for NginxVersion {
    type Err = NginxVersionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Returns true when `version` falls within the optional inclusive bounds.
///
/// A `None` bound means "unbounded" on that side.
///
/// # Panics
///
/// In debug builds, panics if both bounds are supplied and `min > max`
/// (i.e. the range is empty). A reversed range is almost always a plugin
/// author mistake; failing fast in tests catches the bug at the source
/// rather than silently filtering every rule out at runtime.
pub fn is_in_range(
    version: &NginxVersion,
    min: Option<&NginxVersion>,
    max: Option<&NginxVersion>,
) -> bool {
    if let (Some(min), Some(max)) = (min, max) {
        debug_assert!(
            min <= max,
            "is_in_range called with reversed bounds: min={} > max={}",
            min,
            max
        );
    }
    if let Some(min) = min
        && version < min
    {
        return false;
    }
    if let Some(max) = max
        && version > max
    {
        return false;
    }
    true
}

/// Format an optional `(min, max)` nginx version pair as a human-readable
/// range. Returns `None` when both bounds are unset.
///
/// Uses `>=` / `<=` comparison-operator notation rather than Rust's `..=`
/// inclusive-range syntax — most nginx-lint users are not Rust developers
/// and `>=`/`<=` is the same notation npm, pip, and similar tools use for
/// version constraints.
///
/// # Examples
///
/// ```
/// use nginx_lint_common::nginx_version::format_range;
///
/// assert_eq!(
///     format_range(Some("0.6.27"), Some("1.30.0")),
///     Some("nginx >=0.6.27, <=1.30.0".to_string())
/// );
/// assert_eq!(format_range(Some("1.0.0"), None), Some("nginx >=1.0.0".to_string()));
/// assert_eq!(format_range(None, Some("1.30.0")), Some("nginx <=1.30.0".to_string()));
/// assert_eq!(format_range(None, None), None);
/// ```
pub fn format_range(min: Option<&str>, max: Option<&str>) -> Option<String> {
    match (min, max) {
        (Some(min), Some(max)) => Some(format!("nginx >={}, <={}", min, max)),
        (Some(min), None) => Some(format!("nginx >={}", min)),
        (None, Some(max)) => Some(format!("nginx <={}", max)),
        (None, None) => None,
    }
}

/// Error returned by [`NginxVersion::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NginxVersionParseError {
    /// The string did not have the expected three dot-separated components.
    InvalidFormat(String),
    /// One of the three components was not a valid `u32`.
    InvalidComponent(String),
}

impl fmt::Display for NginxVersionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(s) => write!(
                f,
                "invalid nginx version '{}': expected major.minor.patch format",
                s
            ),
            Self::InvalidComponent(s) => write!(
                f,
                "invalid nginx version '{}': components must be non-negative integers",
                s
            ),
        }
    }
}

impl std::error::Error for NginxVersionParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_versions() {
        assert_eq!(
            NginxVersion::parse("1.30.0").unwrap(),
            NginxVersion::new(1, 30, 0)
        );
        assert_eq!(
            NginxVersion::parse("0.0.1").unwrap(),
            NginxVersion::new(0, 0, 1)
        );
        assert_eq!(
            NginxVersion::parse("10.20.30").unwrap(),
            NginxVersion::new(10, 20, 30)
        );
    }

    #[test]
    fn parse_rejects_two_components() {
        assert!(matches!(
            NginxVersion::parse("1.30"),
            Err(NginxVersionParseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn parse_rejects_four_components() {
        assert!(matches!(
            NginxVersion::parse("1.30.0.1"),
            Err(NginxVersionParseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn parse_rejects_v_prefix() {
        assert!(matches!(
            NginxVersion::parse("v1.30.0"),
            Err(NginxVersionParseError::InvalidComponent(_))
        ));
    }

    #[test]
    fn parse_rejects_non_numeric_component() {
        assert!(matches!(
            NginxVersion::parse("1.30.x"),
            Err(NginxVersionParseError::InvalidComponent(_))
        ));
        assert!(matches!(
            NginxVersion::parse("1.a.0"),
            Err(NginxVersionParseError::InvalidComponent(_))
        ));
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(NginxVersion::parse("").is_err());
    }

    #[test]
    fn ordering() {
        let v_1_30_0 = NginxVersion::new(1, 30, 0);
        let v_1_30_1 = NginxVersion::new(1, 30, 1);
        let v_1_31_0 = NginxVersion::new(1, 31, 0);
        let v_2_0_0 = NginxVersion::new(2, 0, 0);
        assert!(v_1_30_0 < v_1_30_1);
        assert!(v_1_30_1 < v_1_31_0);
        assert!(v_1_31_0 < v_2_0_0);
        assert_eq!(v_1_30_0, NginxVersion::new(1, 30, 0));
    }

    #[test]
    fn display() {
        assert_eq!(NginxVersion::new(1, 30, 1).to_string(), "1.30.1");
    }

    #[test]
    fn range_unbounded() {
        let v = NginxVersion::new(1, 30, 0);
        assert!(is_in_range(&v, None, None));
    }

    #[test]
    fn range_min_only() {
        let min = NginxVersion::new(1, 0, 0);
        assert!(is_in_range(&NginxVersion::new(1, 0, 0), Some(&min), None));
        assert!(is_in_range(&NginxVersion::new(2, 0, 0), Some(&min), None));
        assert!(!is_in_range(&NginxVersion::new(0, 9, 0), Some(&min), None));
    }

    #[test]
    fn range_max_only() {
        let max = NginxVersion::new(1, 30, 0);
        assert!(is_in_range(&NginxVersion::new(1, 30, 0), None, Some(&max)));
        assert!(is_in_range(&NginxVersion::new(1, 0, 0), None, Some(&max)));
        assert!(!is_in_range(&NginxVersion::new(1, 30, 1), None, Some(&max)));
        assert!(!is_in_range(&NginxVersion::new(1, 31, 0), None, Some(&max)));
    }

    #[test]
    fn range_both_bounds_inclusive() {
        let min = NginxVersion::new(0, 6, 27);
        let max = NginxVersion::new(1, 30, 0);
        assert!(is_in_range(
            &NginxVersion::new(0, 6, 27),
            Some(&min),
            Some(&max)
        ));
        assert!(is_in_range(
            &NginxVersion::new(1, 30, 0),
            Some(&min),
            Some(&max)
        ));
        assert!(is_in_range(
            &NginxVersion::new(1, 0, 0),
            Some(&min),
            Some(&max)
        ));
        assert!(!is_in_range(
            &NginxVersion::new(0, 6, 26),
            Some(&min),
            Some(&max)
        ));
        assert!(!is_in_range(
            &NginxVersion::new(1, 30, 1),
            Some(&min),
            Some(&max)
        ));
    }

    #[test]
    fn from_str_works() {
        let v: NginxVersion = "1.30.1".parse().unwrap();
        assert_eq!(v, NginxVersion::new(1, 30, 1));
    }

    #[test]
    #[should_panic(expected = "reversed bounds")]
    fn range_panics_on_reversed_bounds_in_debug() {
        // Plugin author mistakes (e.g. swapping min/max) would silently
        // filter every version out at runtime; debug_assert! catches it
        // in tests instead. Only fires in debug builds.
        let min = NginxVersion::new(2, 0, 0);
        let max = NginxVersion::new(1, 0, 0);
        let v = NginxVersion::new(1, 5, 0);
        is_in_range(&v, Some(&min), Some(&max));
    }

    #[test]
    fn range_equal_bounds_is_allowed() {
        // min == max is a valid single-point range, not a mistake.
        let exact = NginxVersion::new(1, 30, 0);
        assert!(is_in_range(
            &NginxVersion::new(1, 30, 0),
            Some(&exact),
            Some(&exact)
        ));
        assert!(!is_in_range(
            &NginxVersion::new(1, 30, 1),
            Some(&exact),
            Some(&exact)
        ));
    }

    #[test]
    fn format_range_both_bounds() {
        assert_eq!(
            format_range(Some("0.6.27"), Some("1.30.0")),
            Some("nginx >=0.6.27, <=1.30.0".to_string())
        );
    }

    #[test]
    fn format_range_min_only() {
        assert_eq!(
            format_range(Some("1.0.0"), None),
            Some("nginx >=1.0.0".to_string())
        );
    }

    #[test]
    fn format_range_max_only() {
        assert_eq!(
            format_range(None, Some("1.29.6")),
            Some("nginx <=1.29.6".to_string())
        );
    }

    #[test]
    fn format_range_none() {
        // Rules with no declared range produce no string at all so callers
        // can use `if let Some(range) = ...` to skip the display section.
        assert_eq!(format_range(None, None), None);
    }
}
