//! Turns a Lua script into an nginx-lint plugin component.
//!
//! The heavy lifting was done once, when `runtimes/lua/runtime.core.wasm` was
//! built: it is the Lua interpreter plus the glue that implements the
//! `plugin` world by calling into a script. What is left to do per plugin
//! is to put the script where the runtime expects it and wrap the result
//! as a component. Neither step needs a compiler, so a plugin author needs
//! nothing but this tool.
//!
//! The runtime reserves a zero-filled buffer in its linear memory and
//! exports its address and capacity (as wasm globals, the linker's way of
//! exporting data symbols). [`inject_script`] appends a data segment that
//! initializes that buffer with the script, and [`componentize`] wraps the
//! module the same way `wasm-tools component new` would.

pub mod licenses;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
use wasm_encoder::{ConstExpr, DataCountSection, DataSection, Module, RawSection};
use wasmparser::{ExternalKind, KnownCustom, Operator, Parser, Payload};

/// The Lua runtime, built by `runtimes/lua/Makefile` and committed.
pub const RUNTIME: &[u8] = include_bytes!("../runtimes/lua/runtime.core.wasm");

const SLOT_EXPORT: &str = "nginx_lint_lua_script_slot";
const SLOT_MAX_EXPORT: &str = "nginx_lint_lua_script_slot_max";

/// Where the runtime wants the script: the buffer's address and capacity in
/// bytes, and the initial size of the memory it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub address: u32,
    pub capacity: u32,
    pub memory_size: u64,
}

/// The `sdk` entries of a module's producers section, name to version. The
/// runtime's Makefile records what it was built from there: the hash of its
/// inputs under `nginx-lint-lua-runtime-inputs`, the plugin API version
/// under `nginx-lint-plugin-api`, and the `lua` and `wasi-sdk` versions.
pub fn sdk_metadata(module: &[u8]) -> Result<BTreeMap<String, String>> {
    let mut sdks = BTreeMap::new();
    for payload in Parser::new(0).parse_all(module) {
        if let Payload::CustomSection(section) = payload?
            && let KnownCustom::Producers(reader) = section.as_known()
        {
            for field in reader {
                let field = field?;
                if field.name != "sdk" {
                    continue;
                }
                for value in field.values {
                    let value = value?;
                    sdks.insert(value.name.to_string(), value.version.to_string());
                }
            }
        }
    }
    Ok(sdks)
}

/// Reads the script slot's location out of a runtime module.
pub fn find_slot(runtime: &[u8]) -> Result<Slot> {
    let mut imported_globals = 0u32;
    let mut globals = Vec::new();
    let mut exports = Vec::new();
    let mut segments = Vec::new();
    let mut memory_size = None;

    for payload in Parser::new(0).parse_all(runtime) {
        match payload? {
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if let wasmparser::TypeRef::Global(_) = import?.ty {
                        imported_globals += 1;
                    }
                }
            }
            Payload::GlobalSection(reader) => {
                for global in reader {
                    globals.push(const_i32(&global?.init_expr));
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    if export.kind == ExternalKind::Global {
                        exports.push((export.name.to_string(), export.index));
                    }
                }
            }
            Payload::MemorySection(reader) => {
                if let Some(memory) = reader.into_iter().next() {
                    let memory = memory?;
                    ensure!(!memory.memory64, "the runtime uses a 64-bit memory");
                    memory_size = Some(memory.initial * 65536);
                }
            }
            Payload::DataSection(reader) => {
                for segment in reader {
                    let segment = segment?;
                    if let wasmparser::DataKind::Active {
                        memory_index: 0,
                        offset_expr,
                    } = &segment.kind
                        && let Some(offset) = const_i32(offset_expr)
                    {
                        segments.push((offset, segment.data.to_vec()));
                    }
                }
            }
            _ => {}
        }
    }

    let exported_address = |name: &str| -> Result<u32> {
        let (_, index) = exports
            .iter()
            .find(|(export, _)| export == name)
            .with_context(|| format!("the runtime does not export `{name}`"))?;
        let defined = index
            .checked_sub(imported_globals)
            .with_context(|| format!("`{name}` is an imported global"))?;
        globals
            .get(defined as usize)
            .copied()
            .flatten()
            .with_context(|| format!("`{name}` is not a constant i32 global"))
    };
    let address = exported_address(SLOT_EXPORT)?;
    let capacity_address = exported_address(SLOT_MAX_EXPORT)?;

    let capacity = segments
        .iter()
        .find_map(|(offset, data)| {
            let start = capacity_address.checked_sub(*offset)? as usize;
            let bytes = data.get(start..start.checked_add(4)?)?;
            Some(u32::from_le_bytes(bytes.try_into().unwrap()))
        })
        .with_context(|| format!("`{SLOT_MAX_EXPORT}` points outside the runtime's data"))?;
    let memory_size = memory_size.context("the runtime defines no memory")?;
    ensure!(
        u64::from(address) + u64::from(capacity) <= memory_size,
        "the script slot extends past the runtime's initial memory"
    );
    Ok(Slot {
        address,
        capacity,
        memory_size,
    })
}

