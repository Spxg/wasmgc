use super::{
    DataReader, FunctionExport, FunctionImport, Operation, Result, Schema, Struct, Value, root,
};
use crate::schema::{
    FUNCTION_BRIDGE_PREFIX, FUNCTION_DESC_PREFIX, FUNCTION_MAGIC, Layout, MAGIC_LEN,
    OPERATION_MAGIC, OperationKind, STRUCT_EXPORT_DESC_PREFIX, STRUCT_MAGIC, SchemaKind, ValueTag,
};
use core::ops::Range;
use std::collections::{HashMap, HashSet};
use wasmparser::{Operator, Parser, Payload};

pub struct Resolver<'a> {
    pub memory: DataReader<'a>,
    pub original_types: u32,
    pub original_globals: u32,
    pub original_global_imports: u32,
    pub original_tables: u32,
    pub original_function_imports: u32,
    pub new_function_start: u32,
    pub new_function_indices: Vec<Option<u32>>,
    pub new_global_start: u32,
    pub new_global_indices: Vec<Option<u32>>,
    pub new_data_count: u32,
    pub new_data_indices: Vec<u32>,
    pub new_data_parts: Vec<Vec<Range<usize>>>,
    pub type_groups: Vec<Vec<u64>>,
    pub type_indices: HashMap<u64, u32>,
    pub gc_table: Option<root::GcTable>,
    pub externref_table: Option<root::GcTable>,
    pub schema_set: HashMap<u64, Schema>,
    pub pending_resolve: Vec<u64>,
    pub discarded: Vec<Range<u64>>,
    pub operations: HashMap<u32, Operation>,
    pub intrinsics: Vec<(u32, u32, &'a str)>,
    pub struct_exports: Vec<Struct>,
    pub function_exports: Vec<FunctionExport>,
    pub function_imports: Vec<FunctionImport>,
}

impl<'a> Resolver<'a> {
    fn new(memory: DataReader<'a>) -> Self {
        Self {
            memory,
            original_types: 0,
            original_globals: 0,
            original_global_imports: 0,
            original_tables: 0,
            original_function_imports: 0,
            new_function_start: 0,
            new_function_indices: Vec::new(),
            new_global_start: 0,
            new_global_indices: Vec::new(),
            new_data_count: 0,
            new_data_indices: Vec::new(),
            new_data_parts: Vec::new(),
            type_groups: Vec::new(),
            type_indices: HashMap::new(),
            gc_table: None,
            externref_table: None,
            schema_set: HashMap::new(),
            pending_resolve: Vec::new(),
            discarded: Vec::new(),
            operations: HashMap::new(),
            intrinsics: Vec::new(),
            struct_exports: Vec::new(),
            function_exports: Vec::new(),
            function_imports: Vec::new(),
        }
    }

