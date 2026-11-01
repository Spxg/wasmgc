//! Rewrites linked Rust modules to use WasmGC types and native references.
//!
//! Processing follows the stages listed in [`process`]: locate placeholders,
//! decode their descriptors, generate replacement bodies and dependencies, and
//! write the module back. Original definitions keep their order; generated
//! definitions are appended with native interfaces and instance-local root tables.

use crate::schema::{FunctionKind, OperationKind, SchemaKind, ValueTag};
use std::collections::BTreeMap;

mod array_copy;
mod decode;
mod encode;
mod externref;
mod function;
mod generate;
mod normalize;
mod root;
mod types;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone)]
struct Value {
    tag: ValueTag,
    target: Option<u32>,
}

#[derive(Debug, Clone)]
struct Schema {
    kind: SchemaKind,
    fields: Vec<Value>,
}

#[derive(Debug, Clone, Copy)]
struct Operation {
    schema: u32,
    kind: OperationKind,
    field: u32,
}

#[derive(Debug, Clone)]
struct Struct {
    schema: u32,
    kind: OperationKind,
    field: u32,
    name: String,
}

#[derive(Debug, Clone)]
struct Bridge {
    function: u32,
    kind: FunctionKind,
    params: Vec<Value>,
    result: Option<Value>,
    module: String,
    name: String,
}

impl Bridge {
    fn is_scalar_export(&self) -> bool {
        self.kind == FunctionKind::Export
            && self
                .params
                .iter()
                .chain(&self.result)
                .all(|value| !value.tag.is_reference())
    }
}

#[derive(Default)]
struct Input {
    schemas: Vec<Schema>,
    schema_groups: Vec<std::ops::Range<usize>>,
    // Indexed by original function-import index; a name identifies a wasmgc
    // intrinsic, while None denotes a host import to retain.
    imports: Vec<(Option<String>, u32)>,
    types: u32,
    functions: u32,
    tables: u32,
    globals: u32,
    memory: Option<wasmparser::MemoryType>,
    // Keys remain original module function indices until code reencoding.
    operations: BTreeMap<u32, Operation>,
    struct_exports: Vec<Struct>,
    bridges: Vec<Bridge>,
}

/// Rewrites a linked Rust module to use native WasmGC operations and references.
///
/// Runtime shims are replaced with generated instructions. Temporary metadata
/// exports and marker imports are removed, and requested native functions are
/// generated. Unrelated imports, exports, and function bodies are retained with
/// their function indices adjusted as needed.
///
/// This function is available with the `cli` feature and is intended to run
/// in the host tool after linking, before the module is instantiated.
///
/// # Input
///
/// `bytes` must contain an unprocessed core Wasm module built with this version
/// of wasmgc. Descriptors must reside in active data segments of memory zero.
/// All active data segments of that memory must have statically known offsets.
/// Integer constants, immutable globals with known values, and integer addition,
/// subtraction, and multiplication are supported. Imported global values are
/// unavailable during post-processing.
/// Both 32-bit and 64-bit memory pointers are supported.
/// The resulting module requires an engine with WasmGC support.
///
/// # Errors
///
/// Returns an error if parsing fails, no wasmgc metadata or intrinsic is found,
/// descriptors cannot be decoded, an intrinsic is unsupported, or a generated
/// export name conflicts with another export. This function does not replace
/// a full Wasm validator.
///
/// # Examples
///
/// Enable the `cli` feature in the host tool's dependency on wasmgc:
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let input = std::fs::read("app.wasm")?;
/// let output = wasmgc::process(&input)?;
/// std::fs::write("app.gc.wasm", output)?;
/// # Ok(())
/// # }
/// ```
pub fn process(bytes: &[u8]) -> Result<Vec<u8>> {
    let module = decode::scan(bytes)?;
    let input = module.decode_descriptors()?;
    let changes = generate::generate(input)?;
    encode::rewrite(bytes, changes)
}
