use super::{Operation, Resolver, Value};
use crate::schema::{OperationKind, ValueTag};
use std::collections::HashMap;
use wasm_encoder::{
    BlockType, EntityType, ExportKind, ExportSection, FuncType, Function, HeapType, ImportSection,
    Instruction as I, MemArg, MemArgTarget, RefType, ValType,
};

pub struct Transformer<'a> {
    pub resolver: Resolver<'a>,
    pub signatures: Vec<FuncType>,
    pub functions: Vec<(u32, Function)>,
    pub replacements: HashMap<u32, Function>,
}

impl<'a> Transformer<'a> {
    pub fn new(resolver: Resolver<'a>) -> Self {
        Self {
            resolver,
            signatures: Vec::new(),
            functions: Vec::new(),
            replacements: HashMap::new(),
        }
    }

    pub fn transform(mut self) -> Self {
        self.function_import();
        self.root();
        self.intrinsic();
        self.operation();
        self.struct_export();
        self.function_export();
        self
    }

    pub fn append_imports(&self, imports: &mut ImportSection) {
        for import in &self.resolver.function_imports {
            imports.import(
                &import.module,
                &import.name,
                EntityType::Function(import.type_index),
            );
        }
    }

    pub fn function_import(&mut self) {
        for import in &self.resolver.function_imports {
            let signature = FuncType::new(
                import.params.iter().map(|value| self.value_type(value)),
                import.result.iter().map(|value| self.value_type(value)),
            );
            self.signatures.push(signature);

            let mut function = Function::new([]);
            for (local, value) in import.params.iter().enumerate() {
                self.value_input(&mut function, value, local as u32);
            }
            function.instruction(&I::Call(import.function_index));
            if let Some(value) = &import.result {
                self.value_output(&mut function, value);
            }
            function.instruction(&I::End);
            self.replacements
                .insert(import.original_function_index, function);
        }
    }

    pub fn value_input(&self, function: &mut Function, value: &Value, local: u32) {
        function.instruction(&I::LocalGet(local));

        if value.tag == ValueTag::ExternRef {
            function.instruction(&I::TableGet(
                self.resolver.externref_table.as_ref().unwrap().index,
            ));
            return;
        }

        if !value.tag.is_reference() {
            return;
        }

        let ValType::Ref(reference) = self.value_type(value) else {
            unreachable!();
        };
        function
            .instruction(&I::TableGet(self.resolver.gc_table.as_ref().unwrap().index))
            .instruction(&if reference.nullable {
                I::RefCastNullable(reference.heap_type)
            } else {
                I::RefCastNonNull(reference.heap_type)
            });
    }

    pub fn value_output(&self, function: &mut Function, value: &Value) {
        if value.tag == ValueTag::ExternRef {
            function.instruction(&I::Call(
                self.resolver.externref_table.as_ref().unwrap().root,
            ));
        } else if value.tag.is_reference() {
            function.instruction(&I::Call(self.resolver.gc_table.as_ref().unwrap().root));
        }
    }

    pub fn heap_input(&self, function: &mut Function, ty: u32, local: u32) {
        function
            .instruction(&I::LocalGet(local))
            .instruction(&I::TableGet(self.resolver.gc_table.as_ref().unwrap().index))
            .instruction(&I::RefCastNonNull(HeapType::Concrete(ty)));
    }

