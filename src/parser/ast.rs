//! AST types for nginx configuration files
//!
//! Designed for round-trip support (source reconstruction) to enable autofix functionality.

/// Source position in the file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub offset: usize, // Byte offset for editing
}

impl Position {
    pub fn new(line: usize, column: usize, offset: usize) -> Self {
        Self {
            line,
            column,
            offset,
        }
    }
}

/// Source range (start and end positions)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// Root of the configuration file
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub items: Vec<ConfigItem>,
}

impl Config {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Returns an iterator over top-level directives (excludes comments and blank lines)
    pub fn directives(&self) -> impl Iterator<Item = &Directive> {
        self.items.iter().filter_map(|item| match item {
            ConfigItem::Directive(d) => Some(d.as_ref()),
            _ => None,
        })
    }

    /// Returns an iterator over all directives recursively (for lint rules)
    pub fn all_directives(&self) -> AllDirectives<'_> {
        AllDirectives::new(&self.items)
    }

    /// Reconstruct source code from AST (for autofix)
    pub fn to_source(&self) -> String {
        let mut output = String::new();
        for item in &self.items {
            item.write_source(&mut output, 0);
        }
        output
    }
}

/// An item in the configuration (directive, comment, or blank line)
#[derive(Debug, Clone)]
pub enum ConfigItem {
    Directive(Box<Directive>),
    Comment(Comment),
    BlankLine(Span),
}

impl ConfigItem {
    fn write_source(&self, output: &mut String, indent: usize) {
        match self {
            ConfigItem::Directive(d) => d.write_source(output, indent),
            ConfigItem::Comment(c) => {
                output.push_str(&"    ".repeat(indent));
                output.push_str(&c.text);
                output.push('\n');
            }
            ConfigItem::BlankLine(_) => {
                output.push('\n');
            }
        }
    }
}

/// A comment (# ...)
#[derive(Debug, Clone)]
pub struct Comment {
    pub text: String, // Includes the '#' character
    pub span: Span,
}

/// A directive (simple or block)
#[derive(Debug, Clone)]
pub struct Directive {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<Argument>,
    pub block: Option<Block>,
    pub span: Span,                         // Entire directive range
    pub trailing_comment: Option<Comment>,  // Comment at end of line
}

impl Directive {
    /// Check if this directive has a specific name
    pub fn is(&self, name: &str) -> bool {
        self.name == name
    }

    /// Get the first argument value as a string (useful for simple directives)
    pub fn first_arg(&self) -> Option<&str> {
        self.args.first().map(|a| a.as_str())
    }

    /// Check if the first argument equals a specific value
    pub fn first_arg_is(&self, value: &str) -> bool {
        self.first_arg() == Some(value)
    }

    fn write_source(&self, output: &mut String, indent: usize) {
        let indent_str = "    ".repeat(indent);
        output.push_str(&indent_str);
        output.push_str(&self.name);

        for arg in &self.args {
            output.push(' ');
            output.push_str(&arg.raw);
        }

        if let Some(block) = &self.block {
            output.push_str(" {\n");
            for item in &block.items {
                item.write_source(output, indent + 1);
            }
            output.push_str(&indent_str);
            output.push('}');
        } else {
            output.push(';');
        }

        if let Some(comment) = &self.trailing_comment {
            output.push(' ');
            output.push_str(&comment.text);
        }

        output.push('\n');
    }
}

/// A block { ... }
#[derive(Debug, Clone)]
pub struct Block {
    pub items: Vec<ConfigItem>,
    pub span: Span,
}

impl Block {
    /// Returns an iterator over directives in this block
    pub fn directives(&self) -> impl Iterator<Item = &Directive> {
        self.items.iter().filter_map(|item| match item {
            ConfigItem::Directive(d) => Some(d.as_ref()),
            _ => None,
        })
    }
}

/// A directive argument
#[derive(Debug, Clone)]
pub struct Argument {
    pub value: ArgumentValue,
    pub span: Span,
    pub raw: String, // Original source text (including quotes)
}

