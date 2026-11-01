//! Reads linked descriptors without instantiating the module.
//!
//! Descriptor addresses come from temporary exported globals or constants in
//! functions that call a marker. Active, constant-offset data segments provide
//! their bytes in memory zero. Schema addresses are interned before their
//! fields are visited so recursive references can be resolved iteratively.
//!
//! All recorded function indices belong to the original module. Reordering
//! and intrinsic removal happen later in the encoder.

use std::collections::{BTreeMap, BTreeSet};

use crate::schema::{
    FUNCTION_BRIDGE_PREFIX, FUNCTION_DESC_PREFIX, FUNCTION_MAGIC, FunctionKind, Layout, MAGIC_LEN,
    OPERATION_MAGIC, OperationKind, SCHEMA_MAGIC, STRUCT_EXPORT_DESC_PREFIX, STRUCT_MAGIC,
    SchemaKind, ValueTag,
};
use wasmparser::{ConstExpr, DataKind, ExternalKind, Operator, Parser, Payload, TypeRef};

use super::{Bridge, Input, Operation, Result, Schema, Struct, Value};

struct Memory<'a> {
    layout: Layout,
    segments: Vec<(u64, &'a [u8])>,
}

#[derive(Default)]
struct FunctionExport {
    function: Option<u32>,
    descriptor: Option<u64>,
}

struct Decoder<'a> {
    memory: Memory<'a>,
    addresses: BTreeMap<u64, u32>,
    schemas: Vec<Schema>,
    pending: Vec<u64>,
    byte_len: usize,
}

enum Placeholder {
    Operation(Operation),
    Import(Bridge),
}

/// Section counts, descriptor locations, and marker callers from the linked module.
pub(super) struct ScannedModule<'a> {
    input: Input,
    segments: Vec<(u64, &'a [u8])>,
    bodies: BTreeMap<u32, BTreeSet<u64>>,
    struct_export_descriptors: Vec<u64>,
    function_exports: BTreeMap<String, FunctionExport>,
    byte_len: usize,
}

/// Collects linked addresses and module contents without decoding descriptors.
pub(super) fn scan(bytes: &[u8]) -> Result<ScannedModule<'_>> {
    let mut input = Input::default();
    let mut markers = BTreeSet::new();
    let mut segments = Vec::new();
    let mut bodies = BTreeMap::new();
    let mut global_values = BTreeMap::new();
    let mut struct_export_descriptors = Vec::new();
    let mut function_exports = BTreeMap::<String, FunctionExport>::new();
    let mut next_function = 0;

    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::TypeSection(reader) => {
                for group in reader {
                    input.types += group?.types().len() as u32;
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    let intrinsic = import.module == "wasmgc";

                    match import.ty {
                        TypeRef::Func(ty) | TypeRef::FuncExact(ty) => {
                            if intrinsic && import.name.starts_with("marker_") {
                                markers.insert(next_function);
                            }

                            input
                                .imports
                                .push((intrinsic.then(|| import.name.to_owned()), ty));
                            next_function += 1;
                        }
                        _ if intrinsic => {
                            return Err("wasmgc imports must be functions".into());
                        }
                        TypeRef::Table(_) => input.tables += 1,
                        TypeRef::Global(_) => input.globals += 1,
                        TypeRef::Memory(memory) => {
                            input.memory.get_or_insert(memory);
                        }
                        _ => {}
                    }
                }
            }
            Payload::FunctionSection(reader) => input.functions = reader.count(),
            Payload::TableSection(reader) => input.tables += reader.count(),
            Payload::GlobalSection(reader) => {
                for global in reader {
                    let global = global?;
                    if !global.ty.mutable
                        && let Some(value) = offset(global.init_expr, &global_values)?
                    {
                        global_values.insert(input.globals, value);
                    }
                    input.globals += 1;
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    if export.kind == ExternalKind::Global
                        && export.name.starts_with(STRUCT_EXPORT_DESC_PREFIX)
                    {
                        struct_export_descriptors.push(*global_values.get(&export.index).ok_or(
                            "wasmgc struct export descriptor requires a constant address",
                        )?);
                    } else if export.kind == ExternalKind::Func
                        && let Some(name) = export.name.strip_prefix(FUNCTION_BRIDGE_PREFIX)
                    {
                        function_exports
                            .entry(name.to_owned())
                            .or_default()
                            .function = Some(export.index);
                    } else if export.kind == ExternalKind::Global
                        && let Some(name) = export.name.strip_prefix(FUNCTION_DESC_PREFIX)
                    {
                        let address = *global_values
                            .get(&export.index)
                            .ok_or("wasmgc function descriptor requires a constant address")?;
                        function_exports
                            .entry(name.to_owned())
                            .or_default()
                            .descriptor = Some(address);
                    }
                }
            }
            Payload::MemorySection(reader) => {
                for memory in reader {
                    input.memory.get_or_insert(memory?);
                }
            }
            Payload::CodeSectionEntry(body) => {
                if markers.is_empty() {
                    next_function += 1;
                    continue;
                }

                let mut constants = BTreeSet::new();
                let mut calls_marker = false;
                let mut operators = body.get_operators_reader()?;

                while !operators.eof() {
                    match operators.read()? {
                        Operator::I32Const { value } => {
                            constants.insert(u64::from(value as u32));
                        }
                        Operator::I64Const { value } => {
                            constants.insert(value as u64);
                        }
                        Operator::Call { function_index }
                        | Operator::ReturnCall { function_index }
                            if markers.contains(&function_index) =>
                        {
                            calls_marker = true;
                        }
                        _ => {}
                    }
                }

                if calls_marker {
                    bodies.insert(next_function, constants);
                }

                next_function += 1;
            }
            Payload::DataSection(reader) => {
                for data in reader {
                    let data = data?;

                    if let DataKind::Active {
                        memory_index: 0,
                        offset_expr,
                    } = data.kind
                    {
                        let start = offset(offset_expr, &global_values)?.ok_or(
                            "wasmgc requires a statically known offset for active data in memory zero",
                        )?;
                        segments.push((start, data.data));
                    }
                }
            }
            _ => {}
        }
    }

    if input.imports.iter().all(|(name, _)| name.is_none())
        && struct_export_descriptors.is_empty()
        && function_exports.is_empty()
    {
        return Err(
            "no wasmgc imports or exports found; expected an unprocessed Rust module using wasmgc"
                .into(),
        );
    }

    Ok(ScannedModule {
        input,
        segments,
        bodies,
        struct_export_descriptors,
        function_exports,
        byte_len: bytes.len(),
    })
}

