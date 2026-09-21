//! Applying [`Fix`]es to a file's content: line-based fixes are turned
//! into byte ranges, and the ranges applied back to front.

use super::Fix;

/// Compute the byte offset of the start of each line (1-indexed).
///
/// Returns a vector where `line_starts[0]` is always `0` (start of line 1),
/// `line_starts[1]` is the byte offset of line 2, etc.
/// An extra entry at the end equals `content.len()` for convenience.
pub fn compute_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts.push(content.len());
    starts
}

/// Convert a line-based [`Fix`] into an offset-based one using precomputed line starts.
///
/// Line-based fixes (created via deprecated `Fix::replace`, `Fix::delete`, etc.) are
/// normalized to `Fix::replace_range` using the provided `line_starts` offsets.
///
/// Returns `None` if the fix references an out-of-range line or the `old_text` is not found.
pub fn normalize_line_fix(fix: &Fix, content: &str, line_starts: &[usize]) -> Option<Fix> {
    if fix.line == 0 {
        return None;
    }

    let num_lines = line_starts.len() - 1; // last entry is content.len()

    if fix.delete_line {
        if fix.line > num_lines {
            return None;
        }
        let start = line_starts[fix.line - 1];
        let end = if fix.line < num_lines {
            line_starts[fix.line] // includes the trailing \n
        } else {
            // Last line: also remove the preceding \n if there is one
            let end = line_starts[fix.line]; // == content.len()
            if start > 0 && content.as_bytes().get(start - 1) == Some(&b'\n') {
                return Some(Fix::replace_range(start - 1, end, ""));
            }
            end
        };
        return Some(Fix::replace_range(start, end, ""));
    }

    if fix.insert_after {
        if fix.line > num_lines {
            return None;
        }
        // Insert point: right after the \n at end of the target line
        let insert_offset = if fix.line < num_lines {
            line_starts[fix.line]
        } else {
            content.len()
        };
        let new_text = if insert_offset == content.len() && !content.ends_with('\n') {
            format!("\n{}", fix.new_text)
        } else {
            format!("{}\n", fix.new_text)
        };
        return Some(Fix::replace_range(insert_offset, insert_offset, &new_text));
    }

    if fix.line > num_lines {
        return None;
    }

    let line_start = line_starts[fix.line - 1];
    let line_end_with_newline = line_starts[fix.line];
    // Line content without trailing newline
    let line_end = if line_end_with_newline > line_start
        && content.as_bytes().get(line_end_with_newline - 1) == Some(&b'\n')
    {
        line_end_with_newline - 1
    } else {
        line_end_with_newline
    };

    if let Some(ref old_text) = fix.old_text {
        // Replace first occurrence of old_text within the line
        let line_content = &content[line_start..line_end];
        if let Some(pos) = line_content.find(old_text.as_str()) {
            let start = line_start + pos;
            let end = start + old_text.len();
            return Some(Fix::replace_range(start, end, &fix.new_text));
        }
        return None;
    }

    // Replace entire line content (not including newline)
    Some(Fix::replace_range(line_start, line_end, &fix.new_text))
}

/// Result of applying fixes to content, with detailed counts.
#[derive(Debug, Clone)]
pub struct FixApplyResult {
    /// Content after applying the fixes
    pub content: String,
    /// Number of fixes applied
    pub applied: usize,
    /// Number of fixes skipped because they could not be applied: offsets out
    /// of range or not on UTF-8 character boundaries, or a line-based fix
    /// referencing a missing line or `old_text` (e.g. produced by a buggy
    /// plugin). Does not include fixes skipped due to overlap with an applied
    /// fix.
    pub skipped_invalid: usize,
}

/// Apply fixes to content string.
///
/// Convenience wrapper around [`apply_fixes_to_content_detailed`] for callers
/// that do not need the skipped-fix count.
///
/// Returns `(modified_content, number_of_fixes_applied)`.
pub fn apply_fixes_to_content(content: &str, fixes: &[&Fix]) -> (String, usize) {
    let result = apply_fixes_to_content_detailed(content, fixes);
    (result.content, result.applied)
}

/// Whether `s` is non-empty and consists entirely of whitespace — i.e. an
/// insert of `s` is pure reformatting (e.g. `indent`'s fixes), not content.
fn is_whitespace_only(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_whitespace)
}

/// Whether `s` contains at least one non-whitespace character — i.e. an
/// insert of `s` adds real content (e.g. a missing closing brace), not just
/// whitespace. The complement of [`is_whitespace_only`] over non-empty
/// strings; both are `false` for the empty string (a no-op insert).
fn has_non_whitespace(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
}

