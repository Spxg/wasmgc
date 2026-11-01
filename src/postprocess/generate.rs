//! Generates replacement bodies and the declarations they depend on.
//!
//! Schemas are normalized before instruction emission. Original definitions keep
//! their order; new definitions receive indices after them. Generation finishes
//! by discarding signature lookups and index allocators, returning only the
//! declarations, replacement bodies, and function map needed for writing.

use std::collections::BTreeMap;

use super::{
    Bridge, Input, Operation, Result, Struct, Value, array_copy, externref,
    function::{Definition, FunctionRegistry},
    normalize, root,
    types::{FunctionTypes, schema_index, storage_type, value_type},
};
use crate::schema::{FunctionKind, OperationKind, SchemaKind, ValueTag};
use wasm_encoder::{
    ArrayType, CompositeInnerType, CompositeType, ConstExpr, FieldType, FuncType, Function,
    GlobalType, HeapType, Instruction as I, RefType, StructType, SubType, TableType, ValType,
};

pub(super) struct NativeImport {
    pub module: String,
    pub name: String,
    pub ty: u32,
}

/// Complete additions and replacements, with generated references already resolved.
pub(super) struct Changes {
    pub types: Vec<Vec<SubType>>,
    pub signatures: Vec<FuncType>,
    pub imports: Vec<NativeImport>,
    pub tables: Vec<TableType>,
    pub globals: Vec<(GlobalType, ConstExpr)>,
    pub functions: Vec<Definition>,
    pub replacements: BTreeMap<u32, Function>,
    pub exports: BTreeMap<String, u32>,
    pub function_map: Vec<Option<u32>>,
    pub first_body: u32,
}

pub(super) fn generate(mut input: Input) -> Result<Changes> {
    normalize::merge_array_types(&mut input);
    normalize::group_recursive_types(&mut input);

    let mut signatures = FunctionTypes::new(input.types + input.schemas.len() as u32);
    let mut function_map = vec![None; input.imports.len() + input.functions as usize];
    let mut host_count = 0;
    for (index, (name, _)) in input.imports.iter().enumerate() {
        if name.is_none() {
            function_map[index] = Some(host_count);
            host_count += 1;
        }
    }

    let mut imports = Vec::new();
    let mut import_bridges = BTreeMap::new();
    for bridge in input
        .bridges
        .iter()
        .filter(|bridge| bridge.kind == FunctionKind::Import)
    {
        let params = bridge.params.iter().map(|value| value_type(&input, value));
        let results = bridge.result.iter().map(|value| value_type(&input, value));
        let ty = signatures.intern(params, results);
        import_bridges.insert(bridge.function, host_count + imports.len() as u32);
        imports.push(NativeImport {
            module: bridge.module.clone(),
            name: bridge.name.clone(),
            ty,
        });
    }

    let first_original = host_count + imports.len() as u32;
    for offset in 0..input.functions {
        function_map[input.imports.len() + offset as usize] = Some(first_original + offset);
    }

    let mut functions = FunctionRegistry::new(first_original + input.functions);
    let mut exports = BTreeMap::new();
    let mut tables = Vec::new();
    let mut globals = Vec::new();
    let roots = root::Roots::new(
        &input,
        input.tables,
        input.globals,
        &mut signatures,
        &mut functions,
        &mut exports,
    );
    roots.append_table(&mut tables);
    roots.append_globals(&mut globals);

    let externref = externref::ExternRef::new(
        &input,
        input.tables + tables.len() as u32,
        input.globals + globals.len() as u32,
        &mut signatures,
        &mut functions,
        &mut exports,
    );
    externref.append_tables(&mut tables);
    externref.append_globals(&mut globals);

    let emitter = Emitter {
        input: &input,
        roots: &roots,
        externref: &externref,
    };
    for (original, (name, ty)) in input.imports.iter().enumerate() {
        if let Some(name) = name.as_deref().filter(|name| !name.starts_with("marker_")) {
            function_map[original] = Some(functions.push(*ty, emitter.intrinsic(name)?));
        }
    }

    let mut replacements: BTreeMap<_, _> = input
        .operations
        .iter()
        .map(|(&function, &operation)| (function, emitter.operation(operation)))
        .collect();
    for bridge in &input.bridges {
        if bridge.kind == FunctionKind::Import {
            let callee = import_bridges[&bridge.function];
            replacements.insert(bridge.function, emitter.import_bridge(bridge, callee));
        } else {
            let callee = function_map[bridge.function as usize].expect("retained Rust bridge");
            let function = if bridge.is_scalar_export() {
                callee
            } else {
                // Linkers may merge numeric Rust bodies whose native GC
                // signatures differ. Each bridge registers its own signature.
                let params = bridge.params.iter().map(|value| value_type(&input, value));
                let results = bridge.result.iter().map(|value| value_type(&input, value));
                functions.push(
                    signatures.intern(params, results),
                    emitter.export_bridge(bridge, callee),
                )
            };
            add_export(&mut exports, &bridge.name, function)?;
        }
    }
    for export in &input.struct_exports {
        let (params, results, body) = emitter.struct_export(export);
        let function = functions.push(signatures.intern(params, results), body);
        add_export(&mut exports, &export.name, function)?;
    }

    Ok(Changes {
        types: schema_types(&input),
        signatures: signatures.finish(),
        imports,
        tables,
        globals,
        functions: functions.finish(),
        replacements,
        exports,
        function_map,
        first_body: input.imports.len() as u32,
    })
}