    pub fn parse(module: &'a [u8]) -> Result<Self> {
        let mut next_func_idx = 0;
        let mut removed_function_imports = 0;
        let mut original_function_imports = 0;
        let mut new_function_indices = Vec::new();
        let mut markers = HashSet::new();
        let mut marker_functions = HashMap::new();
        let mut intrinsics = Vec::new();
        let mut segments = Vec::new();
        let mut memory_type = None;
        let mut original_types = 0;
        let mut original_globals = 0;
        let mut original_global_imports = 0;
        let mut original_tables = 0;
        let mut global_addresses = HashMap::new();
        let mut struct_descriptors = Vec::new();
        let mut function_descriptors = Vec::new();
        let mut function_bridges = HashMap::new();
        let mut export_names = HashSet::new();

        for payload in Parser::new(0).parse_all(module) {
            match payload? {
                Payload::TypeSection(section_limited) => {
                    for section in section_limited {
                        original_types += section?.types().len() as u32;
                    }
                }
                Payload::ImportSection(section_limited) => {
                    for section in section_limited {
                        for import in section? {
                            let (_, import) = import?;
                            match import.ty {
                                wasmparser::TypeRef::Func(ty)
                                | wasmparser::TypeRef::FuncExact(ty) => {
                                    if import.module == "wasmgc" {
                                        removed_function_imports += 1;
                                        new_function_indices.push(None);
                                        if import.name.starts_with("marker") {
                                            assert!(matches!(
                                                import.ty,
                                                wasmparser::TypeRef::Func(_)
                                            ));
                                            markers.insert(next_func_idx);
                                        } else if matches!(
                                            import.name,
                                            "ref_clone"
                                                | "ref_release"
                                                | "ref_eq"
                                                | "externref_null"
                                                | "externref_is_null"
                                                | "externref_clone"
                                                | "externref_release"
                                        ) {
                                            intrinsics.push((next_func_idx, ty, import.name));
                                        }
                                    } else {
                                        new_function_indices
                                            .push(Some(next_func_idx - removed_function_imports));
                                    }
                                    original_function_imports += 1;
                                    next_func_idx += 1;
                                }
                                wasmparser::TypeRef::Memory(t) => {
                                    memory_type.get_or_insert(t);
                                }
                                wasmparser::TypeRef::Global(_) => {
                                    original_globals += 1;
                                    original_global_imports += 1;
                                }
                                wasmparser::TypeRef::Table(_) => original_tables += 1,
                                _ => (),
                            }
                        }
                    }
                }
                Payload::TableSection(section_limited) => {
                    original_tables += section_limited.count()
                }
                Payload::MemorySection(section_limited) => {
                    for section in section_limited {
                        memory_type.get_or_insert(section?);
                    }
                }
                Payload::GlobalSection(section_limited) => {
                    for global in section_limited {
                        let global = global?;
                        let mut operators = global.init_expr.get_operators_reader();
                        let address = match operators.read()? {
                            Operator::I32Const { value } => Some(u64::from(value as u32)),
                            Operator::I64Const { value } => Some(value as u64),
                            _ => None,
                        };
                        if let Some(address) = address
                            && matches!(operators.read()?, Operator::End)
                        {
                            global_addresses.insert(original_globals, address);
                        }
                        original_globals += 1;
                    }
                }
                Payload::ExportSection(section_limited) => {
                    for export in section_limited {
                        let export = export?;
                        if export.kind == wasmparser::ExternalKind::Global
                            && export.name.starts_with(STRUCT_EXPORT_DESC_PREFIX)
                        {
                            let address = *global_addresses.get(&export.index).ok_or(
                                "wasmgc struct descriptor requires a constant global address",
                            )?;
                            struct_descriptors.push((export.index, address));
                        } else if export.kind == wasmparser::ExternalKind::Global
                            && let Some(name) = export.name.strip_prefix(FUNCTION_DESC_PREFIX)
                        {
                            let address = *global_addresses.get(&export.index).ok_or(
                                "wasmgc function descriptor requires a constant global address",
                            )?;
                            function_descriptors.push((export.index, name, address));
                        } else if export.kind == wasmparser::ExternalKind::Func
                            && let Some(name) = export.name.strip_prefix(FUNCTION_BRIDGE_PREFIX)
                        {
                            function_bridges.insert(name, export.index);
                        } else {
                            export_names.insert(export.name.to_owned());
                        }
                    }
                }
                Payload::DataSection(section_limited) => {
                    for section in section_limited {
                        let data = section?;
                        let segment = if let wasmparser::DataKind::Active {
                            memory_index: 0,
                            offset_expr,
                        } = data.kind
                        {
                            let mut operator = offset_expr.get_operators_reader();
                            let offset = match operator.read()? {
                                Operator::I32Const { value } => u64::from(value as u32),
                                Operator::I64Const { value } => value as u64,
                                _ => return Err("wasmgc requires a statically known offset for active data in memory zero".into()),
                            };
                            if !matches!(operator.read()?, Operator::End) {
                                return Err(
                                    "expected end after data segment offset constant".into()
                                );
                            }
                            Some((offset, data.data))
                        } else {
                            None
                        };
                        segments.push(segment);
                    }
                }
                Payload::CodeSectionEntry(function_body) => {
                    new_function_indices.push(Some(next_func_idx - removed_function_imports));

                    // No marker
                    if markers.is_empty() {
                        next_func_idx += 1;
                        continue;
                    }

                    let mut may_addresses = vec![];
                    let mut call_marker = false;
                    let mut operators = function_body.get_operators_reader()?;

                    while !operators.eof() {
                        match operators.read()? {
                            Operator::Call { function_index }
                            | Operator::ReturnCall { function_index } => {
                                if markers.contains(&function_index) {
                                    call_marker = true;
                                }
                            }
                            Operator::I32Const { value } => {
                                may_addresses.push(u64::from(value as u32));
                            }
                            Operator::I64Const { value } => {
                                may_addresses.push(value as u64);
                            }
                            _ => (),
                        }
                    }

                    if call_marker {
                        marker_functions.insert(next_func_idx, may_addresses);
                    }

                    next_func_idx += 1;
                }
                _ => continue,
            }
        }

        let memory = DataReader {
            layout: if memory_type.is_some_and(|memory| memory.memory64) {
                Layout::WASM64
            } else {
                Layout::WASM32
            },
            segments,
        };

        let mut resolver = Self::new(memory);
        resolver.original_types = original_types;
        resolver.original_globals = original_globals;
        resolver.original_global_imports = original_global_imports;
        resolver.original_tables = original_tables;
        resolver.original_function_imports = original_function_imports;

        resolver.new_function_start = next_func_idx - removed_function_imports;
        resolver.new_function_indices = new_function_indices;
        resolver.intrinsics = intrinsics;
        resolver.resolve(
            marker_functions,
            struct_descriptors,
            function_descriptors,
            function_bridges,
            export_names,
        )?;

        Ok(resolver)
    }