    fn operation(&mut self) {
        for (&index, operation) in &self.resolver.operations {
            if matches!(
                operation.kind,
                OperationKind::ArrayFromSlice
                    | OperationKind::ArrayRead
                    | OperationKind::ArrayWrite
                    | OperationKind::ArrayReadAbi
                    | OperationKind::ArrayWriteAbi
            ) {
                let function = self.array_memory_operation(operation);
                self.replacements.insert(index, function);
                continue;
            }

            let schema = &self.resolver.schema_set[&operation.schema_addr];
            let ty = self.resolver.type_indices[&operation.schema_addr];

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
                        .instruction(&I::Call(self.resolver.gc_table.as_ref().unwrap().root));
                }
                OperationKind::StructGet | OperationKind::StructSet => {
                    let field = &schema.fields[operation.field_index as usize];
                    self.heap_input(&mut function, ty, 0);

                    if operation.kind == OperationKind::StructSet {
                        self.value_input(&mut function, field, 1);
                        function.instruction(&I::StructSet {
                            struct_type_index: ty,
                            field_index: operation.field_index,
                        });
                    } else {
                        let instruction = if matches!(field.tag, ValueTag::I8 | ValueTag::I16) {
                            I::StructGetU {
                                struct_type_index: ty,
                                field_index: operation.field_index,
                            }
                        } else {
                            I::StructGet {
                                struct_type_index: ty,
                                field_index: operation.field_index,
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
                        .instruction(&I::Call(self.resolver.gc_table.as_ref().unwrap().root));
                }
                OperationKind::ArrayNewFixed => {
                    function
                        .instruction(&I::ArrayNewFixed {
                            array_type_index: ty,
                            array_size: 0,
                        })
                        .instruction(&I::Call(self.resolver.gc_table.as_ref().unwrap().root));
                }
                OperationKind::ArrayNewDefault => {
                    function
                        .instruction(&I::LocalGet(0))
                        .instruction(&I::ArrayNewDefault(ty))
                        .instruction(&I::Call(self.resolver.gc_table.as_ref().unwrap().root));
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
                _ => continue,
            }

            function.instruction(&I::End);
            self.replacements.insert(index, function);
        }
    }

    pub fn array_memory_operation(&self, operation: &Operation) -> Function {
        let ty = self.resolver.type_indices[&operation.schema_addr];
        let element = &self.resolver.schema_set[&operation.schema_addr].fields[0];
        let abi = matches!(
            operation.kind,
            OperationKind::ArrayReadAbi | OperationKind::ArrayWriteAbi
        );
        let tag = if abi {
            match element.tag {
                ValueTag::I64 | ValueTag::F32 | ValueTag::F64 => element.tag,
                _ => ValueTag::I32,
            }
        } else {
            element.tag
        };
        let (buffer_type, stride) = match tag {
            ValueTag::I8 | ValueTag::Bool => (ValType::I32, 1),
            ValueTag::I16 => (ValType::I32, 2),
            ValueTag::I32 => (ValType::I32, 4),
            ValueTag::I64 => (ValType::I64, 8),
            ValueTag::F32 => (ValType::F32, 4),
            ValueTag::F64 => (ValType::F64, 8),
            _ => unreachable!("scalar memory buffer storage"),
        };

        let mut locals = vec![
            (
                1,
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(ty),
                }),
            ),
            (1, ValType::I32),
        ];
        if operation.kind == OperationKind::ArrayWriteAbi {
            locals.push((1, buffer_type));
        }
        let from_slice = operation.kind == OperationKind::ArrayFromSlice;
        let (pointer, length, array, cursor, offset) = if from_slice {
            (0, 1, 2, 3, None)
        } else {
            (2, 3, 4, 5, Some(1))
        };
        let value_local = cursor + 1;
        let read_to_memory = matches!(
            operation.kind,
            OperationKind::ArrayRead | OperationKind::ArrayReadAbi
        );
        let memory = MemArg {
            offset: 0,
            align: 0,
            target: MemArgTarget::Memory(0),
            ordering: None,
        };
        let mut function = Function::new(locals);

        if from_slice {
            function
                .instruction(&I::LocalGet(length))
                .instruction(&I::ArrayNewDefault(ty))
                .instruction(&I::LocalSet(array));
        } else {
            self.heap_input(&mut function, ty, 0);
            function.instruction(&I::LocalSet(array));
        }

        function
            .instruction(&I::Block(BlockType::Empty))
            .instruction(&I::Loop(BlockType::Empty))
            .instruction(&I::LocalGet(cursor))
            .instruction(&I::LocalGet(length))
            .instruction(&I::I32GeU)
            .instruction(&I::BrIf(1));

        if read_to_memory {
            self.array_memory_address(&mut function, pointer, cursor, stride);
        }

        function.instruction(&I::LocalGet(array));
        if let Some(offset) = offset {
            function
                .instruction(&I::LocalGet(offset))
                .instruction(&I::LocalGet(cursor))
                .instruction(&I::I32Add);
        } else {
            function.instruction(&I::LocalGet(cursor));
        }

        if read_to_memory {
            function.instruction(&if matches!(element.tag, ValueTag::I8 | ValueTag::I16) {
                I::ArrayGetU(ty)
            } else {
                I::ArrayGet(ty)
            });
            if abi {
                self.value_output(&mut function, element);
            } else if tag == ValueTag::Bool {
                function.instruction(&I::I32Eqz).instruction(&I::I32Eqz);
            }
            function.instruction(&match tag {
                ValueTag::I8 | ValueTag::Bool => I::I32Store8(memory),
                ValueTag::I16 => I::I32Store16(memory),
                ValueTag::I32 => I::I32Store(memory),
                ValueTag::I64 => I::I64Store(memory),
                ValueTag::F32 => I::F32Store(memory),
                ValueTag::F64 => I::F64Store(memory),
                _ => unreachable!("scalar memory buffer storage"),
            });
        } else {
            self.array_memory_address(&mut function, pointer, cursor, stride);
            function.instruction(&match tag {
                ValueTag::I8 | ValueTag::Bool => I::I32Load8U(memory),
                ValueTag::I16 => I::I32Load16U(memory),
                ValueTag::I32 => I::I32Load(memory),
                ValueTag::I64 => I::I64Load(memory),
                ValueTag::F32 => I::F32Load(memory),
                ValueTag::F64 => I::F64Load(memory),
                _ => unreachable!("scalar memory buffer storage"),
            });
            if abi {
                function.instruction(&I::LocalSet(value_local));
                self.value_input(&mut function, element, value_local);
            }
            function.instruction(&I::ArraySet(ty));
        }

        function
            .instruction(&I::LocalGet(cursor))
            .instruction(&I::I32Const(1))
            .instruction(&I::I32Add)
            .instruction(&I::LocalSet(cursor))
            .instruction(&I::Br(0))
            .instruction(&I::End)
            .instruction(&I::End);

        if from_slice {
            function
                .instruction(&I::LocalGet(array))
                .instruction(&I::Call(self.resolver.gc_table.as_ref().unwrap().root));
        }
        function.instruction(&I::End);
        function
    }

