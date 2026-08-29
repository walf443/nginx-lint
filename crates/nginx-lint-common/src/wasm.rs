//! WASM component exposing the fix applier (the `fixer` world).
//!
//! The TypeScript SDK cannot call [`crate::apply_fixes_to_content_detailed`]
//! directly the way the Python SDK does, so it instantiates this component
//! instead. Keeping the applier here rather than in a component built from
//! `nginx-lint-parser` is what avoids a dependency cycle: this crate already
//! depends on the parser.

wit_bindgen::generate!({
    path: "../../wit/nginx-lint-plugin.wit",
    world: "fixer",
    pub_export_macro: true,
});

struct FixerComponent;

impl Guest for FixerComponent {
    fn apply_fixes(
        content: String,
        fixes: Vec<nginx_lint::plugin::types::Fix>,
    ) -> nginx_lint::plugin::fixer_types::FixResult {
        let fixes: Vec<crate::Fix> = fixes.iter().map(convert_fix).collect();
        let refs: Vec<&crate::Fix> = fixes.iter().collect();
        let result = crate::apply_fixes_to_content_detailed(&content, &refs);

        nginx_lint::plugin::fixer_types::FixResult {
            content: result.content,
            applied: result.applied as u32,
            skipped_invalid: result.skipped_invalid as u32,
        }
    }
}

export!(FixerComponent);

fn convert_fix(fix: &nginx_lint::plugin::types::Fix) -> crate::Fix {
    crate::Fix {
        line: fix.line as usize,
        old_text: fix.old_text.clone(),
        new_text: fix.new_text.clone(),
        delete_line: fix.delete_line,
        insert_after: fix.insert_after,
        start_offset: fix.start_offset.map(|o| o as usize),
        end_offset: fix.end_offset.map(|o| o as usize),
    }
}