fn const_i32(expr: &wasmparser::ConstExpr<'_>) -> Option<u32> {
    let mut ops = expr.get_operators_reader();
    match ops.read().ok()? {
        Operator::I32Const { value } => Some(value as u32),
        _ => None,
    }
}

/// The bytes the runtime reads from its slot: both lengths, the name (which
/// becomes the chunk name Lua reports errors under), then the script.
fn slot_payload(name: &str, script: &[u8]) -> Result<Vec<u8>> {
    let script_len = u32::try_from(script.len()).context("the script is larger than 4 GiB")?;
    let name_len = u32::try_from(name.len()).context("the script name is larger than 4 GiB")?;
    let mut payload = Vec::with_capacity(8 + name.len() + script.len());
    payload.extend_from_slice(&script_len.to_le_bytes());
    payload.extend_from_slice(&name_len.to_le_bytes());
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(script);
    Ok(payload)
}

/// Writes `script` into the runtime's slot, returning the new core module.
/// Every section but the data section (and its count) is copied through
/// byte for byte.
pub fn inject_script(runtime: &[u8], name: &str, script: &[u8]) -> Result<Vec<u8>> {
    ensure!(!script.is_empty(), "the script is empty");
    let slot = find_slot(runtime)?;
    let payload = slot_payload(name, script)?;
    ensure!(
        payload.len() <= slot.capacity as usize,
        "the script is too large: {} bytes with its name, and the runtime holds at most {} bytes",
        payload.len(),
        slot.capacity
    );

    let mut module = Module::new();
    for section in Parser::new(0).parse_all(runtime) {
        let section = section?;
        match &section {
            Payload::DataSection(reader) => {
                let mut data = DataSection::new();
                RoundtripReencoder.parse_data_section(&mut data, reader.clone())?;
                data.active(
                    0,
                    &ConstExpr::i32_const(slot.address as i32),
                    payload.iter().copied(),
                );
                module.section(&data);
            }
            Payload::DataCountSection { count, .. } => {
                module.section(&DataCountSection { count: count + 1 });
            }
            _ => {
                if let Some((id, range)) = section.as_section() {
                    let range = range.start as usize..range.end as usize;
                    module.section(&RawSection {
                        id,
                        data: &runtime[range],
                    });
                }
            }
        }
    }
    Ok(module.finish())
}

/// Wraps a core module that carries its component type (as the runtime
/// does) into a component, validating the result.
pub fn componentize(module: &[u8]) -> Result<Vec<u8>> {
    wit_component::ComponentEncoder::default()
        .module(module)?
        .validate(true)
        .encode()
        .context("failed to encode the component")
}

/// Builds a plugin component from a script. `name` is the file name the
/// script's errors are reported under.
pub fn build_plugin(name: &str, script: &[u8]) -> Result<Vec<u8>> {
    if name.is_empty() {
        bail!("the script name is empty");
    }
    let module = inject_script(RUNTIME, name, script)?;
    componentize(&module)
}

#[cfg(test)]
mod tests {
    use super::*;

    use sha2::{Digest, Sha256};
    use std::path::Path;

    /// The files the runtime is built from, in the order `runtimes/lua/Makefile`
    /// hashes them (STAMP_INPUTS); the toolchain is pinned in the Makefile,
    /// so a toolchain change is an input change too.
    const STAMP_INPUTS: &[&str] = &[
        "runtimes/lua/shim.c",
        "runtimes/lua/stubs.c",
        "runtimes/lua/nginx_lint.lua",
        "runtimes/lua/Makefile",
        "../../wit/nginx-lint-plugin.wit",
    ];