    fn array_memory_address(
        &self,
        function: &mut Function,
        pointer: u32,
        cursor: u32,
        stride: u32,
    ) {
        function
            .instruction(&I::LocalGet(pointer))
            .instruction(&I::LocalGet(cursor));

        if self.resolver.memory.layout.pointer_size == 8 {
            function.instruction(&I::I64ExtendI32U);
            if stride != 1 {
                function
                    .instruction(&I::I64Const(i64::from(stride)))
                    .instruction(&I::I64Mul);
            }
            function.instruction(&I::I64Add);
        } else {
            if stride != 1 {
                function
                    .instruction(&I::I32Const(stride as i32))
                    .instruction(&I::I32Mul);
            }
            function.instruction(&I::I32Add);
        }
    }

    pub fn append_struct_exports(&self, exports: &mut ExportSection) {
        for descriptor in &self.resolver.struct_exports {
            exports.export(
                &descriptor.name,
                ExportKind::Func,
                descriptor.function_index,
            );
        }
    }

    pub fn struct_export(&mut self) {
        for export in &self.resolver.struct_exports {
            let schema = &self.resolver.schema_set[&export.schema_addr];
            let ty = self.resolver.type_indices[&export.schema_addr];
            let owner = ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(ty),
            });
            let (params, results, instruction) = match export.kind {
                OperationKind::StructNew => (
                    schema
                        .fields
                        .iter()
                        .map(|field| self.value_type(field))
                        .collect::<Vec<_>>(),
                    vec![owner],
                    I::StructNew(ty),
                ),
                OperationKind::StructGet => {
                    let field = &schema.fields[export.field_index as usize];
                    let instruction = if matches!(field.tag, ValueTag::I8 | ValueTag::I16) {
                        I::StructGetU {
                            struct_type_index: ty,
                            field_index: export.field_index,
                        }
                    } else {
                        I::StructGet {
                            struct_type_index: ty,
                            field_index: export.field_index,
                        }
                    };
                    (vec![owner], vec![self.value_type(field)], instruction)
                }
                OperationKind::StructSet => (
                    vec![
                        owner,
                        self.value_type(&schema.fields[export.field_index as usize]),
                    ],
                    Vec::new(),
                    I::StructSet {
                        struct_type_index: ty,
                        field_index: export.field_index,
                    },
                ),
                _ => unreachable!("struct export operation"),
            };

            let mut function = Function::new([]);
            for local in 0..params.len() as u32 {
                function.instruction(&I::LocalGet(local));
            }
            function.instruction(&instruction).instruction(&I::End);

            let signature = self.resolver.original_types
                + self.resolver.schema_set.len() as u32
                + self.signatures.len() as u32;
            self.signatures.push(FuncType::new(params, results));
            self.functions.push((signature, function));
        }
    }

    pub fn append_function_exports(&self, exports: &mut ExportSection) {
        for export in &self.resolver.function_exports {
            exports.export(&export.name, ExportKind::Func, export.function_index);
        }
    }

    pub fn function_export(&mut self) {
        for export in &self.resolver.function_exports {
            if !export
                .params
                .iter()
                .chain(&export.result)
                .any(|value| value.tag.is_reference())
            {
                continue;
            }

            let rooted_result = export
                .result
                .as_ref()
                .filter(|value| value.tag.is_reference());
            let result_handle = export.params.len() as u32;
            let mut function = Function::new(rooted_result.map(|_| (1, ValType::I32)));
            for (local, value) in export.params.iter().enumerate() {
                function.instruction(&I::LocalGet(local as u32));
                self.value_output(&mut function, value);
            }
            function.instruction(&I::Call(
                self.resolver.new_function_indices[export.original_function_index as usize]
                    .expect("function export callee index"),
            ));

            if let Some(value) = rooted_result {
                function.instruction(&I::LocalSet(result_handle));
                self.value_input(&mut function, value, result_handle);
                let table = if value.tag == ValueTag::ExternRef {
                    self.resolver.externref_table.as_ref().unwrap()
                } else {
                    self.resolver.gc_table.as_ref().unwrap()
                };
                let nullable = matches!(value.tag, ValueTag::NullableRef | ValueTag::NullableArray);
                if nullable {
                    function
                        .instruction(&I::LocalGet(result_handle))
                        .instruction(&I::If(BlockType::Empty));
                }
                self.release(&mut function, table, result_handle);
                if nullable {
                    function.instruction(&I::End);
                }
            }
            function.instruction(&I::End);

            let signature = FuncType::new(
                export.params.iter().map(|value| self.value_type(value)),
                export.result.iter().map(|value| self.value_type(value)),
            );
            let ty = self.resolver.original_types
                + self.resolver.schema_set.len() as u32
                + self.signatures.len() as u32;
            self.signatures.push(signature);
            self.functions.push((ty, function));
        }
    }
}
