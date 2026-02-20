use wasm_bindgen::prelude::*;

/// Parse an nginx configuration string and return JSON AST.
///
/// Returns JSON string of the parsed Config AST on success,
/// or a JSON object `{"error": "message"}` on parse failure.
#[wasm_bindgen]
pub fn parse_string_to_json(source: &str) -> String {
    match crate::parse_string(source) {
        Ok(config) => serde_json::to_string(&config).unwrap_or_else(|e| {
            format!(r#"{{"error":"serialization error: {}"}}"#, e)
        }),
        Err(e) => {
            let msg = e.to_string().replace('\\', "\\\\").replace('"', "\\\"");
            format!(r#"{{"error":"{}"}}"#, msg)
        }
    }
}