    fn expected_stamp() -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut hasher = Sha256::new();
        for input in STAMP_INPUTS {
            let bytes = std::fs::read(root.join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
            hasher.update(bytes);
        }
        format!("{:x}", hasher.finalize())
    }

    /// The committed runtime was built from the tree as it is now. When
    /// this fails, something under runtimes/lua/ or the WIT changed without
    /// `make -C plugins/nginx-lint-plugin-sdk/runtimes/lua` being rerun and the
    /// result committed.
    #[test]
    fn committed_runtime_is_built_from_the_current_inputs() {
        let sdks = sdk_metadata(RUNTIME).unwrap();
        assert_eq!(
            sdks.get("nginx-lint-lua-runtime-inputs")
                .map(String::as_str),
            Some(expected_stamp().as_str()),
            "runtime.core.wasm is stale: rebuild it with `make build-lua-runtime` and commit the result"
        );
    }

    /// The runtime declares the plugin API version of the SDK crate it was
    /// built alongside; a bump there means a rebuild here.
    #[test]
    fn committed_runtime_declares_the_current_api_version() {
        let sdks = sdk_metadata(RUNTIME).unwrap();
        assert_eq!(
            sdks.get("nginx-lint-plugin-api").map(String::as_str),
            Some(nginx_lint_plugin::API_VERSION),
        );
    }

    /// The sandbox claim: plugins built from this runtime load without
    /// --allow-wasi-plugins.
    #[test]
    fn committed_runtime_imports_nothing_from_wasi() {
        for payload in Parser::new(0).parse_all(RUNTIME) {
            if let Payload::ImportSection(reader) = payload.unwrap() {
                for import in reader.into_imports() {
                    let import = import.unwrap();
                    assert!(
                        !import.module.starts_with("wasi"),
                        "runtime imports {}::{}",
                        import.module,
                        import.name
                    );
                }
            }
        }
    }

    #[test]
    fn runtime_exposes_its_slot() {
        let slot = find_slot(RUNTIME).unwrap();
        assert_eq!(slot.capacity, 1 << 20);
        assert!(slot.address > 0);
    }

    /// The injected segment is where the runtime will look, and holds
    /// exactly the header and script.
    #[test]
    fn injects_the_script_as_a_data_segment_at_the_slot() {
        let script = b"return { spec = {}, check = function() return {} end }";
        let module = inject_script(RUNTIME, "rule.lua", script).unwrap();
        let slot = find_slot(&module).unwrap();

        let mut found = None;
        let mut count = 0;
        for payload in Parser::new(0).parse_all(&module) {
            match payload.unwrap() {
                Payload::DataSection(reader) => {
                    for segment in reader {
                        let segment = segment.unwrap();
                        count += 1;
                        if let wasmparser::DataKind::Active { offset_expr, .. } = &segment.kind
                            && const_i32(offset_expr) == Some(slot.address)
                        {
                            found = Some(segment.data.to_vec());
                        }
                    }
                }
                Payload::DataCountSection {
                    count: declared, ..
                } => {
                    assert_eq!(declared, count_segments(RUNTIME) + 1);
                }
                _ => {}
            }
        }
        assert_eq!(count, count_segments(RUNTIME) + 1);
        assert_eq!(found.unwrap(), slot_payload("rule.lua", script).unwrap());
    }

    fn count_segments(module: &[u8]) -> u32 {
        Parser::new(0)
            .parse_all(module)
            .filter_map(|payload| match payload.unwrap() {
                Payload::DataSection(reader) => Some(reader.count()),
                _ => None,
            })
            .sum()
    }

    #[test]
    fn injected_module_still_encodes_as_a_component() {
        let module = inject_script(RUNTIME, "rule.lua", b"return {}").unwrap();
        let component = componentize(&module).unwrap();
        assert!(wasmparser::Parser::is_component(&component));
    }

    #[test]
    fn refuses_a_script_that_does_not_fit() {
        let slot = find_slot(RUNTIME).unwrap();
        let script = vec![b' '; slot.capacity as usize];
        let err = inject_script(RUNTIME, "rule.lua", &script).unwrap_err();
        assert!(err.to_string().contains("too large"), "{err}");
    }

    #[test]
    fn refuses_an_empty_script() {
        assert!(inject_script(RUNTIME, "rule.lua", b"").is_err());
    }
}
