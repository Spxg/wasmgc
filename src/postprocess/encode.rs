use super::Transformer;
use crate::schema::{FUNCTION_BRIDGE_PREFIX, FUNCTION_DESC_PREFIX, STRUCT_EXPORT_DESC_PREFIX};
use std::io;
use wasm_encoder::reencode::{self, Reencode};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ExportSection, FunctionSection, GlobalSection,
    ImportSection, IndirectNameMap, Module, NameMap, NameSection, SectionId, TableSection,
    TypeSection,
};

type EncodeResult<T = ()> = Result<T, reencode::Error<io::Error>>;

impl Transformer<'_> {
    pub fn finish(mut self, module: &[u8]) -> super::Result<Vec<u8>> {
        let mut output = Module::new();
        self.parse_core_module(&mut output, wasmparser::Parser::new(0), module)?;
        Ok(output.finish())
    }
}

impl Reencode for Transformer<'_> {
    type Error = io::Error;

    fn data_index(&mut self, index: u32) -> EncodeResult<u32> {
        self.resolver
            .new_data_indices
            .get(index as usize)
            .copied()
            .ok_or_else(|| {
                reencode::Error::UserError(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("data segment index {index} is out of bounds"),
                ))
            })
    }

    fn data_count(&mut self, _count: u32) -> EncodeResult<u32> {
        Ok(self.resolver.new_data_count)
    }

    fn function_index(&mut self, index: u32) -> EncodeResult<u32> {
        self.resolver
            .new_function_indices
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(|| {
                reencode::Error::UserError(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("function {index} has no retained output index"),
                ))
            })
    }

    fn global_index(&mut self, index: u32) -> EncodeResult<u32> {
        self.resolver
            .new_global_indices
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(|| {
                reencode::Error::UserError(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("global {index} has no retained output index"),
                ))
            })
    }

    fn parse_import_section(
        &mut self,
        imports: &mut ImportSection,
        section: wasmparser::ImportSectionReader<'_>,
    ) -> EncodeResult {
        for import in section.into_imports() {
            let import = import?;
            if import.module == "wasmgc"
                && matches!(
                    import.ty,
                    wasmparser::TypeRef::Func(_) | wasmparser::TypeRef::FuncExact(_)
                )
            {
                continue;
            }
            reencode::utils::parse_import(self, imports, import)?;
        }
        self.append_imports(imports);
        Ok(())
    }

    fn parse_type_section(
        &mut self,
        types: &mut TypeSection,
        section: wasmparser::TypeSectionReader<'_>,
    ) -> EncodeResult {
        reencode::utils::parse_type_section(self, types, section)?;
        self.append_types(types);
        Ok(())
    }

    fn parse_function_section(
        &mut self,
        functions: &mut FunctionSection,
        section: wasmparser::FunctionSectionReader<'_>,
    ) -> EncodeResult {
        reencode::utils::parse_function_section(self, functions, section)?;
        for (ty, _) in &self.functions {
            functions.function(*ty);
        }
        Ok(())
    }

    fn parse_table_section(
        &mut self,
        tables: &mut TableSection,
        section: wasmparser::TableSectionReader<'_>,
    ) -> EncodeResult {
        reencode::utils::parse_table_section(self, tables, section)?;
        self.append_tables(tables);
        Ok(())
    }

    fn parse_global_section(
        &mut self,
        globals: &mut GlobalSection,
        section: wasmparser::GlobalSectionReader<'_>,
    ) -> EncodeResult {
        for (offset, global) in section.into_iter().enumerate() {
            let global = global?;
            let index = self.resolver.original_global_imports + offset as u32;
            if self.resolver.new_global_indices[index as usize].is_some() {
                reencode::utils::parse_global(self, globals, global)?;
            }
        }
        self.append_globals(globals);
        Ok(())
    }

    fn parse_export_section(
        &mut self,
        exports: &mut ExportSection,
        section: wasmparser::ExportSectionReader<'_>,
    ) -> EncodeResult {
        for export in section {
            let export = export?;
            if (export.kind == wasmparser::ExternalKind::Global
                && (export.name.starts_with(STRUCT_EXPORT_DESC_PREFIX)
                    || export.name.starts_with(FUNCTION_DESC_PREFIX)))
                || (export.kind == wasmparser::ExternalKind::Func
                    && export.name.starts_with(FUNCTION_BRIDGE_PREFIX))
            {
                continue;
            }
            reencode::utils::parse_export(self, exports, export)?;
        }
        self.append_struct_exports(exports);
        self.append_function_exports(exports);
        Ok(())
    }

    fn parse_code_section(
        &mut self,
        code: &mut CodeSection,
        section: wasmparser::CodeSectionReader<'_>,
    ) -> EncodeResult {
        for (offset, body) in section.into_iter().enumerate() {
            let body = body?;
            let index = self.resolver.original_function_imports + offset as u32;
            if let Some(replacement) = self.replacements.get(&index) {
                code.function(replacement);
            } else {
                reencode::utils::parse_function_body(self, code, body)?;
            }
        }
        for (_, function) in &self.functions {
            code.function(function);
        }
        Ok(())
    }

    fn parse_data_section(
        &mut self,
        data: &mut DataSection,
        section: wasmparser::DataSectionReader<'_>,
    ) -> EncodeResult {
        for (index, datum) in section.into_iter().enumerate() {
            let datum = datum?;
            if let Some((offset, bytes)) = self.resolver.memory.segments[index] {
                for part in &self.resolver.new_data_parts[index] {
                    let offset = offset + part.start as u64;
                    let offset = if self.resolver.memory.layout.pointer_size == 8 {
                        ConstExpr::i64_const(offset as i64)
                    } else {
                        ConstExpr::i32_const(offset as i32)
                    };
                    data.active(0, &offset, bytes[part.clone()].iter().copied());
                }
            } else {
                reencode::utils::parse_data(self, data, datum)?;
            }
        }
        Ok(())
    }

    fn parse_custom_name_subsection(
        &mut self,
        names: &mut NameSection,
        section: wasmparser::Name<'_>,
    ) -> EncodeResult {
        let labels = matches!(&section, wasmparser::Name::Label(_));
        match section {
            wasmparser::Name::Function(entries) => {
                let mut remapped = Vec::new();
                for entry in entries {
                    let entry = entry?;
                    if let Some(index) = self
                        .resolver
                        .new_function_indices
                        .get(entry.index as usize)
                        .copied()
                        .flatten()
                    {
                        remapped.push((index, entry.name));
                    }
                }
                remapped.sort_unstable_by_key(|entry| entry.0);
                let mut map = NameMap::new();
                for (index, name) in remapped {
                    map.append(index, name);
                }
                names.functions(&map);
            }
            wasmparser::Name::Local(entries) | wasmparser::Name::Label(entries) => {
                let mut remapped = Vec::new();
                for entry in entries {
                    let entry = entry?;
                    if self.replacements.contains_key(&entry.index) {
                        continue;
                    }
                    if let Some(index) = self
                        .resolver
                        .new_function_indices
                        .get(entry.index as usize)
                        .copied()
                        .flatten()
                    {
                        remapped.push((index, reencode::utils::name_map(entry.names, Ok)?));
                    }
                }
                remapped.sort_unstable_by_key(|entry| entry.0);
                let mut map = IndirectNameMap::new();
                for (index, entry) in remapped {
                    map.append(index, &entry);
                }
                if labels {
                    names.labels(&map);
                } else {
                    names.locals(&map);
                }
            }
            wasmparser::Name::Global(entries) => {
                let mut map = NameMap::new();
                for entry in entries {
                    let entry = entry?;
                    if let Some(index) = self
                        .resolver
                        .new_global_indices
                        .get(entry.index as usize)
                        .copied()
                        .flatten()
                    {
                        map.append(index, entry.name);
                    }
                }
                names.globals(&map);
            }
            section => reencode::utils::parse_custom_name_subsection(self, names, section)?,
        }
        Ok(())
    }

    fn intersperse_section_hook(
        &mut self,
        module: &mut Module,
        after: Option<SectionId>,
        before: Option<SectionId>,
    ) -> EncodeResult {
        let order = |section| match section {
            SectionId::Custom => 0,
            SectionId::Type => 1,
            SectionId::Import => 2,
            SectionId::Function => 3,
            SectionId::Table => 4,
            SectionId::Memory => 5,
            SectionId::Tag => 6,
            SectionId::Global => 7,
            SectionId::Export => 8,
            SectionId::Start => 9,
            SectionId::Element => 10,
            SectionId::DataCount => 11,
            SectionId::Code => 12,
            SectionId::Data => 13,
        };
        let missing = |section| {
            after.is_none_or(|after| order(after) < order(section))
                && before.is_none_or(|before| order(section) < order(before))
        };

        if missing(SectionId::Type) {
            let mut types = TypeSection::new();
            self.append_types(&mut types);
            if !types.is_empty() {
                module.section(&types);
            }
        }
        if missing(SectionId::Import) {
            let mut imports = ImportSection::new();
            self.append_imports(&mut imports);
            if !imports.is_empty() {
                module.section(&imports);
            }
        }
        if missing(SectionId::Function) {
            let mut functions = FunctionSection::new();
            for (ty, _) in &self.functions {
                functions.function(*ty);
            }
            if !functions.is_empty() {
                module.section(&functions);
            }
        }
        if missing(SectionId::Table) {
            let mut tables = TableSection::new();
            self.append_tables(&mut tables);
            if !tables.is_empty() {
                module.section(&tables);
            }
        }
        if missing(SectionId::Global) {
            let mut globals = GlobalSection::new();
            self.append_globals(&mut globals);
            if !globals.is_empty() {
                module.section(&globals);
            }
        }
        if missing(SectionId::Export) {
            let mut exports = ExportSection::new();
            self.append_struct_exports(&mut exports);
            self.append_function_exports(&mut exports);
            if !exports.is_empty() {
                module.section(&exports);
            }
        }
        if missing(SectionId::Code) {
            let mut code = CodeSection::new();
            for (_, function) in &self.functions {
                code.function(function);
            }
            if !code.is_empty() {
                module.section(&code);
            }
        }
        Ok(())
    }
}