impl ScannedModule<'_> {
    /// Decodes the located descriptors and every schema reachable from them.
    pub(super) fn decode_descriptors(self) -> Result<Input> {
        let Self {
            mut input,
            segments,
            bodies,
            struct_export_descriptors,
            function_exports,
            byte_len,
        } = self;

        let layout = if input.memory.is_some_and(|memory| memory.memory64) {
            Layout::WASM64
        } else {
            Layout::WASM32
        };

        let mut decoder = Decoder {
            memory: Memory { layout, segments },
            addresses: BTreeMap::new(),
            schemas: Vec::new(),
            pending: Vec::new(),
            byte_len,
        };

        // Pair by export name: the linker may share a numeric bridge between
        // exports whose native reference signatures differ.
        for export in function_exports.into_values() {
            let function = export
                .function
                .ok_or("wasmgc function descriptor has no bridge")?;
            let address = export
                .descriptor
                .ok_or("wasmgc function bridge has no descriptor")?;
            let bridge = decoder.bridge(address, function)?;
            input.bridges.push(bridge);
        }

        // A marker identifies the function to replace. Constants only nominate
        // descriptor addresses; their magic bytes identify valid descriptors.
        // Keeping this association avoids decoding the function-pointer table slot.
        for (function, constants) in bodies {
            match decoder.placeholder(function, constants)? {
                Placeholder::Operation(operation) => {
                    input.operations.insert(function, operation);
                }
                Placeholder::Import(bridge) => input.bridges.push(bridge),
            }
        }

        for address in struct_export_descriptors {
            input.struct_exports.push(decoder.struct_export(address)?);
        }

        input.schemas = decoder.finish()?;
        Ok(input)
    }
}

