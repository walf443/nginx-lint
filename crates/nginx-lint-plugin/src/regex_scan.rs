//! A single pass over PCRE syntax, shared by every rule that reasons about
//! capture groups.
//!
//! nginx stores unnamed captures in the shared `$1`..`$9` slots, so rules need
//! to tell a real capture group apart from a paren that merely looks like one —
//! and rules that autofix rewrite source bytes at the offsets found here, so a
//! paren misjudged is a user's config corrupted.
//!
//! # Why one walker
//!
//! This used to be two: one to find unnamed captures, one to answer "does this
//! regex have a named capture at all". They drifted, as duplicated parsers do.
//! `(?'name'...)` was taught to the first and not the second, which defeated
//! nginx_rift's mixed-named/unnamed guard and silently renumbered a live
//! `rewrite`'s captures. Later, `\Q...\E`, `(?#...)` comments and POSIX classes
//! were taught to the first and not the second, so the second still claimed
//! `\Q(?<n>x)\E` had a named group.
//!
//! [`scan`] is now the only place that knows PCRE syntax; everything else is a
//! filter over its output, so a construct learned once is learned by all.
//!
//! # Scope
//!
//! This classifies parens. It does not validate the regex — an input PCRE
//! rejects may produce anything, which is fine because nginx would refuse such
//! a config anyway.
//!
//! It also tracks no nesting, which bounds one case deliberately: `(?n)` (PCRE2
//! 10.43+) turns off auto-capture until the end of the *enclosing group*, and
//! `(?n:...)` only within its own. Honouring that needs a group stack, so it is
//! not honoured — `(?n)(a)` is reported as an unnamed capture though PCRE gives
//! it none.
//!
//! That over-reports, which is the safe direction and costs nothing real: the
//! spurious warning lands on a regex that has no captures to leak, and the
//! rewrite it invites, `(?n)(a)` -> `(?n)(?:a)`, has the same zero captures and
//! still loads (checked on nginx 1.29). A global flag was tried and rejected:
//! it made `(?n:(a))(b)` report *no* captures where PCRE has one, turning a
//! harmless over-report into a security rule missing a real capture.

/// What a `(` in the pattern opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// `(...)` — a capture reachable only as `$1`..`$9`.
    Unnamed,
    /// `(?<n>...)`, `(?'n'...)`, `(?P<n>...)` — reachable by name. Still a
    /// capturing group as far as PCRE's count is concerned.
    Named,
}