/// Apply fixes to content string, reporting skipped fixes.
///
/// All fixes (both line-based and offset-based) are normalized to offset-based,
/// then applied in reverse order to avoid index shifts. Overlapping fixes are skipped.
/// Fixes that cannot be applied (invalid offsets, or line-based fixes that fail
/// normalization) are skipped and counted in [`FixApplyResult::skipped_invalid`].
pub fn apply_fixes_to_content_detailed(content: &str, fixes: &[&Fix]) -> FixApplyResult {
    let line_starts = compute_line_starts(content);
    let mut skipped_invalid = 0;

    // Normalize all fixes to range-based
    let mut range_fixes: Vec<Fix> = Vec::with_capacity(fixes.len());
    for fix in fixes {
        if fix.is_range_based() {
            range_fixes.push((*fix).clone());
        } else if let Some(normalized) = normalize_line_fix(fix, content, &line_starts) {
            range_fixes.push(normalized);
        } else {
            skipped_invalid += 1;
        }
    }

    // Sort by start_offset descending to avoid index shifts.
    // For same-offset insertions (start == end), sort by indent ascending so that
    // the more-indented text is processed last and ends up first in the file.
    range_fixes.sort_by(|a, b| {
        let a_start = a.start_offset.unwrap();
        let b_start = b.start_offset.unwrap();
        match b_start.cmp(&a_start) {
            std::cmp::Ordering::Equal => {
                let a_is_insert = a.end_offset.unwrap() == a_start;
                let b_is_insert = b.end_offset.unwrap() == b_start;
                if a_is_insert && b_is_insert {
                    // For insertions at the same point: ascending indent order
                    // so more-indented text is processed last (appears first in output)
                    let a_indent = a.new_text.len() - a.new_text.trim_start().len();
                    let b_indent = b.new_text.len() - b.new_text.trim_start().len();
                    a_indent.cmp(&b_indent)
                } else {
                    std::cmp::Ordering::Equal
                }
            }
            other => other,
        }
    });

    // Offsets that have at least one structural (content-inserting) zero-width
    // insert, e.g. `unmatched-braces` inserting a missing `}`. Computed over
    // the whole fix set up front so the decision below doesn't depend on the
    // order fixes happen to be processed in — the ascending-indent sort tiebreak
    // means a structural insert with more leading whitespace than a competing
    // reformatting insert would otherwise be processed second and let both apply.
    let structural_insert_offsets: std::collections::HashSet<usize> = range_fixes
        .iter()
        .filter(|f| {
            f.start_offset.unwrap() == f.end_offset.unwrap() && has_non_whitespace(&f.new_text)
        })
        .map(|f| f.start_offset.unwrap())
        .collect();

    let mut fix_count = 0;
    let mut result = content.to_string();
    let mut applied_ranges: Vec<(usize, usize)> = Vec::new();

    for fix in &range_fixes {
        let start = fix.start_offset.unwrap();
        let end = fix.end_offset.unwrap();
        let is_insert = start == end;

        // Check if this range overlaps with any already applied range
        let overlaps = applied_ranges.iter().any(|(s, e)| start < *e && end > *s);

        // Two zero-width inserts at the identical point don't trip the
        // check above (touching, not overlapping) — which is intentional
        // when both are pure whitespace (e.g. two `indent` fixes for the
        // same line combine into the right total indentation). But
        // stacking a whitespace-only reformatting insert next to one that
        // inserts real content (e.g. `unmatched-braces` inserting a missing
        // `}`) produces nonsensical interleaved output — the whitespace fix
        // was computed against a structure this other fix is about to
        // change anyway, so drop it (regardless of which is processed first).
        let conflicts_with_structural_insert = is_insert
            && is_whitespace_only(&fix.new_text)
            && structural_insert_offsets.contains(&start);

        if overlaps || conflicts_with_structural_insert {
            continue;
        }

        // Offsets must lie on UTF-8 char boundaries: replace_range panics
        // otherwise, and plugin-provided fixes are untrusted input.
        if start <= end
            && end <= result.len()
            && result.is_char_boundary(start)
            && result.is_char_boundary(end)
        {
            result.replace_range(start..end, &fix.new_text);
            applied_ranges.push((start, start + fix.new_text.len()));
            fix_count += 1;
        } else {
            skipped_invalid += 1;
        }
    }

    // Ensure trailing newline
    if !result.ends_with('\n') {
        result.push('\n');
    }

    FixApplyResult {
        content: result,
        applied: fix_count,
        skipped_invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_line_starts() {
        let starts = compute_line_starts("abc\ndef\nghi");
        // line 1 starts at 0, line 2 at 4, line 3 at 8, sentinel at 11
        assert_eq!(starts, vec![0, 4, 8, 11]);
    }

    #[test]
    fn test_compute_line_starts_trailing_newline() {
        let starts = compute_line_starts("abc\n");
        // line 1 at 0, line 2 at 4 (empty), sentinel at 4
        assert_eq!(starts, vec![0, 4, 4]);
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_replace() {
        let content = "listen 80;\nserver_name example.com;\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::replace(1, "80", "8080");
        let normalized = normalize_line_fix(&fix, content, &line_starts).unwrap();
        assert!(normalized.is_range_based());
        assert_eq!(normalized.start_offset, Some(7));
        assert_eq!(normalized.end_offset, Some(9));
        assert_eq!(normalized.new_text, "8080");
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_delete() {
        let content = "line1\nline2\nline3\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::delete(2);
        let normalized = normalize_line_fix(&fix, content, &line_starts).unwrap();
        assert!(normalized.is_range_based());
        // Should delete "line2\n" (offset 6..12)
        assert_eq!(normalized.start_offset, Some(6));
        assert_eq!(normalized.end_offset, Some(12));
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_insert_after() {
        let content = "line1\nline2\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::insert_after(1, "inserted");
        let normalized = normalize_line_fix(&fix, content, &line_starts).unwrap();
        assert!(normalized.is_range_based());
        // Insert at offset 6 (start of line 2)
        assert_eq!(normalized.start_offset, Some(6));
        assert_eq!(normalized.end_offset, Some(6));
        assert_eq!(normalized.new_text, "inserted\n");
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_out_of_range() {
        let content = "line1\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::delete(99);
        assert!(normalize_line_fix(&fix, content, &line_starts).is_none());
    }

    #[test]
    #[allow(deprecated)]
    fn test_normalize_replace_not_found() {
        let content = "listen 80;\n";
        let line_starts = compute_line_starts(content);
        let fix = Fix::replace(1, "nonexistent", "new");
        assert!(normalize_line_fix(&fix, content, &line_starts).is_none());
    }

    #[test]
    fn test_apply_range_fix() {
        let content = "listen 80;\n";
        let fix = Fix::replace_range(7, 9, "8080");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "listen 8080;\n");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_apply_multiple_fixes_same_line() {
        // Two fixes on the same line should both apply
        let content = "proxy_set_header Host $host;\n";
        let fix1 = Fix::replace_range(17, 21, "X-Real-IP");
        let fix2 = Fix::replace_range(22, 27, "$remote_addr");
        let fixes: Vec<&Fix> = vec![&fix1, &fix2];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "proxy_set_header X-Real-IP $remote_addr;\n");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_apply_overlapping_fixes_skips() {
        let content = "abcdef\n";
        let fix1 = Fix::replace_range(0, 3, "XYZ"); // replace "abc"
        let fix2 = Fix::replace_range(2, 5, "QQQ"); // overlaps with fix1
        let fixes: Vec<&Fix> = vec![&fix1, &fix2];
        let (_, count) = apply_fixes_to_content(content, &fixes);
        // Only one fix should apply (the other is skipped due to overlap)
        assert_eq!(count, 1);
    }

    #[test]
    fn test_apply_fix_non_char_boundary_skipped() {
        // "あ" is 3 bytes (0..3); offsets 1 and 2 are not char boundaries.
        // Such fixes (e.g. from a malicious plugin) must be skipped, not panic.
        let content = "あいう;\n";
        let fix = Fix::replace_range(1, 2, "x");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, content);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_apply_fix_non_char_boundary_end_skipped() {
        // start is on a boundary but end is mid-character
        let content = "あいう;\n";
        let fix = Fix::replace_range(0, 4, "x");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, content);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_apply_fix_multibyte_on_boundary_applies() {
        // Offsets on char boundaries within multibyte content still work
        let content = "あいう;\n";
        let fix = Fix::replace_range(3, 6, "x");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "あxう;\n");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_detailed_counts_invalid_fixes() {
        // One valid fix, one non-boundary fix, one out-of-range fix
        let content = "あいう;\n";
        let valid = Fix::replace_range(3, 6, "x");
        let non_boundary = Fix::replace_range(1, 2, "y");
        let out_of_range = Fix::replace_range(100, 200, "z");
        let fixes: Vec<&Fix> = vec![&valid, &non_boundary, &out_of_range];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.content, "あxう;\n");
        assert_eq!(result.applied, 1);
        assert_eq!(result.skipped_invalid, 2);
    }

    #[test]
    #[allow(deprecated)]
    fn test_detailed_counts_failed_line_normalization() {
        let content = "listen 80;\n";
        let missing_old_text = Fix::replace(1, "nonexistent", "x");
        let out_of_range_line = Fix::replace(99, "listen", "x");
        let fixes: Vec<&Fix> = vec![&missing_old_text, &out_of_range_line];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.content, content);
        assert_eq!(result.applied, 0);
        assert_eq!(result.skipped_invalid, 2);
    }

    #[test]
    fn test_detailed_overlap_not_counted_as_invalid() {
        let content = "abcdef\n";
        let fix1 = Fix::replace_range(0, 3, "XYZ");
        let fix2 = Fix::replace_range(2, 5, "QQQ"); // overlaps with fix1
        let fixes: Vec<&Fix> = vec![&fix1, &fix2];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.applied, 1);
        assert_eq!(result.skipped_invalid, 0);
    }

    /// Two whitespace-only inserts at the exact same point (e.g. two
    /// `indent` errors reconciling to the same total indentation) must
    /// still stack in ascending-indent order — this is the legitimate use
    /// of same-point insertion the conflict check below must not break.
    #[test]
    fn test_same_point_whitespace_only_inserts_stack() {
        let content = "#note\n";
        let four_spaces = Fix::replace_range(0, 0, "    ");
        let two_spaces = Fix::replace_range(0, 0, "  ");
        let fixes: Vec<&Fix> = vec![&four_spaces, &two_spaces];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(result.content, "      #note\n");
        assert_eq!(
            result.applied, 2,
            "both whitespace-only inserts should apply"
        );
    }

    /// Regression test for https://github.com/walf443/nginx-lint/issues/296.
    ///
    /// A structural insert (e.g. `unmatched-braces` inserting a missing
    /// `}`) and a whitespace-only reformatting insert (e.g. `indent`
    /// reformatting the very line the brace is being inserted before) at
    /// the exact same point don't trip the ordinary range-overlap check
    /// (both are zero-width, touching but not overlapping) — without a
    /// dedicated conflict check they get concatenated in whatever order the
    /// sort happens to produce, yielding nonsensical interleaved output
    /// (e.g. `      }` — 6 spaces of indentation matching neither fix's own
    /// intent). The whitespace-only fix must be dropped instead, since it
    /// was computed against content this other fix is about to change
    /// immediately adjacent to it anyway.
    #[test]
    fn test_whitespace_only_insert_skipped_when_it_conflicts_with_structural_insert() {
        let content = "# Missing closing brace for http\n";
        let close_brace = Fix::replace_range(0, 0, "}\n");
        let reindent = Fix::replace_range(0, 0, "      ");
        let fixes: Vec<&Fix> = vec![&close_brace, &reindent];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(
            result.content, "}\n# Missing closing brace for http\n",
            "the whitespace-only fix must be dropped, not interleaved with the brace"
        );
        assert_eq!(result.applied, 1);
    }

    /// The whitespace-vs-structural conflict must be resolved independently
    /// of processing order. A structural insert's own `new_text` can carry
    /// MORE leading whitespace than the competing whitespace-only fix — e.g.
    /// `unmatched-braces` closing a deeply-nested block emits
    /// `"      }\n"` (its brace at the block's own indent), while `indent`
    /// proposes a smaller reindent for the same line (plausible on
    /// unclosed-brace input, cf. #300). The ascending-indent sort tiebreak
    /// then processes the whitespace-only fix FIRST, so a check that only
    /// looked at already-applied fixes would miss the conflict and let both
    /// apply. The structural fix must still win.
    #[test]
    fn test_whitespace_only_insert_skipped_even_when_structural_insert_is_more_indented() {
        let content = "# note\n";
        let close_brace = Fix::replace_range(0, 0, "      }\n"); // 6 leading spaces
        let reindent = Fix::replace_range(0, 0, "  "); // 2 leading spaces
        let fixes: Vec<&Fix> = vec![&close_brace, &reindent];
        let result = apply_fixes_to_content_detailed(content, &fixes);
        assert_eq!(
            result.content, "      }\n# note\n",
            "structural fix must win regardless of relative leading-whitespace ordering"
        );
        assert_eq!(result.applied, 1);
    }

    #[test]
    #[allow(deprecated)]
    fn test_apply_deprecated_fix_via_normalization() {
        let content = "listen 80;\nserver_name old;\n";
        let fix = Fix::replace(2, "old", "new");
        let fixes: Vec<&Fix> = vec![&fix];
        let (result, count) = apply_fixes_to_content(content, &fixes);
        assert_eq!(result, "listen 80;\nserver_name new;\n");
        assert_eq!(count, 1);
    }
}