impl Decoder<'_> {
    fn schema(&mut self, address: u64) -> Result<u32> {
        if let Some(index) = self.addresses.get(&address) {
            return Ok(*index);
        }

        let layout = self.memory.layout;
        if address == 0
            || self
                .memory
                .read_at::<MAGIC_LEN>(address, layout.schema_magic_offset)?
                != SCHEMA_MAGIC
        {
            return Err("invalid wasmgc schema descriptor".into());
        }

        let index = u32::try_from(self.schemas.len()).map_err(|_| "too many wasmgc schemas")?;
        let kind = SchemaKind::from_raw(self.memory.word(address, layout.schema_kind_offset)?)
            .ok_or("invalid wasmgc schema kind")?;

        // Reserve the index before visiting fields so a self-reference or cycle
        // resolves to the same declaration rather than enqueueing it again.
        self.addresses.insert(address, index);
        self.schemas.push(Schema {
            kind,
            fields: Vec::new(),
        });
        self.pending.push(address);

        Ok(index)
    }

    fn value(&mut self, address: u64) -> Result<Value> {
        let layout = self.memory.layout;
        let tag = ValueTag::from_raw(self.memory.word(address, layout.value_kind_tag_offset)?)
            .ok_or("invalid wasmgc value tag")?;
        let target = if matches!(
            tag,
            ValueTag::Ref | ValueTag::NullableRef | ValueTag::Array | ValueTag::NullableArray
        ) {
            Some(
                self.schema(
                    self.memory
                        .pointer(address, layout.value_kind_target_offset)?,
                )?,
            )
        } else {
            None
        };

        Ok(Value { tag, target })
    }

    fn values(&mut self, address: u64, count: u32) -> Result<Vec<Value>> {
        let size = self.memory.layout.value_kind_size;
        if count as usize > self.byte_len / size as usize {
            return Err("invalid wasmgc value count".into());
        }

        let mut values = Vec::with_capacity(count as usize);
        for index in 0..count {
            let address = address
                .checked_add(u64::from(index) * u64::from(size))
                .ok_or("wasmgc values address overflow")?;
            values.push(self.value(address)?);
        }

        Ok(values)
    }

    fn bridge(&mut self, address: u64, function: u32) -> Result<Bridge> {
        let layout = self.memory.layout;
        if self
            .memory
            .read_at::<MAGIC_LEN>(address, layout.function_magic_offset)?
            != FUNCTION_MAGIC
        {
            return Err("invalid wasmgc function descriptor".into());
        }

        let kind = FunctionKind::from_raw(self.memory.word(address, layout.function_kind_offset)?)
            .ok_or("invalid wasmgc function kind")?;
        let count = self
            .memory
            .word(address, layout.function_param_count_offset)?;
        let start = self
            .memory
            .pointer(address, layout.function_params_offset)?;
        let params = self.values(start, count)?;
        let result_address = self
            .memory
            .pointer(address, layout.function_result_offset)?;
        let result = if result_address == 0 {
            None
        } else {
            Some(self.value(result_address)?)
        };

        Ok(Bridge {
            function,
            kind,
            params,
            result,
            module: self.memory.string(
                address,
                layout.function_module_offset,
                layout.function_module_len_offset,
            )?,
            name: self.memory.string(
                address,
                layout.function_name_offset,
                layout.function_name_len_offset,
            )?,
        })
    }

    fn struct_export(&mut self, address: u64) -> Result<Struct> {
        let layout = self.memory.layout;
        if self
            .memory
            .read_at::<MAGIC_LEN>(address, layout.struct_export_magic_offset)?
            != STRUCT_MAGIC
        {
            return Err("invalid wasmgc struct export descriptor".into());
        }

        let kind = OperationKind::from_raw(
            self.memory
                .word(address, layout.struct_export_kind_offset)?,
        )
        .filter(|kind| {
            matches!(
                kind,
                OperationKind::StructNew | OperationKind::StructGet | OperationKind::StructSet
            )
        })
        .ok_or("invalid wasmgc struct export kind")?;
        let schema = self.schema(
            self.memory
                .pointer(address, layout.struct_export_schema_offset)?,
        )?;

        Ok(Struct {
            schema,
            kind,
            field: self
                .memory
                .word(address, layout.struct_export_field_offset)?,
            name: self.memory.string(
                address,
                layout.struct_export_name_offset,
                layout.struct_export_name_len_offset,
            )?,
        })
    }

    fn placeholder(&mut self, function: u32, constants: BTreeSet<u64>) -> Result<Placeholder> {
        let layout = self.memory.layout;
        let mut decoded = None;
        for address in constants {
            let Ok(magic) = self.memory.read::<MAGIC_LEN>(address) else {
                continue;
            };
            let placeholder = match magic {
                FUNCTION_MAGIC => {
                    let bridge = self.bridge(address, function)?;
                    if bridge.kind != FunctionKind::Import {
                        return Err("wasmgc marker function requires an imported function".into());
                    }
                    Placeholder::Import(bridge)
                }
                OPERATION_MAGIC => {
                    let kind = OperationKind::from_raw(
                        self.memory.word(address, layout.operation_kind_offset)?,
                    )
                    .ok_or("invalid wasmgc operation kind")?;
                    Placeholder::Operation(Operation {
                        schema: self.schema(
                            self.memory
                                .pointer(address, layout.operation_schema_offset)?,
                        )?,
                        kind,
                        field: self.memory.word(address, layout.operation_field_offset)?,
                    })
                }
                _ => continue,
            };

            if decoded.replace(placeholder).is_some() {
                return Err("multiple wasmgc descriptors identify one function".into());
            }
        }

        decoded.ok_or_else(|| "wasmgc marker call has no valid operation descriptor".into())
    }

    fn finish(mut self) -> Result<Vec<Schema>> {
        let layout = self.memory.layout;

        // Reading a field may discover another schema. Drain this worklist after
        // all roots have been registered, without recursively walking the graph.
        while let Some(address) = self.pending.pop() {
            let count = self
                .memory
                .word(address, layout.schema_field_count_offset)?;
            let fields_address = self.memory.pointer(address, layout.schema_fields_offset)?;
            let fields = self.values(fields_address, count)?;
            let schema = &mut self.schemas[self.addresses[&address] as usize];
            if schema.kind == SchemaKind::Array && fields.len() != 1 {
                return Err("wasmgc array schema requires one element type".into());
            }
            schema.fields = fields;
        }

        Ok(self.schemas)
    }
}

