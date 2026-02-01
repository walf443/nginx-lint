pub mod best_practices;
pub mod security;
pub mod style;
pub mod syntax;

pub use best_practices::{GzipNotEnabled, MissingErrorLog};
pub use security::{AutoindexEnabled, DeprecatedSslProtocol, ServerTokensEnabled, WeakSslCiphers};
pub use style::{InconsistentIndentation, TrailingWhitespace};
pub use syntax::{DuplicateDirective, MissingSemicolon, UnclosedQuote, UnmatchedBraces};