impl Argument {
    /// Get the string value (without quotes for quoted strings)
    pub fn as_str(&self) -> &str {
        match &self.value {
            ArgumentValue::Literal(s) => s,
            ArgumentValue::QuotedString(s) => s,
            ArgumentValue::SingleQuotedString(s) => s,
            ArgumentValue::Variable(s) => s,
        }
    }

    /// Check if this is an "on" value
    pub fn is_on(&self) -> bool {
        self.as_str() == "on"
    }

    /// Check if this is an "off" value
    pub fn is_off(&self) -> bool {
        self.as_str() == "off"
    }
}

/// Type of argument value
#[derive(Debug, Clone)]
pub enum ArgumentValue {
    Literal(String),            // on, off, 80, /path/to/file
    QuotedString(String),       // "hello world" -> hello world
    SingleQuotedString(String), // 'hello world' -> hello world
    Variable(String),           // $variable_name -> variable_name
}

/// Iterator over all directives in a config (recursively)
pub struct AllDirectives<'a> {
    stack: Vec<std::slice::Iter<'a, ConfigItem>>,
}

impl<'a> AllDirectives<'a> {
    fn new(items: &'a [ConfigItem]) -> Self {
        Self {
            stack: vec![items.iter()],
        }
    }
}

impl<'a> Iterator for AllDirectives<'a> {
    type Item = &'a Directive;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(iter) = self.stack.last_mut() {
            if let Some(item) = iter.next() {
                if let ConfigItem::Directive(directive) = item {
                    // If the directive has a block, push its items onto the stack
                    if let Some(block) = &directive.block {
                        self.stack.push(block.items.iter());
                    }
                    return Some(directive.as_ref());
                }
                // Skip comments and blank lines
            } else {
                // Current iterator is exhausted, pop it
                self.stack.pop();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_directives_iterator() {
        let config = Config {
            items: vec![
                ConfigItem::Directive(Box::new(Directive {
                    name: "worker_processes".to_string(),
                    name_span: Span::default(),
                    args: vec![Argument {
                        value: ArgumentValue::Literal("auto".to_string()),
                        span: Span::default(),
                        raw: "auto".to_string(),
                    }],
                    block: None,
                    span: Span::default(),
                    trailing_comment: None,
                })),
                ConfigItem::Directive(Box::new(Directive {
                    name: "http".to_string(),
                    name_span: Span::default(),
                    args: vec![],
                    block: Some(Block {
                        items: vec![
                            ConfigItem::Directive(Box::new(Directive {
                                name: "server".to_string(),
                                name_span: Span::default(),
                                args: vec![],
                                block: Some(Block {
                                    items: vec![ConfigItem::Directive(Box::new(Directive {
                                        name: "listen".to_string(),
                                        name_span: Span::default(),
                                        args: vec![Argument {
                                            value: ArgumentValue::Literal("80".to_string()),
                                            span: Span::default(),
                                            raw: "80".to_string(),
                                        }],
                                        block: None,
                                        span: Span::default(),
                                        trailing_comment: None,
                                    }))],
                                    span: Span::default(),
                                }),
                                span: Span::default(),
                                trailing_comment: None,
                            })),
                        ],
                        span: Span::default(),
                    }),
                    span: Span::default(),
                    trailing_comment: None,
                })),
            ],
        };

        let names: Vec<&str> = config.all_directives().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["worker_processes", "http", "server", "listen"]);
    }

    #[test]
    fn test_directive_helpers() {
        let directive = Directive {
            name: "server_tokens".to_string(),
            name_span: Span::default(),
            args: vec![Argument {
                value: ArgumentValue::Literal("on".to_string()),
                span: Span::default(),
                raw: "on".to_string(),
            }],
            block: None,
            span: Span::default(),
            trailing_comment: None,
        };

        assert!(directive.is("server_tokens"));
        assert!(!directive.is("gzip"));
        assert_eq!(directive.first_arg(), Some("on"));
        assert!(directive.first_arg_is("on"));
        assert!(directive.args[0].is_on());
        assert!(!directive.args[0].is_off());
    }
}