    fn discard(&mut self, address: u64, size: u64) -> Result<()> {
        let size = size.next_multiple_of(u64::from(self.memory.layout.pointer_size));
        let end = address
            .checked_add(size)
            .ok_or("wasmgc descriptor address overflow")?;
        self.discarded.push(address..end);
        Ok(())
    }

    fn schema(&mut self, address: u64) -> Result<()> {
        if self.schema_set.contains_key(&address) {
            return Ok(());
        }

        let layout = self.memory.layout;
        let kind = SchemaKind::from_raw(self.memory.read_u32(address, layout.schema_kind_offset)?)
            .ok_or("invalid wasmgc schema kind")?;

        self.schema_set.insert(
            address,
            Schema {
                kind,
                fields: Vec::new(),
            },
        );

        self.pending_resolve.push(address);

        Ok(())
    }

    fn placeholder(&mut self, function: u32, may_addresses: Vec<u64>) -> Result<()> {
        let layout = self.memory.layout;
        for address in may_addresses {
            let Ok(magic) = self.memory.read::<MAGIC_LEN>(address) else {
                continue;
            };
            match magic {
                OPERATION_MAGIC => {
                    self.discard(address, u64::from(layout.operation_size))?;

                    let kind = OperationKind::from_raw(
                        self.memory
                            .read_u32(address, layout.operation_kind_offset)?,
                    )
                    .ok_or("invalid wasmgc operation kind")?;
                    let schema_addr = self
                        .memory
                        .pointer(address, layout.operation_schema_offset)?;
                    self.schema(schema_addr)?;

                    let operation = Operation {
                        schema_addr,
                        kind,
                        field_index: self
                            .memory
                            .read_u32(address, layout.operation_field_index_offset)?,
                    };
                    self.operations.insert(function, operation);
                    break;
                }
                FUNCTION_MAGIC => {
                    let (params, result, module, name) = self.function_signature(address)?;
                    self.function_imports.push(FunctionImport {
                        original_function_index: function,
                        function_index: 0,
                        type_index: 0,
                        params,
                        result,
                        module,
                        name,
                    });
                    break;
                }
                _ => continue,
            };
        }

        Ok(())
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
                .read_u32(address, layout.struct_export_kind_offset)?,
        )
        .filter(|kind| {
            matches!(
                kind,
                OperationKind::StructNew | OperationKind::StructGet | OperationKind::StructSet
            )
        })
        .ok_or("invalid wasmgc struct export kind")?;
        let schema_addr = self
            .memory
            .pointer(address, layout.struct_export_schema_offset)?;
        self.schema(schema_addr)?;

        let name_len = self
            .memory
            .read_u32(address, layout.struct_export_name_len_offset)?;
        let name_address = self
            .memory
            .address(address, u64::from(layout.struct_export_name_offset))?;
        self.discard(
            address,
            u64::from(layout.struct_export_name_offset) + u64::from(name_len),
        )?;

