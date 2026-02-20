//! Context-aware directive traversal.
//!
//! This module provides [`DirectiveWithContext`] and [`AllDirectivesWithContextIter`],
//! which perform depth-first traversal of the config AST while tracking the parent
//! block hierarchy (e.g., `["http", "server"]`).
//!
//! Obtained via [`Config::all_directives_with_context()`](crate::ast::Config::all_directives_with_context).

use crate::ast::{ConfigItem, Directive};

/// A directive paired with its parent block context.
///
/// Yielded by [`Config::all_directives_with_context()`](crate::ast::Config::all_directives_with_context).
/// Provides methods to query the parent block hierarchy without manually tracking nesting.
#[derive(Debug, Clone)]
pub struct DirectiveWithContext<'a> {
    /// The directive itself.
    pub directive: &'a Directive,
    /// Stack of parent directive names (e.g., `["http", "server"]`).
    pub parent_stack: Vec<String>,
    /// Nesting depth (0 = root level).
    pub depth: usize,
}

impl<'a> DirectiveWithContext<'a> {
    /// Get the immediate parent directive name, if any
    pub fn parent(&self) -> Option<&str> {
        self.parent_stack.last().map(|s| s.as_str())
    }

    /// Check if this directive is inside a specific parent context
    pub fn is_inside(&self, parent_name: &str) -> bool {
        self.parent_stack.iter().any(|p| p == parent_name)
    }

    /// Check if the immediate parent is a specific directive
    pub fn parent_is(&self, parent_name: &str) -> bool {
        self.parent() == Some(parent_name)
    }

    /// Check if this directive is at root level
    pub fn is_at_root(&self) -> bool {
        self.parent_stack.is_empty()
    }
}

/// Iterator over all directives with their parent context.
///
/// Obtained via [`Config::all_directives_with_context`](crate::ast::Config::all_directives_with_context).
/// Performs depth-first traversal, tracking parent block names as it descends.
pub struct AllDirectivesWithContextIter<'a> {
    stack: Vec<(std::slice::Iter<'a, ConfigItem>, Option<String>)>,
    current_parents: Vec<String>,
}

impl<'a> AllDirectivesWithContextIter<'a> {
    pub(crate) fn new(items: &'a [ConfigItem], initial_context: Vec<String>) -> Self {
        Self {
            stack: vec![(items.iter(), None)],
            current_parents: initial_context,
        }
    }
}

impl<'a> Iterator for AllDirectivesWithContextIter<'a> {
    type Item = DirectiveWithContext<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((iter, _)) = self.stack.last_mut() {
            if let Some(item) = iter.next() {
                if let ConfigItem::Directive(directive) = item {
                    let context = DirectiveWithContext {
                        directive: directive.as_ref(),
                        parent_stack: self.current_parents.clone(),
                        depth: self.current_parents.len(),
                    };

                    if let Some(block) = &directive.block {
                        self.current_parents.push(directive.name.clone());
                        self.stack
                            .push((block.items.iter(), Some(directive.name.clone())));
                    }

                    return Some(context);
                }
            } else {
                let (_, parent_name) = self.stack.pop().unwrap();
                if parent_name.is_some() {
                    self.current_parents.pop();
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Config;

    #[test]
    fn test_all_directives_with_context() {
        let config =
            crate::parse_string("http {\n    server {\n        listen 80;\n    }\n}").unwrap();

        let contexts: Vec<_> = config.all_directives_with_context().collect();
        assert_eq!(contexts.len(), 3);

        // http at root
        assert_eq!(contexts[0].directive.name, "http");
        assert!(contexts[0].is_at_root());
        assert_eq!(contexts[0].depth, 0);

        // server inside http
        assert_eq!(contexts[1].directive.name, "server");
        assert!(contexts[1].is_inside("http"));
        assert!(contexts[1].parent_is("http"));
        assert_eq!(contexts[1].depth, 1);

        // listen inside http > server
        assert_eq!(contexts[2].directive.name, "listen");
        assert!(contexts[2].is_inside("http"));
        assert!(contexts[2].is_inside("server"));
        assert!(contexts[2].parent_is("server"));
        assert_eq!(contexts[2].depth, 2);
    }

    #[test]
    fn test_all_directives_with_context_include_context() {
        let mut config = crate::parse_string("server {\n    listen 80;\n}").unwrap();
        config.include_context = vec!["http".to_string()];

        let contexts: Vec<_> = config.all_directives_with_context().collect();
        assert_eq!(contexts.len(), 2);

        // server is inside http (from include_context)
        assert_eq!(contexts[0].directive.name, "server");
        assert!(contexts[0].is_inside("http"));
        assert_eq!(contexts[0].depth, 1);

        // listen is inside http > server
        assert_eq!(contexts[1].directive.name, "listen");
        assert!(contexts[1].is_inside("http"));
        assert!(contexts[1].is_inside("server"));
        assert_eq!(contexts[1].depth, 2);
    }

    #[test]
    fn test_include_context_helpers() {
        let mut config = Config::new();
        assert!(!config.is_included_from_http());
        assert!(!config.is_included_from_stream());

        config.include_context = vec!["http".to_string()];
        assert!(config.is_included_from_http());
        assert!(config.is_included_from("http"));
        assert!(!config.is_included_from_stream());
        assert_eq!(config.immediate_parent_context(), Some("http"));

        config.include_context = vec!["http".to_string(), "server".to_string()];
        assert!(config.is_included_from_http());
        assert!(config.is_included_from_http_server());
        assert!(!config.is_included_from_http_location());

        config.include_context = vec!["http".to_string(), "location".to_string()];
        assert!(config.is_included_from_http_location());
    }
}
