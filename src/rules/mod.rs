pub mod best_practices;
pub mod security;
pub mod style;
pub mod syntax;

pub use best_practices::{GzipNotEnabled, MissingErrorLog};
pub use security::{AutoindexEnabled, DeprecatedSslProtocol, ServerTokensEnabled};
pub use style::InconsistentIndentation;
pub use syntax::{DuplicateDirective, MissingSemicolon, UnmatchedBraces};
