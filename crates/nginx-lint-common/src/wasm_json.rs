//! A core-module JSON entry point for the fix applier.
//!
//! The `fixer` component next door is only reachable from a component-model
//! runtime, which Go does not have. This exposes
//! [`crate::apply_fixes_to_content_detailed`] — the function the CLI applies
//! `--fix` with — to any plain wasm runtime instead: no imports, no canonical
//! ABI, JSON in and JSON out. The Go SDK's test helper runs it under wazero,
//! which is what lets `AssertFixProduces` check a rule's fixes against the
//! applier that will actually run them rather than a copy of it.

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ApplyRequest {
    content: String,
    fixes: Vec<crate::Fix>,
}

#[derive(Serialize)]
struct ApplyResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// How many fixes were applied, and how many were rejected outright. A
    /// fix dropped for overlapping one already applied is counted by neither,
    /// so a caller checking that every fix landed compares `applied` against
    /// how many it submitted.
    applied: u32,
    skipped_invalid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Reserve `len` bytes for the caller to write an argument into. The caller
/// does not free it; the module is instantiated per call and thrown away.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Apply fixes to content, both given as one JSON request, and return the
/// result as JSON.
///
/// Returns the pointer and length packed into one u64 — `(ptr << 32) | len` —
/// because a wasm export returns a single value.
#[unsafe(no_mangle)]
pub extern "C" fn apply_fixes_json(request_ptr: *const u8, request_len: usize) -> u64 {
    let json = match read(request_ptr, request_len) {
        Some(request) => apply(request),
        None => error("the request is not valid UTF-8".to_string()),
    };
    let bytes = json.into_bytes();
    let (ptr, len) = (bytes.as_ptr() as u64, bytes.len() as u64);
    std::mem::forget(bytes);
    (ptr << 32) | len
}

fn read<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        return Some("");
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(ptr, len) }).ok()
}

fn apply(request: &str) -> String {
    let request: ApplyRequest = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(e) => return error(format!("invalid request: {e}")),
    };

    let fixes: Vec<&crate::Fix> = request.fixes.iter().collect();
    let result = crate::apply_fixes_to_content_detailed(&request.content, &fixes);

    let response = ApplyResponse {
        content: Some(result.content),
        applied: result.applied as u32,
        skipped_invalid: result.skipped_invalid as u32,
        error: None,
    };
    serde_json::to_string(&response).unwrap_or_else(|e| error(e.to_string()))
}

fn error(message: String) -> String {
    let response = ApplyResponse {
        content: None,
        applied: 0,
        skipped_invalid: 0,
        error: Some(message),
    };
    // A serde_json failure on this struct cannot happen, but a panic here
    // would reach the caller as an unhelpful trap.
    serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"applied":0,"skipped_invalid":0,"error":"could not encode the error"}"#.to_string()
    })
}