/// Every capture group in `regex`, as (byte offset of its `(`, kind), in source
/// order.
///
/// Skipped, because PCRE does not read them as capture groups: escapes (`\(`),
/// `\Q...\E` literal spans, character classes (including a leading `]` member,
/// negated `[^...]`, and nested POSIX classes like `[[:^alpha:]]`), the
/// non-capturing `(?...)` family — `(?:...)`, lookarounds, atomic `(?>...)`,
/// inline modifiers, conditionals `(?(1)...)` — comments `(?#...)`, and the
/// `(*VERB)` / `(*MARK:name)` family.
///
/// Comment and verb bodies are opaque text ending at the first `)`, and are
/// stepped over rather than scanned: they can contain anything, and a stray `[`
/// or `\Q` in one would otherwise open a class or literal span that never
/// closes, hiding every real capture after it.
///
/// # Examples
///
/// ```
/// use nginx_lint_plugin::regex_scan::{scan, Group};
///
/// assert_eq!(scan("(a)(?<n>b)(?:c)"), vec![(0, Group::Unnamed), (3, Group::Named)]);
/// assert!(scan(r"\Q(?<n>x)\E").is_empty());
/// assert!(scan("[[:^alpha:]()]").is_empty());
/// ```
pub fn scan(regex: &str) -> Vec<(usize, Group)> {
    let bytes = regex.as_bytes();
    let mut groups = Vec::new();
    let mut i = 0;
    let mut in_char_class = false;

    // Byte offset just past `[` (and an optional negating `^`), where a `]` is
    // a literal member rather than the class terminator.
    let mut class_body_start = 0;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'\\' && i + 1 < bytes.len() {
            // `\Q...\E` quotes everything up to `\E` (or end of pattern).
            if bytes[i + 1] == b'Q' {
                i = find_literal_span_end(bytes, i + 2);
                continue;
            }
            // Any other escape — skip both bytes.
            i += 2;
            continue;
        }

        if in_char_class {
            // A POSIX class such as `[:alpha:]` nests inside the class, and its
            // `]` does not end the enclosing one.
            if b == b'['
                && bytes.get(i + 1) == Some(&b':')
                && let Some(end) = find_posix_class_end(bytes, i + 2)
            {
                i = end;
                continue;
            }
            // PCRE reads `]` as a member when it opens the class body, so only
            // a later one closes it. `[]()]`, `[^]()]` are single classes.
            if b == b']' && i > class_body_start {
                in_char_class = false;
            }
            i += 1;
            continue;
        }

        if b == b'[' {
            in_char_class = true;
            class_body_start = i + 1;
            if bytes.get(class_body_start) == Some(&b'^') {
                class_body_start += 1;
            }
            i += 1;
            continue;
        }

        if b == b'(' {
            match bytes.get(i + 1).copied() {
                Some(b'*') => {
                    // `(*` spells two unrelated things. A control verb or
                    // option setting — `(*PRUNE)`, `(*MARK:name)`, `(*UTF)`,
                    // `(*:name)` — carries arbitrary text, so it is opaque. But
                    // `(*pla:...)`, `(*atomic:...)`, `(*sr:...)` and friends are
                    // *groups*, spelled with a lowercase name, and their bodies
                    // are regex holding real captures. Skipping those as if they
                    // were verbs hid every capture inside them.
                    if is_alpha_group_prefix(bytes, i + 2) {
                        i += 2;
                    } else {
                        i = find_close_paren(bytes, i + 2);
                    }
                    continue;
                }
                Some(b'?') => match bytes.get(i + 2).copied() {
                    // `(?#...)` is a comment; its body is arbitrary text.
                    Some(b'#') => {
                        i = find_close_paren(bytes, i + 3);
                        continue;
                    }
                    // A callout with a string argument — `(?C'...'`, `(?C"..."`,
                    // `` (?C`...` ``, `(?C{...}` etc. Like a comment, the body is
                    // arbitrary text: a `[` or `\Q` in it would otherwise open a
                    // class or literal span that never closes. Numeric callouts
                    // `(?C1)` have no body and need no special case.
                    Some(b'C')
                        if bytes
                            .get(i + 3)
                            .is_some_and(|c| !c.is_ascii_digit() && *c != b')') =>
                    {
                        i = find_close_paren(bytes, i + 3);
                        continue;
                    }
                    // A conditional `(?(1)...)` / `(?(<name>)...)` nests a paren
                    // that is syntax, not a group.
                    Some(b'(') => {
                        i += 3;
                        continue;
                    }
                    // `(?'name'...)`.
                    Some(b'\'') => groups.push((i, Group::Named)),
                    // `(?<name>...)`. Not `(?<=...)` / `(?<!...)` (lookbehinds)
                    // nor `(?<*...)` (non-atomic lookbehind) — those capture
                    // nothing.
                    Some(b'<')
                        if !matches!(
                            bytes.get(i + 3),
                            Some(b'=') | Some(b'!') | Some(b'*') | None
                        ) =>
                    {
                        groups.push((i, Group::Named))
                    }
                    // `(?P<name>...)`.
                    Some(b'P') if bytes.get(i + 3) == Some(&b'<') => groups.push((i, Group::Named)),
                    // Every other `(?...)` construct captures nothing, but its
                    // body is regex and must still be scanned.
                    _ => {}
                },
                _ => groups.push((i, Group::Unnamed)),
            }
        }

        i += 1;
    }

    groups
}

