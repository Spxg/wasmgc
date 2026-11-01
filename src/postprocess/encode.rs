//! Writes generated changes into a linked Wasm module.
//!
//! Original definitions retain their order. Generated definitions follow them,
//! while the function map updates references throughout the original module.
//! Generated bodies already contain final indices and are written directly.

use super::{Result, generate::Changes};
use crate::schema::{FUNCTION_BRIDGE_PREFIX, FUNCTION_DESC_PREFIX, STRUCT_EXPORT_DESC_PREFIX};
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, FunctionSection, GlobalSection, ImportSection, Module,
    SectionId, TableSection, TypeSection,
    reencode::{self, Reencode},
};
use wasmparser::Parser;

pub(super) fn rewrite(bytes: &[u8], changes: Changes) -> Result<Vec<u8>> {
    let mut encoder = Encoder {
        next_body: changes.first_body,
        changes,
    };
    let mut module = Module::new();
    encoder.parse_core_module(&mut module, Parser::new(0), bytes)?;
    Ok(module.finish())
}

struct Encoder {
    changes: Changes,
    next_body: u32,
}

impl Encoder {
    fn append_types(&self, types: &mut TypeSection) {
        for group in &self.changes.types {
            types.ty().rec(group.iter().cloned());
        }
        for signature in &self.changes.signatures {
            types.ty().function(
                signature.params().iter().copied(),
                signature.results().iter().copied(),
            );
        }
    }

    fn append_imports(&self, imports: &mut ImportSection) {
        for import in &self.changes.imports {
            imports.import(
                &import.module,
                &import.name,
                wasm_encoder::EntityType::Function(import.ty),
            );
        }
    }

    fn append_tables(&self, tables: &mut TableSection) {
        for table in &self.changes.tables {
            tables.table(*table);
        }
    }

    fn append_globals(&self, globals: &mut GlobalSection) {
        for (ty, init) in &self.changes.globals {
            globals.global(*ty, init);
        }
    }

    fn append_exports(&self, exports: &mut ExportSection) {
        for (name, &function) in &self.changes.exports {
            exports.export(name, ExportKind::Func, function);
        }
    }

    fn write_functions(
        &mut self,
        functions: &mut FunctionSection,
        original: Option<wasmparser::FunctionSectionReader<'_>>,
    ) -> EncodeResult {
        if let Some(original) = original {
            reencode::utils::parse_function_section(self, functions, original)?;
        }
        for definition in &self.changes.functions {
            functions.function(definition.ty);
        }
        Ok(())
    }

    fn write_code(
        &mut self,
        code: &mut CodeSection,
        original: Option<wasmparser::CodeSectionReader<'_>>,
    ) -> EncodeResult {
        if let Some(original) = original {
            reencode::utils::parse_code_section(self, code, original)?;
        }
        for definition in &self.changes.functions {
            code.function(&definition.body);
        }
        Ok(())
    }
}

type EncodeResult<T = ()> = std::result::Result<T, reencode::Error<std::io::Error>>;

impl Reencode for Encoder {
    type Error = std::io::Error;

