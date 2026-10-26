//! Writing text from linted files and plugins to output read line by line.

use std::path::Path;

/// `text` on one line, with `\r` and `\n` written as the two characters `\r`
/// and `\n`.
///
/// Reports are read line by line, and in a GitHub Actions job the runner
/// takes every output line starting with `::` as a workflow command, whatever
/// printed it. Messages and paths come from the linted tree (the parser
/// decodes `\n` in quoted strings, and git allows newlines in file names), so
/// a newline in them could start such a line: a forged annotation, or a
/// `::stop-commands::` that hides the real ones.
pub fn one_line(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\r', '\n']) {
        text.replace('\r', "\\r").replace('\n', "\\n").into()
    } else {
        text.into()
    }
}

/// [`one_line`] for a path.
pub fn one_line_path(path: &Path) -> String {
    one_line(&path.display().to_string()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_escapes_line_breaks() {
        assert_eq!(
            one_line("a\n::stop-commands::zz\r\nb"),
            r"a\n::stop-commands::zz\r\nb"
        );
        assert!(matches!(
            one_line("plain"),
            std::borrow::Cow::Borrowed("plain")
        ));
        assert_eq!(
            one_line_path(Path::new("conf.d/x\n::y.conf")),
            r"conf.d/x\n::y.conf"
        );
    }
}