/// Whether `(*` at `from - 2` introduces a *group* rather than a control verb.
///
/// PCRE spells both with `(*`. Groups use a lowercase name followed by `:` —
/// `(*pla:`, `(*plb:`, `(*nla:`, `(*nlb:`, `(*napla:`, `(*naplb:`,
/// `(*atomic:`, `(*sr:`, `(*asr:`, and the long forms
/// (`(*positive_lookahead:`, `(*script_run:`, `(*atomic_script_run:`, ...).
/// Their bodies are regex and hold real captures.
///
/// Verbs and option settings use uppercase (`(*PRUNE)`, `(*MARK:name)`,
/// `(*UTF)`) or no name at all (`(*:name)`), and carry arbitrary text.
///
/// Underscores appear in the long forms, so they count as part of a name.
fn is_alpha_group_prefix(bytes: &[u8], from: usize) -> bool {
    let mut i = from;
    while bytes
        .get(i)
        .is_some_and(|b| b.is_ascii_lowercase() || *b == b'_')
    {
        i += 1;
    }
    i > from && bytes.get(i) == Some(&b':')
}

/// Offset just past the next `)` at or after `from`, or the end of the input
/// when there is none. Used for constructs whose body is opaque text — a
/// `(?#...)` comment or a `(*VERB:name)` — which PCRE ends at the first `)`.
///
/// **Escapes are deliberately not honoured, and must not be.** PCRE gives no
/// way to put a `)` inside these: a comment ends at the first one even when a
/// backslash precedes it, so `a(?#x\)b` is `a`, a comment, then `b` — it
/// matches `ab`. Teaching this function to skip `\)` would desynchronise it
/// from PCRE and hide the group that follows. Pinned by
/// `comment_ends_at_first_paren_even_after_a_backslash`.
fn find_close_paren(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b')' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// Offset just past the `:]` closing a POSIX class whose body starts at `from`,
/// or `None` when there is none — in which case the `[` was an ordinary member
/// and should be treated as such.
fn find_posix_class_end(bytes: &[u8], from: usize) -> Option<usize> {
    // `[[:^alpha:]]` negates the class; the `^` is part of the syntax, not the
    // name.
    let mut i = from + usize::from(bytes.get(from) == Some(&b'^'));

    while i + 1 < bytes.len() {
        if bytes[i] == b':' && bytes[i + 1] == b']' {
            return Some(i + 2);
        }
        // A POSIX class name is alphabetic; anything else means this was not
        // one, e.g. a literal `[` followed by `:` in the class.
        if !bytes[i].is_ascii_alphabetic() {
            return None;
        }
        i += 1;
    }

    None
}

/// Offset just past the `\E` closing a `\Q` literal span that starts at `from`,
/// or the end of the pattern when it is unterminated.
fn find_literal_span_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' && bytes[i + 1] == b'E' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unnamed(regex: &str) -> Vec<usize> {
        scan(regex)
            .into_iter()
            .filter(|(_, g)| *g == Group::Unnamed)
            .map(|(pos, _)| pos)
            .collect()
    }

    fn named(regex: &str) -> bool {
        scan(regex).iter().any(|(_, g)| *g == Group::Named)
    }

    /// The whole point of the merge: every construct below is one the two old
    /// walkers disagreed about. A single `scan` cannot disagree with itself.
    #[test]
    fn both_views_agree_on_constructs_that_used_to_diverge() {
        // `\Q...\E` is literal text: no groups of either kind.
        assert!(unnamed(r"\Q(a)\E").is_empty());
        assert!(!named(r"\Q(?<n>x)\E"));

        // A comment body is opaque: no groups of either kind.
        assert!(unnamed("(?#(a)x").is_empty());
        assert!(!named("(?#(?<n)x"));

        // Verb names are arbitrary text.
        assert!(unnamed("(*MARK:(a))").is_empty());
        assert!(!named("(*MARK:(?<n>x))"));

        // Character classes, including the awkward forms.
        assert!(unnamed("[]()]").is_empty());
        assert!(!named("[(?<x>]"));
        assert!(unnamed("[[:^print:]()]").is_empty());
    }

    #[test]
    fn all_three_named_syntaxes_are_recognised() {
        assert!(named("(?<n>x)"));
        assert!(named("(?'n'x)"));
        assert!(named("(?P<n>x)"));

        // Lookbehinds are not captures.
        assert!(!named("(?<=foo)bar"));
        assert!(!named("(?<!foo)bar"));
        // Truncated input must not be read as a name.
        assert!(!named("(?<"));
    }

    #[test]
    fn mixed_groups_keep_their_order_and_kind() {
        assert_eq!(
            scan("(a)(?<n>b)(?:c)(?'m'd)(e)"),
            vec![
                (0, Group::Unnamed),
                (3, Group::Named),
                (15, Group::Named),
                (22, Group::Unnamed),
            ]
        );
    }

    /// PCRE gives no way to put a `)` inside a `(?#...)` comment — the first one
    /// ends it, backslash or not (`a(?#x\)b` matches "ab"). So the group after
    /// it is real, and skipping `\)` here would hide it.
    #[test]
    fn comment_ends_at_first_paren_even_after_a_backslash() {
        assert_eq!(unnamed(r"(?#x\)(a)"), vec![6]);
    }

    #[test]
    fn escaped_parens_are_not_groups() {
        assert!(unnamed(r"\(literal\)").is_empty());
        assert!(unnamed("(*PRUNE)").is_empty());
        assert_eq!(unnamed("(a)(?(1)x|y)"), vec![0]);
    }

    /// PCRE reads `]` as a member when it opens the class body, so the class
    /// runs on past it.
    #[test]
    fn class_with_leading_bracket_member_is_not_a_capture() {
        assert!(unnamed("[]()]").is_empty());
        assert!(unnamed("[^]()]").is_empty());
        assert!(unnamed("[[:alpha:]()]").is_empty());
        // A `]` that is not first still closes the class.
        assert_eq!(unnamed("[abc](d)"), vec![5]);
    }

    /// `[[:^alpha:]]` negates the class — the `^` is syntax, not the name.
    #[test]
    fn negated_posix_class_does_not_end_the_class() {
        assert!(unnamed("[[:^print:]()]").is_empty());
        assert!(unnamed("[[:^alpha:]]").is_empty());
        // Still finds a real capture after the class.
        assert_eq!(unnamed("[[:^print:]](a)"), vec![12]);
    }

    /// `\Q...\E` quotes its contents, so parens inside are literal.
    #[test]
    fn quoted_literal_span_is_not_scanned() {
        assert!(unnamed(r"\Q(a)\E").is_empty());
        assert_eq!(unnamed(r"\Q(a)\E(b)"), vec![7]);
        // Unterminated `\Q` quotes to the end.
        assert!(unnamed(r"\Q(a)").is_empty());
    }

    /// The paren nested in a conditional is syntax, not a group.
    #[test]
    fn conditional_inner_paren_is_not_a_capture() {
        assert_eq!(unnamed("(a)(?(1)x|y)"), vec![0]);
        assert!(unnamed("(?(<n>)x|y)").is_empty());
    }

    /// A `(?#...)` comment body is opaque text ending at the first `)`.
    #[test]
    fn comment_body_is_skipped() {
        assert!(unnamed("^/a(?#()$").is_empty());
        assert_eq!(unnamed("^/a(?#()/(.*)$"), vec![9]);
        assert_eq!(unnamed("(?# [ )(a)"), vec![7]);
        assert_eq!(unnamed(r"(?#\Q)(a)"), vec![6]);
        assert_eq!(unnamed("(?#c)(a)"), vec![5]);
    }

    /// `(*MARK:name)` carries arbitrary text; a `[` in it must not swallow the
    /// rest of the pattern.
    #[test]
    fn verb_name_is_skipped() {
        assert_eq!(unnamed("(*MARK:[)(a)"), vec![9]);
        assert!(unnamed("(*PRUNE)").is_empty());
    }

    /// Truncated and degenerate inputs. PCRE rejects all of these, so nginx
    /// would refuse the config either way — what matters is only that `scan`
    /// does not panic and does not read past the end.
    ///
    /// `(?'` answering "named" is a deliberate loosening: the previous walker
    /// guarded on `i + 3 < len` and said "no", this one asks `bytes.get()` and
    /// says "yes". Callers use the answer to decline an autofix, so erring
    /// towards "named" on a regex that cannot compile costs nothing.
    #[test]
    fn truncated_input_is_handled_without_panicking() {
        assert!(!named("("));
        assert!(!named("(?"));
        assert!(!named("(?<"));
        assert!(!named("(?P"));
        assert!(!named("(?<="));
        assert!(!named("(?<!"));
        assert!(!named(""));
        assert!(named("(?'"));

        assert_eq!(unnamed("("), vec![0]);
        assert!(unnamed("").is_empty());
        assert!(unnamed(")").is_empty());
    }

    /// Byte offsets must land on the `(` itself even when multibyte characters
    /// precede it, or a caller slicing at them would split a char.
    #[test]
    fn offsets_are_char_boundaries_with_multibyte_input() {
        for (regex, expected) in [
            ("é(a)", vec![2]),
            (r"\é(a)", vec![3]),
            ("あ(.*)い", vec![3]),
        ] {
            let found = unnamed(regex);
            assert_eq!(found, expected, "{regex}");
            for pos in found {
                assert!(
                    regex.is_char_boundary(pos),
                    "{regex}: offset {pos} splits a char"
                );
                assert_eq!(
                    regex.as_bytes()[pos],
                    b'(',
                    "{regex}: offset {pos} is not a paren"
                );
            }
        }
    }

    /// `(*` spells two unrelated things, and treating them alike hid captures.
    /// A lowercase name plus `:` is a *group* whose body is regex —
    /// `(*pla:...)`, `(*atomic:...)`, `(*script_run:...)` — while verbs and
    /// option settings carry opaque text. All verified against libpcre2: the
    /// group forms really do hold the captures below.
    #[test]
    fn alpha_groups_are_scanned_but_verbs_stay_opaque() {
        // Groups: the body is regex, so its captures count.
        assert_eq!(unnamed("(*atomic:(a))b"), vec![9]);
        assert_eq!(unnamed("(*sr:(a))b"), vec![5]);
        assert_eq!(unnamed("(*nla:(a))b"), vec![6]);
        assert_eq!(unnamed("(*script_run:(a))b"), vec![13]);
        assert_eq!(unnamed("(*positive_lookahead:(a))b"), vec![21]);
        assert!(named("^/a(*pla:(?<n>x))(.*)$"));
        assert_eq!(unnamed("^/a(*pla:(y))(.*)$"), vec![9, 13]);

        // Verbs and option settings: opaque, and no capture of their own.
        assert_eq!(unnamed("(*MARK:n)(a)"), vec![9]);
        assert_eq!(unnamed("(*PRUNE)(a)"), vec![8]);
        assert_eq!(unnamed("(*:name)(a)"), vec![8]);
        assert_eq!(unnamed("(*UTF)(a)"), vec![6]);
        assert_eq!(unnamed("(*MARK:[)(a)"), vec![9]);
    }

    /// A callout's string argument is arbitrary text, exactly like a comment
    /// body — a `[` or `\Q` in one used to open a class or literal span that
    /// never closed, hiding every capture after it.
    #[test]
    fn callout_string_bodies_are_opaque() {
        assert_eq!(unnamed("(?C'[')(a)"), vec![7]);
        assert_eq!(unnamed(r"(?C'\Q')(a)"), vec![8]);
        // ...and their contents are not groups.
        assert!(unnamed("(?C'(a)')x").is_empty());
        // A numeric callout has no body.
        assert_eq!(unnamed("(?C1)(a)"), vec![5]);
        assert_eq!(unnamed("(?C)(a)"), vec![4]);
    }

    /// `(?<*...)` is a non-atomic lookbehind, not a name.
    #[test]
    fn non_atomic_lookbehind_is_not_named() {
        assert!(!named("(?<*ab)c"));
        assert!(!named("(?<=ab)c"));
        assert!(!named("(?<!ab)c"));
        assert!(named("(?<n>x)"));
    }
}