    fn function_index(&mut self, index: u32) -> EncodeResult<u32> {
        self.changes
            .function_map
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(|| {
                reencode::Error::UserError(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "reference to a removed wasmgc marker",
                ))
            })
    }

    fn parse_import(
        &mut self,
        imports: &mut ImportSection,
        import: wasmparser::Import<'_>,
    ) -> EncodeResult {
        if import.module != "wasmgc" {
            reencode::utils::parse_import(self, imports, import)?;
        }

        Ok(())
    }

    fn parse_import_section(
        &mut self,
        imports: &mut ImportSection,
        section: wasmparser::ImportSectionReader<'_>,
    ) -> EncodeResult {
        for import in section.into_imports() {
            self.parse_import(imports, import?)?;
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
        self.write_functions(functions, Some(section))
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
        reencode::utils::parse_global_section(self, globals, section)?;
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

            if self.changes.exports.contains_key(export.name) {
                return Err(reencode::Error::UserError(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("reserved wasmgc export {} already exists", export.name),
                )));
            }

            self.parse_export(exports, export)?;
        }

        self.append_exports(exports);

        Ok(())
    }

    fn parse_code_section(
        &mut self,
        code: &mut CodeSection,
        section: wasmparser::CodeSectionReader<'_>,
    ) -> EncodeResult {
        self.write_code(code, Some(section))
    }

    fn parse_function_body(
        &mut self,
        code: &mut CodeSection,
        body: wasmparser::FunctionBody<'_>,
    ) -> EncodeResult {
        let original = self.next_body;
        self.next_body += 1;

        if let Some(replacement) = self.changes.replacements.get(&original) {
            code.function(replacement);
        } else {
            reencode::utils::parse_function_body(self, code, body)?;
        }

        Ok(())
    }

    fn parse_custom_name_subsection(
        &mut self,
        names: &mut wasm_encoder::NameSection,
        section: wasmparser::Name<'_>,
    ) -> EncodeResult {
        let labels = matches!(&section, wasmparser::Name::Label(_));

        match section {
            wasmparser::Name::Function(map) => {
                let mut entries = Vec::new();

                for entry in map {
                    let entry = entry?;

                    if self
                        .changes
                        .function_map
                        .get(entry.index as usize)
                        .is_some_and(Option::is_none)
                    {
                        continue;
                    }

                    entries.push((self.function_index(entry.index)?, entry.name));
                }

                // The new function order can differ from the original import order, but
                // function name maps still require monotonically increasing indices.
                entries.sort_unstable_by_key(|(index, _)| *index);

                let mut map = wasm_encoder::NameMap::new();

                for (index, name) in entries {
                    map.append(index, name);
                }

                names.functions(&map);
            }
            wasmparser::Name::Local(map) | wasmparser::Name::Label(map) => {
                let mut entries = Vec::new();

                for entry in map {
                    let entry = entry?;

                    // Generated bodies have different locals and labels. Their original names
                    // would describe the removed shim rather than the replacement code.
                    if !self.changes.replacements.contains_key(&entry.index)
                        && !self
                            .changes
                            .function_map
                            .get(entry.index as usize)
                            .is_some_and(Option::is_none)
                    {
                        entries.push((
                            self.function_index(entry.index)?,
                            reencode::utils::name_map(entry.names, Ok)?,
                        ));
                    }
                }

                entries.sort_unstable_by_key(|(index, _)| *index);

                let mut map = wasm_encoder::IndirectNameMap::new();

                for (index, names) in entries {
                    map.append(index, &names);
                }

                if labels {
                    names.labels(&map);
                } else {
                    names.locals(&map);
                }
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
        // Wasm section order differs from numeric section IDs for tags and data
        // counts. Insert missing sections at their semantic position even when
        // the input contained no types, imports, or definitions of that kind.
        fn order(section: SectionId) -> u8 {
            match section {
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
            }
        }

        for section in [
            SectionId::Type,
            SectionId::Import,
            SectionId::Function,
            SectionId::Table,
            SectionId::Global,
            SectionId::Export,
            SectionId::Code,
        ] {
            if after.is_some_and(|after| order(after) >= order(section))
                || before.is_some_and(|before| order(before) <= order(section))
            {
                continue;
            }

            match section {
                SectionId::Type => {
                    let mut types = TypeSection::new();
                    self.append_types(&mut types);
                    if !types.is_empty() {
                        module.section(&types);
                    }
                }
                SectionId::Import => {
                    let mut imports = ImportSection::new();
                    self.append_imports(&mut imports);
                    if !imports.is_empty() {
                        module.section(&imports);
                    }
                }
                SectionId::Function => {
                    let mut functions = FunctionSection::new();
                    self.write_functions(&mut functions, None)?;
                    if !functions.is_empty() {
                        module.section(&functions);
                    }
                }
                SectionId::Table => {
                    let mut tables = TableSection::new();
                    self.append_tables(&mut tables);
                    if !tables.is_empty() {
                        module.section(&tables);
                    }
                }
                SectionId::Global => {
                    let mut globals = GlobalSection::new();
                    self.append_globals(&mut globals);
                    if !globals.is_empty() {
                        module.section(&globals);
                    }
                }
                SectionId::Export => {
                    let mut exports = ExportSection::new();
                    self.append_exports(&mut exports);
                    if !exports.is_empty() {
                        module.section(&exports);
                    }
                }
                SectionId::Code => {
                    let mut code = CodeSection::new();
                    self.write_code(&mut code, None)?;
                    if !code.is_empty() {
                        module.section(&code);
                    }
                }
                _ => unreachable!(),
            }
        }

        Ok(())
    }
}