        Ok(Struct {
            schema_addr,
            kind,
            field_index: self
                .memory
                .read_u32(address, layout.struct_export_field_offset)?,
            name: self.memory.string(name_address, name_len)?,
            function_index: 0,
        })
    }

    fn function_signature(
        &mut self,
        address: u64,
    ) -> Result<(Vec<Value>, Option<Value>, String, String)> {
        let layout = self.memory.layout;
        if self
            .memory
            .read_at::<MAGIC_LEN>(address, layout.function_magic_offset)?
            != FUNCTION_MAGIC
        {
            return Err("invalid wasmgc function descriptor".into());
        }

        let param_count = self
            .memory
            .read_u32(address, layout.function_param_count_offset)?;
        let result_count = self
            .memory
            .read_u32(address, layout.function_result_count_offset)?;
        let module_len = self
            .memory
            .read_u32(address, layout.function_module_len_offset)?;
        let name_len = self
            .memory
            .read_u32(address, layout.function_name_len_offset)?;
        let params_address = self
            .memory
            .address(address, u64::from(layout.function_params_offset))?;
        let params = self.values(params_address, param_count)?;
        let result_offset = u64::from(layout.function_params_offset)
            + u64::from(param_count) * u64::from(layout.value_kind_size);
        let result = match result_count {
            0 => None,
            1 => Some(self.value(self.memory.address(address, result_offset)?)?),
            _ => return Err("invalid wasmgc function result count".into()),
        };
        let module_offset =
            result_offset + u64::from(result_count) * u64::from(layout.value_kind_size);
        let module_address = self.memory.address(address, module_offset)?;
        let module = self.memory.string(module_address, module_len)?;
        let name_offset = module_offset + u64::from(module_len);
        let name_address = self.memory.address(address, name_offset)?;
        let name = self.memory.string(name_address, name_len)?;
        self.discard(address, name_offset + u64::from(name_len))?;

        Ok((params, result, module, name))
    }

    fn function_export(&mut self, address: u64, function: u32) -> Result<FunctionExport> {
        let (params, result, _, name) = self.function_signature(address)?;
        Ok(FunctionExport {
            original_function_index: function,
            function_index: 0,
            params,
            result,
            name,
        })
    }

    fn value(&mut self, address: u64) -> Result<Value> {
        let layout = self.memory.layout;
        let tag = ValueTag::from_raw(
            self.memory
                .read_u32(address, layout.value_kind_tag_offset)?,
        )
        .ok_or("invalid wasmgc value tag")?;

        let target_addr = if matches!(
            tag,
            ValueTag::Ref | ValueTag::NullableRef | ValueTag::Array | ValueTag::NullableArray
        ) {
            let addr = self
                .memory
                .pointer(address, layout.value_kind_target_offset)?;
            self.schema(addr)?;
            Some(addr)
        } else {
            None
        };

        Ok(Value { tag, target_addr })
    }

    fn values(&mut self, address: u64, count: u32) -> Result<Vec<Value>> {
        let size = self.memory.layout.value_kind_size;
        let mut values = Vec::with_capacity(count as usize);
        for index in 0..count {
            let address = address
                .checked_add(u64::from(index) * u64::from(size))
                .ok_or("wasmgc values address overflow")?;
            values.push(self.value(address)?);
        }

        Ok(values)
    }

    pub fn resolve_data(&mut self) -> super::Result<()> {
        self.discarded.sort_unstable_by_key(|range| range.start);
        let mut merged: Vec<Range<u64>> = Vec::new();
        for range in self.discarded.drain(..) {
            if range.is_empty() {
                continue;
            }
            if let Some(previous) = merged.last_mut()
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        self.discarded = merged;

        let mut indices = Vec::with_capacity(self.memory.segments.len());
        let mut parts = Vec::with_capacity(self.memory.segments.len());
        let mut count = 0;
        for segment in &self.memory.segments {
            indices.push(count);
            let kept = if let Some((offset, bytes)) = segment {
                let end = self.memory.address(*offset, bytes.len() as u64)?;
                split(*offset..end, &self.discarded)
            } else {
                Vec::new()
            };
            count += kept.len().max(1) as u32;
            parts.push(kept);
        }

        self.new_data_indices = indices;
        self.new_data_count = count;
        self.new_data_parts = parts;
        Ok(())
    }

    fn resolve_globals(&mut self, discarded: &HashSet<u32>) {
        self.new_global_indices.clear();
        self.new_global_start = 0;
        for index in 0..self.original_globals {
            if discarded.contains(&index) {
                self.new_global_indices.push(None);
            } else {
                self.new_global_indices.push(Some(self.new_global_start));
                self.new_global_start += 1;
            }
        }
    }

    fn resolve_imports(&mut self) {
        self.function_imports
            .sort_unstable_by_key(|import| import.original_function_index);
        let count = self.function_imports.len() as u32;
        let start = self.new_function_indices[..self.original_function_imports as usize]
            .iter()
            .filter(|index| index.is_some())
            .count() as u32;
        for index in self
            .new_function_indices
            .iter_mut()
            .skip(self.original_function_imports as usize)
            .flatten()
        {
            *index += count;
        }
        self.new_function_start += count;
        for (offset, import) in self.function_imports.iter_mut().enumerate() {
            import.function_index = start + offset as u32;
            import.type_index =
                self.original_types + self.type_indices.len() as u32 + offset as u32;
        }
    }

    fn resolve_intrinsics(&mut self) {
        let start = self.new_function_start
            + u32::from(self.gc_table.is_some())
            + u32::from(self.externref_table.is_some());
        for (offset, &(function, _, _)) in self.intrinsics.iter().enumerate() {
            self.new_function_indices[function as usize] = Some(start + offset as u32);
        }
    }

    fn resolve_struct(&mut self) {
        let start = self.new_function_start
            + u32::from(self.gc_table.is_some())
            + u32::from(self.externref_table.is_some())
            + self.intrinsics.len() as u32;
        for (offset, export) in self.struct_exports.iter_mut().enumerate() {
            export.function_index = start + offset as u32;
        }
    }

    fn resolve_functions(&mut self) {
        let mut next = self.new_function_start
            + u32::from(self.gc_table.is_some())
            + u32::from(self.externref_table.is_some())
            + self.intrinsics.len() as u32
            + self.struct_exports.len() as u32;
        for export in &mut self.function_exports {
            if export
                .params
                .iter()
                .chain(&export.result)
                .any(|value| value.tag.is_reference())
            {
                export.function_index = next;
                next += 1;
            } else {
                export.function_index = self.new_function_indices
                    [export.original_function_index as usize]
                    .expect("retained Rust export bridge");
            }
        }
    }

    fn resolve(
        &mut self,
        marker_functions: HashMap<u32, Vec<u64>>,
        struct_descriptors: Vec<(u32, u64)>,
        function_descriptors: Vec<(u32, &str, u64)>,
        function_bridges: HashMap<&str, u32>,
        mut export_names: HashSet<String>,
    ) -> Result<()> {
        let layout = self.memory.layout;

        for (function, may_addresses) in marker_functions {
            self.placeholder(function, may_addresses)?;
        }

        let mut descriptor_globals = struct_descriptors
            .iter()
            .map(|(index, _)| *index)
            .collect::<HashSet<_>>();
        for (_, address) in struct_descriptors {
            let export = self.struct_export(address)?;
            if !export_names.insert(export.name.clone()) {
                return Err(format!("duplicate Wasm export name `{}`", export.name).into());
            }
            self.struct_exports.push(export);
        }

        for (global, name, address) in function_descriptors {
            let function = *function_bridges
                .get(name)
                .ok_or("wasmgc function descriptor has no bridge")?;
            let export = self.function_export(address, function)?;
            if !export_names.insert(export.name.clone()) {
                return Err(format!("duplicate Wasm export name `{}`", export.name).into());
            }
            descriptor_globals.insert(global);
            self.function_exports.push(export);
        }

        while let Some(address) = self.pending_resolve.pop() {
            let count = self
                .memory
                .read_u32(address, layout.schema_field_count_offset)?;

            let fields_address = self
                .memory
                .address(address, u64::from(layout.schema_fields_offset))?;
            let fields = self.values(fields_address, count)?;
            self.discard(
                address,
                u64::from(layout.schema_fields_offset)
                    + u64::from(count) * u64::from(layout.value_kind_size),
            )?;
            self.schema_set.get_mut(&address).unwrap().fields = fields;
        }

        self.resolve_types();
        self.resolve_globals(&descriptor_globals);
        self.resolve_imports();
        self.resolve_tables();
        self.resolve_intrinsics();
        self.resolve_struct();
        self.resolve_functions();
        self.resolve_data()
    }
}

fn split(segment: Range<u64>, discarded: &[Range<u64>]) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut cursor = segment.start;
    let first = discarded.partition_point(|range| range.end <= segment.start);

    for range in &discarded[first..] {
        if range.start >= segment.end {
            break;
        }
        if cursor < range.start {
            parts.push((cursor - segment.start) as usize..(range.start - segment.start) as usize);
        }
        cursor = range.end.min(segment.end);
    }

    if cursor < segment.end {
        parts.push((cursor - segment.start) as usize..(segment.end - segment.start) as usize);
    }
    if parts.is_empty() {
        // Preserve the original active segment's index, even after removing all its bytes.
        parts.push(0..0);
    }
    parts
}