fn add_export(exports: &mut BTreeMap<String, u32>, name: &str, function: u32) -> Result<()> {
    if exports.insert(name.to_owned(), function).is_some() {
        return Err(format!("duplicate wasmgc export {name}").into());
    }
    Ok(())
}

struct Emitter<'a> {
    input: &'a Input,
    roots: &'a root::Roots,
    externref: &'a externref::ExternRef,
}

impl Emitter<'_> {
    /// Import arguments borrow existing roots; results acquire owned roots.
    fn import_bridge(&self, bridge: &Bridge, callee: u32) -> Function {
        let mut function = Function::new([]);
        for (local, value) in bridge.params.iter().enumerate() {
            self.value_input(&mut function, value, local as u32);
        }
        function.instruction(&I::Call(callee));
        if let Some(value) = &bridge.result {
            self.value_output(&mut function, value);
        }
        function.instruction(&I::End);
        function
    }

    /// Export arguments acquire roots consumed by Rust. A returned root is
    /// released only after its reference has been placed on the operand stack.
    fn export_bridge(&self, bridge: &Bridge, callee: u32) -> Function {
        let rooted_result = bridge
            .result
            .as_ref()
            .filter(|value| value.tag.is_reference());
        let result_handle = bridge.params.len() as u32;
        let mut function = Function::new(rooted_result.map(|_| (1, ValType::I32)));
        for (local, value) in bridge.params.iter().enumerate() {
            function.instruction(&I::LocalGet(local as u32));
            self.value_output(&mut function, value);
        }
        function.instruction(&I::Call(callee));
        if let Some(value) = rooted_result {
            function.instruction(&I::LocalSet(result_handle));
            self.value_input(&mut function, value, result_handle);
            if value.tag == ValueTag::ExternRef {
                self.externref.value_release(&mut function, result_handle);
            } else {
                let nullable = matches!(value.tag, ValueTag::NullableRef | ValueTag::NullableArray);
                if nullable {
                    function
                        .instruction(&I::LocalGet(result_handle))
                        .instruction(&I::If(wasm_encoder::BlockType::Empty));
                }
                function
                    .instruction(&I::LocalGet(result_handle))
                    .instruction(&I::Call(self.roots.unroot));
                if nullable {
                    function.instruction(&I::End);
                }
            }
        }
        function.instruction(&I::End);
        function
    }

    fn struct_export(&self, export: &Struct) -> (Vec<ValType>, Vec<ValType>, Function) {
        let ty = schema_index(self.input, export.schema);
        let schema = &self.input.schemas[export.schema as usize];
        let owner = ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(ty),
        });
        let (params, results, instruction) = match export.kind {
            OperationKind::StructNew => (
                schema
                    .fields
                    .iter()
                    .map(|field| value_type(self.input, field))
                    .collect::<Vec<_>>(),
                vec![owner],
                I::StructNew(ty),
            ),
            OperationKind::StructGet => {
                let field = &schema.fields[export.field as usize];
                let instruction = if matches!(field.tag, ValueTag::I8 | ValueTag::I16) {
                    I::StructGetU {
                        struct_type_index: ty,
                        field_index: export.field,
                    }
                } else {
                    I::StructGet {
                        struct_type_index: ty,
                        field_index: export.field,
                    }
                };
                (
                    vec![owner],
                    vec![value_type(self.input, field)],
                    instruction,
                )
            }
            OperationKind::StructSet => (
                vec![
                    owner,
                    value_type(self.input, &schema.fields[export.field as usize]),
                ],
                Vec::new(),
                I::StructSet {
                    struct_type_index: ty,
                    field_index: export.field,
                },
            ),
            _ => unreachable!("struct export operation"),
        };
        let mut function = Function::new([]);
        for local in 0..params.len() as u32 {
            function.instruction(&I::LocalGet(local));
        }
        function.instruction(&instruction).instruction(&I::End);
        (params, results, function)
    }

    fn value_input(&self, function: &mut Function, value: &Value, local: u32) {
        if value.tag == ValueTag::ExternRef {
            self.externref.value_input(function, local);
            return;
        }

        if !value.tag.is_reference() {
            function.instruction(&I::LocalGet(local));
            return;
        }

        let ValType::Ref(reference) = value_type(self.input, value) else {
            unreachable!("reference value storage");
        };
        let heap_type = reference.heap_type;

        function
            .instruction(&I::LocalGet(local))
            .instruction(&I::TableGet(self.roots.table.index))
            .instruction(&if reference.nullable {
                I::RefCastNullable(heap_type)
            } else {
                I::RefCastNonNull(heap_type)
            });
    }

    fn value_output(&self, function: &mut Function, value: &Value) {
        if value.tag == ValueTag::ExternRef {
            self.externref.value_output(function);
        } else if value.tag.is_reference() {
            function.instruction(&I::Call(self.roots.root));
        }
    }

    fn heap_input(&self, function: &mut Function, ty: u32, local: u32) {
        function
            .instruction(&I::LocalGet(local))
            .instruction(&I::TableGet(self.roots.table.index))
            .instruction(&I::RefCastNonNull(HeapType::Concrete(ty)));
    }

    fn intrinsic(&self, name: &str) -> Result<Function> {
        let mut function = Function::new([]);

        match name {
            "ref_clone" => {
                function
                    .instruction(&I::LocalGet(0))
                    .instruction(&I::Call(self.roots.lookup))
                    .instruction(&I::Call(self.roots.root));
            }
            "ref_release" => {
                function
                    .instruction(&I::LocalGet(0))
                    .instruction(&I::Call(self.roots.unroot));
            }
            "ref_eq" => {
                function
                    .instruction(&I::LocalGet(0))
                    .instruction(&I::Call(self.roots.lookup))
                    .instruction(&I::LocalGet(1))
                    .instruction(&I::Call(self.roots.lookup))
                    .instruction(&I::RefEq);
            }
            _ => {
                return self
                    .externref
                    .intrinsic(name)
                    .ok_or_else(|| format!("unsupported wasmgc intrinsic {name}").into());
            }
        }

        function.instruction(&I::End);

        Ok(function)
    }

    fn operation(&self, operation: Operation) -> Function {
        let schema = &self.input.schemas[operation.schema as usize];
        let ty = schema_index(self.input, operation.schema);

        if matches!(
            operation.kind,
            OperationKind::ArrayFromSlice
                | OperationKind::ArrayRead
                | OperationKind::ArrayWrite
                | OperationKind::ArrayReadAbi
                | OperationKind::ArrayWriteAbi
        ) {
            return array_copy::memory_operation(
                operation.kind,
                ty,
                self.input.memory.is_some_and(|memory| memory.memory64),
                self.roots,
                schema.fields[0].tag,
                |function, local| self.value_input(function, &schema.fields[0], local),
                |function| self.value_output(function, &schema.fields[0]),
            );
        }

        let locals = (operation.kind == OperationKind::ArrayFill).then_some((
            1,
            ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(ty),
            }),
        ));
        let mut function = Function::new(locals);

        match operation.kind {
            OperationKind::StructNew => {
                for (local, field) in schema.fields.iter().enumerate() {
                    self.value_input(&mut function, field, local as u32);
                }
                function
                    .instruction(&I::StructNew(ty))
                    .instruction(&I::Call(self.roots.root));
            }
            OperationKind::StructGet | OperationKind::StructSet => {
                let field = &schema.fields[operation.field as usize];
                self.heap_input(&mut function, ty, 0);

                if operation.kind == OperationKind::StructSet {
                    self.value_input(&mut function, field, 1);
                    function.instruction(&I::StructSet {
                        struct_type_index: ty,
                        field_index: operation.field,
                    });
                } else {
                    let instruction = if matches!(field.tag, ValueTag::I8 | ValueTag::I16) {
                        I::StructGetU {
                            struct_type_index: ty,
                            field_index: operation.field,
                        }
                    } else {
                        I::StructGet {
                            struct_type_index: ty,
                            field_index: operation.field,
                        }
                    };
                    function.instruction(&instruction);
                    self.value_output(&mut function, field);
                }
            }
            OperationKind::ArrayNew => {
                self.value_input(&mut function, &schema.fields[0], 1);
                function
                    .instruction(&I::LocalGet(0))
                    .instruction(&I::ArrayNew(ty))
                    .instruction(&I::Call(self.roots.root));
            }
            OperationKind::ArrayNewDefault => {
                function
                    .instruction(&I::LocalGet(0))
                    .instruction(&I::ArrayNewDefault(ty))
                    .instruction(&I::Call(self.roots.root));
            }
            OperationKind::ArrayNewFixed => {
                function
                    .instruction(&I::ArrayNewFixed {
                        array_type_index: ty,
                        array_size: 0,
                    })
                    .instruction(&I::Call(self.roots.root));
            }
            OperationKind::ArrayLen => {
                self.heap_input(&mut function, ty, 0);
                function.instruction(&I::ArrayLen);
            }
            OperationKind::ArrayGet => {
                let element = &schema.fields[0];
                self.heap_input(&mut function, ty, 0);
                function.instruction(&I::LocalGet(1));
                let instruction = if matches!(element.tag, ValueTag::I8 | ValueTag::I16) {
                    I::ArrayGetU(ty)
                } else {
                    I::ArrayGet(ty)
                };
                function.instruction(&instruction);
                self.value_output(&mut function, element);
            }
            OperationKind::ArraySet => {
                self.heap_input(&mut function, ty, 0);
                function.instruction(&I::LocalGet(1));
                self.value_input(&mut function, &schema.fields[0], 2);
                function.instruction(&I::ArraySet(ty));
            }
            OperationKind::ArrayFill => {
                self.heap_input(&mut function, ty, 0);
                function
                    .instruction(&I::LocalTee(2))
                    .instruction(&I::I32Const(0));
                self.value_input(&mut function, &schema.fields[0], 1);
                function
                    .instruction(&I::LocalGet(2))
                    .instruction(&I::ArrayLen)
                    .instruction(&I::ArrayFill(ty));
            }
            OperationKind::ArrayCopy => {
                self.heap_input(&mut function, ty, 0);
                function.instruction(&I::LocalGet(1));
                self.heap_input(&mut function, ty, 2);
                function
                    .instruction(&I::LocalGet(3))
                    .instruction(&I::LocalGet(4))
                    .instruction(&I::ArrayCopy {
                        array_type_index_dst: ty,
                        array_type_index_src: ty,
                    });
            }
            OperationKind::ArrayFromSlice
            | OperationKind::ArrayRead
            | OperationKind::ArrayWrite
            | OperationKind::ArrayReadAbi
            | OperationKind::ArrayWriteAbi => unreachable!(),
        }

        function.instruction(&I::End);

        function
    }
}

fn schema_types(input: &Input) -> Vec<Vec<SubType>> {
    input
        .schema_groups
        .iter()
        .map(|group| {
            input.schemas[group.clone()]
                .iter()
                .map(|schema| {
                    let fields = || {
                        schema.fields.iter().map(|field| FieldType {
                            element_type: storage_type(input, field),
                            mutable: true,
                        })
                    };
                    let inner = match schema.kind {
                        SchemaKind::Struct => CompositeInnerType::Struct(StructType {
                            fields: fields().collect(),
                        }),
                        SchemaKind::Array => {
                            CompositeInnerType::Array(ArrayType(fields().next().unwrap()))
                        }
                    };
                    SubType {
                        is_final: true,
                        supertype_idxs: Vec::new(),
                        composite_type: CompositeType {
                            inner,
                            shared: false,
                            descriptor: None,
                            describes: None,
                        },
                    }
                })
                .collect()
        })
        .collect()
}