impl Memory<'_> {
    fn read<const N: usize>(&self, address: u64) -> Result<[u8; N]> {
        let mut result = [0; N];

        for (offset, byte) in result.iter_mut().enumerate() {
            let address = address
                .checked_add(offset as u64)
                .ok_or("wasmgc descriptor address overflow")?;

            // Later active segments overwrite earlier ones, including partial overlaps.
            // Resolve each byte independently rather than assuming one containing segment.
            *byte = self
                .segments
                .iter()
                .rev()
                .find_map(|(start, bytes)| {
                    let offset = usize::try_from(address.checked_sub(*start)?).ok()?;

                    bytes.get(offset).copied()
                })
                .ok_or("wasmgc descriptor points outside initialized data")?;
        }

        Ok(result)
    }

    fn read_at<const N: usize>(&self, address: u64, offset: u32) -> Result<[u8; N]> {
        self.read(
            address
                .checked_add(u64::from(offset))
                .ok_or("wasmgc descriptor address overflow")?,
        )
    }

    fn word(&self, address: u64, offset: u32) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_at(address, offset)?))
    }

    fn string(&self, address: u64, offset: u32, length_offset: u32) -> Result<String> {
        let start = self.pointer(address, offset)?;
        let length = self.word(address, length_offset)?;
        let mut bytes = Vec::new();
        for offset in 0..length {
            bytes.push(self.read_at::<1>(start, offset)?[0]);
        }
        Ok(String::from_utf8(bytes)?)
    }

    fn pointer(&self, address: u64, offset: u32) -> Result<u64> {
        if self.layout.pointer_size == 8 {
            Ok(u64::from_le_bytes(self.read_at(address, offset)?))
        } else {
            Ok(u64::from(self.word(address, offset)?))
        }
    }
}

// Resolve integer constants and immutable globals without runtime imports.
// Preserve address bits and wrap arithmetic at each instruction's integer width.
fn offset(expr: ConstExpr<'_>, globals: &BTreeMap<u32, u64>) -> Result<Option<u64>> {
    let mut operators = expr.get_operators_reader();
    let mut stack = Vec::new();

    while !operators.eof() {
        match operators.read()? {
            Operator::I32Const { value } => stack.push(u64::from(value as u32)),
            Operator::I64Const { value } => stack.push(value as u64),
            Operator::GlobalGet { global_index } => {
                let Some(&value) = globals.get(&global_index) else {
                    return Ok(None);
                };
                stack.push(value);
            }
            operation @ (Operator::I32Add
            | Operator::I32Sub
            | Operator::I32Mul
            | Operator::I64Add
            | Operator::I64Sub
            | Operator::I64Mul) => {
                let right = stack.pop().ok_or("invalid integer constant expression")?;
                let left = stack.pop().ok_or("invalid integer constant expression")?;
                let value = match operation {
                    Operator::I32Add => u64::from((left as u32).wrapping_add(right as u32)),
                    Operator::I32Sub => u64::from((left as u32).wrapping_sub(right as u32)),
                    Operator::I32Mul => u64::from((left as u32).wrapping_mul(right as u32)),
                    Operator::I64Add => left.wrapping_add(right),
                    Operator::I64Sub => left.wrapping_sub(right),
                    Operator::I64Mul => left.wrapping_mul(right),
                    _ => unreachable!(),
                };
                stack.push(value);
            }
            Operator::End => {
                return Ok(if operators.eof() && stack.len() == 1 {
                    stack.pop()
                } else {
                    None
                });
            }
            _ => return Ok(None),
        }
    }

    Ok(None)
}
