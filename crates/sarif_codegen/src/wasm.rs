use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{
    CodegenValueKind as WasmValueKind, Function, Inst, LocalSlotId, Program, ValueId,
    for_each_inst_recursive,
};

const PAYLOAD_ENUM_SIZE: u32 = 16;

#[derive(Debug)]
pub struct WasmError {
    pub message: String,
}

impl WasmError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum WasmType {
    I64,
    F64,
}

#[derive(Clone, Debug)]
pub(crate) struct WasmRecord {
    pub(crate) size: u32,
    pub(crate) fields: Vec<WasmField>,
}

#[derive(Clone, Debug)]
pub(crate) struct WasmField {
    pub(crate) name: String,
    pub(crate) kind: WasmValueKind,
    pub(crate) offset: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct WasmEnum {
    pub(crate) variants: Vec<WasmEnumVariant>,
}

#[derive(Clone, Debug)]
pub(crate) struct WasmEnumVariant {
    pub(crate) name: String,
    pub(crate) payload: Option<WasmValueKind>,
}

impl WasmType {
    #[must_use]
    const fn render(self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::F64 => "f64",
        }
    }
}

pub(crate) fn enum_is_payload_free(enum_ty: &WasmEnum) -> bool {
    enum_ty.variants.iter().all(|v| v.payload.is_none())
}

mod memory;
mod runtime;
mod runtime_gen;

pub use runtime::{run_function_wasm, run_main_wasm};

pub fn emit_wat(program: &Program) -> Result<String, WasmError> {
    let emitter = WasmEmitter::new(program)?;
    emitter.emit()
}

pub fn emit_wasm(program: &Program) -> Result<Vec<u8>, WasmError> {
    let emitter = WasmEmitter::new(program)?;

    if std::env::var("SARIF_DEBUG_RUNTIME").is_ok() {
        let runtime_binary =
            runtime_gen::emit_runtime_module(program, &emitter.records, &emitter.enums)?;
        eprintln!("[runtime_gen produced {} bytes]", runtime_binary.len());
    }

    let wat = emitter.emit()?;

    if std::env::var("SARIF_DEBUG_WASM").is_ok() {
        eprintln!("{wat}");
    }
    wat::parse_str(&wat).map_err(|error| WasmError::new(error.to_string()))
}

struct WasmEmitter<'a> {
    program: &'a Program,
    records: BTreeMap<String, WasmRecord>,
    enums: BTreeMap<String, WasmEnum>,
}

impl<'a> WasmEmitter<'a> {
    fn new(program: &'a Program) -> Result<Self, WasmError> {
        let mut records = BTreeMap::new();
        let mut enums = BTreeMap::new();

        for struct_ty in &program.structs {
            let mut fields = Vec::new();
            let mut offset = 0;
            for field in &struct_ty.fields {
                let kind = wasm_value_kind_from_name(&field.ty, &program.structs, &program.enums)?;
                fields.push(WasmField {
                    name: field.name.clone(),
                    kind: kind.clone(),
                    offset,
                });
                offset += 8;
            }
            records.insert(
                struct_ty.name.clone(),
                WasmRecord {
                    size: offset,
                    fields,
                },
            );
        }

        for enum_ty in &program.enums {
            let mut variants = Vec::new();
            for variant in &enum_ty.variants {
                let payload = variant
                    .payload_type
                    .as_ref()
                    .map(|ty| wasm_value_kind_from_name(ty, &program.structs, &program.enums))
                    .transpose()?;
                variants.push(WasmEnumVariant {
                    name: variant.name.clone(),
                    payload,
                });
            }
            enums.insert(enum_ty.name.clone(), WasmEnum { variants });
        }

        Ok(Self {
            program,
            records,
            enums,
        })
    }

    fn emit(&self) -> Result<String, WasmError> {
        let mut output = String::new();
        output.push_str(include_str!("wasm/preamble.wat"));

        self.emit_support_functions(&mut output)?;

        for function in &self.program.functions {
            self.emit_function(&mut output, function)?;
        }

        writeln!(output, ")").expect("writing to a string cannot fail");
        Ok(output)
    }

    fn emit_support_functions(&self, output: &mut String) -> Result<(), WasmError> {
        output.push_str(include_str!("wasm/runtime_text.wat"));

        output.push_str(include_str!("wasm/runtime_builder.wat"));

        output.push_str(include_str!("wasm/runtime_list.wat"));

        output.push_str(
            "  (func $__sarif_arg_count (result i64)\n    call $__host_argc\n  )\n\
             (func $__sarif_arg_text (param $index i64) (result i64)\n\
               (local $buf i32) (local $written i32)\n\
               local.get $index\n\
               i32.const 4096\n    call $alloc\n    local.tee $buf\n\
               i32.const 4096\n    call $__host_argv\n\
               local.tee $written\n\
               i32.const 0\n    i32.lt_s\n    if\n\
                 i64.const 0\n      return\n    end\n\
               local.get $buf\n    local.get $written\n    call $__sarif_pack_text\n  )\n\
             (func $__sarif_stdin_text (result i64)\n\
               (local $buf i32) (local $read i32)\n\
               i32.const 8192\n    call $alloc\n    local.tee $buf\n\
               i32.const 8192\n    call $__host_stdin_read\n\
               local.tee $read\n\
               i32.const 0\n    i32.lt_s\n    if\n\
                 i64.const 0\n      return\n    end\n\
               local.get $buf\n    local.get $read\n    call $__sarif_pack_text\n  )\n\
             (func $__sarif_stdin_bytes (result i64)\n\
               (local $buf i32) (local $read i32)\n\
               i32.const 8192\n    call $alloc\n    local.tee $buf\n\
               i32.const 8192\n    call $__host_stdin_read\n\
               local.tee $read\n\
               i32.const 0\n    i32.lt_s\n    if\n\
                 i64.const 0\n      return\n    end\n\
               local.get $buf\n    local.get $read\n    call $__sarif_pack_text\n  )\n",
        );

        for (name, record) in &self.records {
            self.emit_record_eq_helper(output, name, record)?;
        }
        for (name, enum_ty) in &self.enums {
            if !enum_is_payload_free(enum_ty) {
                self.emit_enum_eq_helper(output, name, enum_ty)?;
            }
        }

        Ok(())
    }

    fn emit_record_eq_helper(
        &self,
        output: &mut String,
        name: &str,
        record: &WasmRecord,
    ) -> Result<(), WasmError> {
        writeln!(
            output,
            "  (func {} (param $left i64) (param $right i64) (result i64) (local $result i64)",
            record_eq_helper_name(name)
        )
        .expect("writing to a string cannot fail");
        writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
        writeln!(output, "    local.set $result").expect("writing to a string cannot fail");
        for field in &record.fields {
            writeln!(output, "    local.get $result").expect("writing to a string cannot fail");
            self.emit_memory_kind_equality(output, &field.kind, "$left", "$right", field.offset)?;
            writeln!(output, "    i64.and").expect("writing to a string cannot fail");
            writeln!(output, "    local.set $result").expect("writing to a string cannot fail");
        }
        writeln!(output, "    local.get $result").expect("writing to a string cannot fail");
        writeln!(output, "  )").expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_enum_eq_helper(
        &self,
        output: &mut String,
        name: &str,
        enum_ty: &WasmEnum,
    ) -> Result<(), WasmError> {
        writeln!(
            output,
            "  (func {} (param $left i64) (param $right i64) (result i64) (local $left_tag i64) (local $right_tag i64) (local $left_matches i64) (local $result i64)",
            enum_eq_helper_name(name)
        )
        .expect("writing to a string cannot fail");
        writeln!(output, "    local.get $left").expect("writing to a string cannot fail");
        writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
        writeln!(output, "    i64.load").expect("writing to a string cannot fail");
        writeln!(output, "    local.set $left_tag").expect("writing to a string cannot fail");
        writeln!(output, "    local.get $right").expect("writing to a string cannot fail");
        writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
        writeln!(output, "    i64.load").expect("writing to a string cannot fail");
        writeln!(output, "    local.set $right_tag").expect("writing to a string cannot fail");
        writeln!(output, "    local.get $left_tag").expect("writing to a string cannot fail");
        writeln!(output, "    local.get $right_tag").expect("writing to a string cannot fail");
        writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
        writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
        writeln!(output, "    local.set $result").expect("writing to a string cannot fail");
        for (index, variant) in enum_ty.variants.iter().enumerate() {
            let Some(payload_kind) = &variant.payload else {
                continue;
            };
            writeln!(output, "    local.get $left_tag").expect("writing to a string cannot fail");
            writeln!(output, "    i64.const {}", index).expect("writing to a string cannot fail");
            writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
            writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
            writeln!(output, "    local.set $left_matches")
                .expect("writing to a string cannot fail");
            writeln!(output, "    local.get $result").expect("writing to a string cannot fail");
            writeln!(output, "    local.get $left_matches")
                .expect("writing to a string cannot fail");
            writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
            writeln!(output, "    i64.xor").expect("writing to a string cannot fail");
            self.emit_memory_kind_equality(output, payload_kind, "$left", "$right", 8)?;
            writeln!(output, "    i64.or").expect("writing to a string cannot fail");
            writeln!(output, "    i64.and").expect("writing to a string cannot fail");
            writeln!(output, "    local.set $result").expect("writing to a string cannot fail");
        }
        writeln!(output, "    local.get $result").expect("writing to a string cannot fail");
        writeln!(output, "  )").expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_memory_kind_equality(
        &self,
        output: &mut String,
        kind: &WasmValueKind,
        left_base: &str,
        right_base: &str,
        offset: u32,
    ) -> Result<(), WasmError> {
        match kind {
            WasmValueKind::Unit => {
                writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
            }
            WasmValueKind::F64 => {
                self.emit_memory_load(output, left_base, offset, WasmType::F64);
                self.emit_memory_load(output, right_base, offset, WasmType::F64);
                writeln!(output, "    f64.eq").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
            }
            WasmValueKind::Text => {
                self.emit_memory_load(output, left_base, offset, WasmType::I64);
                self.emit_memory_load(output, right_base, offset, WasmType::I64);
                writeln!(output, "    call $__sarif_text_eq")
                    .expect("writing to a string cannot fail");
            }
            WasmValueKind::Bytes => {
                self.emit_memory_load(output, left_base, offset, WasmType::I64);
                self.emit_memory_load(output, right_base, offset, WasmType::I64);
                writeln!(output, "    call $__sarif_text_eq")
                    .expect("writing to a string cannot fail");
            }
            WasmValueKind::Record(name) => {
                self.emit_memory_load(output, left_base, offset, WasmType::I64);
                self.emit_memory_load(output, right_base, offset, WasmType::I64);
                writeln!(output, "    call {}", record_eq_helper_name(name))
                    .expect("writing to a string cannot fail");
            }
            WasmValueKind::Enum(name) => {
                self.emit_memory_load(output, left_base, offset, WasmType::I64);
                self.emit_memory_load(output, right_base, offset, WasmType::I64);
                if enum_is_payload_free(&self.enums[name]) {
                    writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.extend_i32_u")
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    call {}", enum_eq_helper_name(name))
                        .expect("writing to a string cannot fail");
                }
            }
            WasmValueKind::I32
            | WasmValueKind::Bool
            | WasmValueKind::TextIndex
            | WasmValueKind::TextBuilder
            | WasmValueKind::List(_)
            | WasmValueKind::File => {
                self.emit_memory_load(output, left_base, offset, WasmType::I64);
                self.emit_memory_load(output, right_base, offset, WasmType::I64);
                writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
            }
        }
        Ok(())
    }

    fn emit_memory_load(&self, output: &mut String, base: &str, offset: u32, ty: WasmType) {
        writeln!(output, "    local.get {}", base).expect("writing to a string cannot fail");
        writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
        if offset > 0 {
            writeln!(output, "    i32.const {}", offset).expect("writing to a string cannot fail");
            writeln!(output, "    i32.add").expect("writing to a string cannot fail");
        }
        let op = match ty {
            WasmType::I64 => "i64.load",
            WasmType::F64 => "f64.load",
        };
        writeln!(output, "    {}", op).expect("writing to a string cannot fail");
    }

    fn emit_function(&self, output: &mut String, function: &Function) -> Result<(), WasmError> {
        let mut kinds = BTreeMap::new();
        collect_inst_kinds(
            function,
            &function.instructions,
            &self.program.structs,
            &self.program.enums,
            &self.program.functions,
            &mut kinds,
        )?;

        let return_kind = if let Some(ty) = &function.return_type {
            wasm_value_kind_from_name(ty, &self.program.structs, &self.program.enums)?
        } else {
            WasmValueKind::Unit
        };

        write!(output, "  (func ${}", function.name).expect("writing to a string cannot fail");
        write!(output, " (export \"{}\")", function.name).expect("writing to a string cannot fail");

        for (i, param) in function.params.iter().enumerate() {
            let kind =
                wasm_value_kind_from_name(&param.ty, &self.program.structs, &self.program.enums)?;
            write!(
                output,
                " (param $p{} {})",
                i,
                wasm_type_from_kind(&kind).render()
            )
            .expect("writing to a string cannot fail");
        }

        if let Some(ty) = wasm_type_from_kind_result(&return_kind) {
            write!(output, " (result {})", ty.render()).expect("writing to a string cannot fail");
        }
        writeln!(output).expect("writing to a string cannot fail");

        for local in &function.mutable_locals {
            let kind =
                wasm_value_kind_from_name(&local.ty, &self.program.structs, &self.program.enums)?;
            writeln!(
                output,
                "    (local ${} {})",
                wasm_slot(local.slot),
                wasm_type_from_kind(&kind).render()
            )
            .expect("writing to a string cannot fail");
        }

        let locals = self.collect_locals(function, &function.instructions, &kinds)?;
        for (id, kind) in &locals {
            writeln!(
                output,
                "    (local ${} {})",
                wasm_id(*id),
                wasm_type_from_kind(kind).render()
            )
            .expect("writing to a string cannot fail");
        }

        let mut repeat_counters = BTreeSet::new();
        for_each_inst_recursive(&function.instructions, &mut |inst| {
            if let Inst::Repeat { count, .. } = inst {
                repeat_counters.insert(wasm_id(*count));
            }
        });
        for counter in repeat_counters {
            writeln!(output, "    (local $repeat_counter_{} i64)", counter)
                .expect("writing to a string cannot fail");
        }

        for inst in &function.instructions {
            self.emit_inst(output, function, inst, &kinds)?;
        }

        if let Some(res) = function.result {
            writeln!(output, "    local.get ${}", wasm_id(res))
                .expect("writing to a string cannot fail");
        }

        writeln!(output, "  )").expect("writing to a string cannot fail");
        Ok(())
    }

    #[allow(clippy::only_used_in_recursion)]
    fn collect_locals(
        &self,
        function: &Function,
        instructions: &[Inst],
        kinds: &BTreeMap<ValueId, WasmValueKind>,
    ) -> Result<BTreeMap<ValueId, WasmValueKind>, WasmError> {
        let mut locals = BTreeMap::new();
        for inst in instructions {
            match inst {
                Inst::LoadParam { dest, .. }
                | Inst::LoadLocal { dest, .. }
                | Inst::ConstInt { dest, .. }
                | Inst::ConstF64 { dest, .. }
                | Inst::ConstBool { dest, .. }
                | Inst::ConstText { dest, .. }
                | Inst::TextLen { dest, .. }
                | Inst::BytesLen { dest, .. }
                | Inst::TextByte { dest, .. }
                | Inst::BytesByte { dest, .. }
                | Inst::TextCmp { dest, .. }
                | Inst::TextEqRange { dest, .. }
                | Inst::TextFindByteRange { dest, .. }
                | Inst::BytesFindByteRange { dest, .. }
                | Inst::TextLineEnd { dest, .. }
                | Inst::TextNextLine { dest, .. }
                | Inst::TextFieldEnd { dest, .. }
                | Inst::TextNextField { dest, .. }
                | Inst::TextConcat { dest, .. }
                | Inst::TextIntern { dest, .. }
                | Inst::TextSlice { dest, .. }
                | Inst::BytesSlice { dest, .. }
                | Inst::TextBuilderNew { dest }
                | Inst::TextIndexNew { dest }
                | Inst::TextBuilderAppend { dest, .. }
                | Inst::TextBuilderAppendCodepoint { dest, .. }
                | Inst::TextBuilderAppendAscii { dest, .. }
                | Inst::TextBuilderAppendSlice { dest, .. }
                | Inst::TextBuilderAppendI32 { dest, .. }
                | Inst::TextBuilderFinish { dest, .. }
                | Inst::StdoutWriteBuilder { dest, .. }
                | Inst::TextIndexGet { dest, .. }
                | Inst::TextIndexContains { dest, .. }
                | Inst::TextIndexGetOrInsert { dest, .. }
                | Inst::TextIndexSet { dest, .. }
                | Inst::TextIndexKeys { dest, .. }
                | Inst::TextFromF64Fixed { dest, .. }
                | Inst::ArgCount { dest, .. }
                | Inst::ArgText { dest, .. }
                | Inst::StdinText { dest }
                | Inst::StdinBytes { dest }
                | Inst::ParseI32 { dest, .. }
                | Inst::ParseI32Range { dest, .. }
                | Inst::ParseF64 { dest, .. }
                | Inst::MakeEnum { dest, .. }
                | Inst::MakeRecord { dest, .. }
                | Inst::Field { dest, .. }
                | Inst::EnumTagEq { dest, .. }
                | Inst::EnumPayload { dest, .. }
                | Inst::EnumToI32 { dest, .. }
                | Inst::EnumToText { dest, .. }
                | Inst::ListNew { dest, .. }
                | Inst::ListLen { dest, .. }
                | Inst::ListGet { dest, .. }
                | Inst::ListSet { dest, .. }
                | Inst::ListPush { dest, .. }
                | Inst::ListSortText { dest, .. }
                | Inst::ListSortRecordTextField { dest, .. }
                | Inst::Add { dest, .. }
                | Inst::Sub { dest, .. }
                | Inst::Mul { dest, .. }
                | Inst::Div { dest, .. }
                | Inst::Rem { dest, .. }
                | Inst::BitAnd { dest, .. }
                | Inst::BitOr { dest, .. }
                | Inst::BitXor { dest, .. }
                | Inst::Shl { dest, .. }
                | Inst::Shr { dest, .. }
                | Inst::Eq { dest, .. }
                | Inst::Ne { dest, .. }
                | Inst::Lt { dest, .. }
                | Inst::Le { dest, .. }
                | Inst::Gt { dest, .. }
                | Inst::Ge { dest, .. }
                | Inst::And { dest, .. }
                | Inst::Or { dest, .. }
                | Inst::F64FromI32 { dest, .. }
                | Inst::Sqrt { dest, .. }
                | Inst::Perform { dest, .. }
                | Inst::Handle { dest, .. } => {
                    locals.insert(*dest, kinds[dest].clone());
                }
                Inst::Call { dest, .. } => {
                    locals.insert(*dest, kinds[dest].clone());
                }
                Inst::If {
                    dest,
                    then_insts,
                    else_insts,
                    ..
                } => {
                    locals.insert(*dest, kinds[dest].clone());
                    locals.extend(self.collect_locals(function, then_insts, kinds)?);
                    locals.extend(self.collect_locals(function, else_insts, kinds)?);
                }
                Inst::While {
                    dest,
                    body_insts,
                    condition_insts,
                    ..
                } => {
                    locals.insert(*dest, kinds[dest].clone());
                    locals.extend(self.collect_locals(function, condition_insts, kinds)?);
                    locals.extend(self.collect_locals(function, body_insts, kinds)?);
                }
                Inst::Repeat {
                    dest, body_insts, ..
                } => {
                    locals.insert(*dest, kinds[dest].clone());
                    locals.extend(self.collect_locals(function, body_insts, kinds)?);
                }
                Inst::StoreLocal { .. }
                | Inst::StdoutWrite { .. }
                | Inst::Assert { .. }
                | Inst::AllocPush
                | Inst::AllocPop
                | Inst::BytesToText { .. }
                | Inst::FileOpen { .. }
                | Inst::FileIsValid { .. }
                | Inst::FileRead { .. }
                | Inst::FileReadToEnd { .. }
                | Inst::FileWrite { .. }
                | Inst::FileClose { .. }
                | Inst::FileSeek { .. }
                | Inst::FileSize { .. }
                | Inst::FileExists { .. }
                | Inst::FileRemove { .. } => {}
            }
        }
        Ok(locals)
    }

    fn emit_inst(
        &self,
        output: &mut String,
        function: &Function,
        inst: &Inst,
        kinds: &BTreeMap<ValueId, WasmValueKind>,
    ) -> Result<(), WasmError> {
        match inst {
            Inst::LoadParam { dest, index } => {
                writeln!(output, "    local.get $p{}", index)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::LoadLocal { dest, slot } => {
                writeln!(output, "    local.get ${}", wasm_slot(*slot))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::StoreLocal { slot, src } => {
                writeln!(output, "    local.get ${}", wasm_id(*src))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_slot(*slot))
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstInt { dest, value } => {
                w_const(output, *dest, &format!("i64.const {}", value));
            }
            Inst::ConstF64 { dest, bits } => {
                w_const(
                    output,
                    *dest,
                    &format!("f64.const {}", f64::from_bits(*bits)),
                );
            }
            Inst::ConstBool { dest, value } => {
                w_const(
                    output,
                    *dest,
                    &format!("i64.const {}", if *value { 1 } else { 0 }),
                );
            }
            Inst::ConstText { dest, value } => {
                let bytes = value.as_bytes();
                writeln!(output, "    i32.const {}", bytes.len())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $alloc").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
                for (index, byte) in bytes.iter().copied().enumerate() {
                    writeln!(output, "    local.get ${}", wasm_id(*dest))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                    writeln!(output, "    i32.const {}", index)
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                    writeln!(output, "    i32.const {}", byte)
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i32.store8").expect("writing to a string cannot fail");
                }
                writeln!(output, "    local.get ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const {}", bytes.len())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_pack_text")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::StdinBytes { dest } => {
                writeln!(output, "    call $__sarif_stdin_bytes")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextLen { dest, text } | Inst::BytesLen { dest, bytes: text } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_len_i32")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::BytesByte { dest, bytes, index } => {
                w_call(output, *dest, &[*bytes, *index], "$__sarif_text_byte");
            }
            Inst::BytesSlice {
                dest,
                bytes,
                start,
                end,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*bytes, *start, *end],
                    "$__sarif_bytes_slice",
                );
            }
            Inst::BytesFindByteRange {
                dest,
                source,
                start,
                end,
                byte,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*source, *start, *end, *byte],
                    "$__sarif_bytes_find_byte_range",
                );
            }
            Inst::TextByte { dest, text, index } => {
                w_call(output, *dest, &[*text, *index], "$__sarif_text_byte");
            }
            Inst::TextConcat { dest, left, right } => {
                w_call(output, *dest, &[*left, *right], "$__sarif_text_concat");
            }
            Inst::TextIntern { dest, text } => {
                w_call(output, *dest, &[*text], "$__sarif_text_intern");
            }
            Inst::TextSlice {
                dest,
                text,
                start,
                end,
            } => {
                w_call(output, *dest, &[*text, *start, *end], "$__sarif_text_slice");
            }
            Inst::TextBuilderNew { dest } => {
                w_call(output, *dest, &[], "$__sarif_text_builder_new");
            }
            Inst::TextIndexNew { dest } => {
                w_call(output, *dest, &[], "$__sarif_text_index_new");
            }
            Inst::TextIndexGet { dest, index, key } => {
                w_call(output, *dest, &[*index, *key], "$__sarif_text_index_get");
            }
            Inst::TextIndexContains { dest, index, key } => {
                w_call(
                    output,
                    *dest,
                    &[*index, *key],
                    "$__sarif_text_index_contains",
                );
            }
            Inst::TextIndexGetOrInsert {
                dest,
                index,
                key,
                next,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*index, *key, *next],
                    "$__sarif_text_index_get_or_insert",
                );
            }
            Inst::TextIndexSet {
                dest,
                index,
                key,
                value,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*index, *key, *value],
                    "$__sarif_text_index_set",
                );
            }
            Inst::TextIndexKeys { dest, index } => {
                w_call(output, *dest, &[*index], "$__sarif_text_index_keys");
            }
            Inst::StdoutWriteBuilder { dest, builder } => {
                w_call(output, *dest, &[*builder], "$__sarif_stdout_write_builder");
            }
            Inst::TextBuilderAppend {
                dest,
                builder,
                text,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*builder, *text],
                    "$__sarif_text_builder_append",
                );
            }
            Inst::TextBuilderAppendCodepoint {
                dest,
                builder,
                codepoint,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*builder, *codepoint],
                    "$__sarif_text_builder_append_codepoint",
                );
            }
            Inst::TextBuilderAppendAscii {
                dest,
                builder,
                byte,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*builder, *byte],
                    "$__sarif_text_builder_append_ascii",
                );
            }
            Inst::TextBuilderAppendSlice {
                dest,
                builder,
                text,
                start,
                end,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*builder, *text, *start, *end],
                    "$__sarif_text_builder_append_slice",
                );
            }
            Inst::TextBuilderAppendI32 {
                dest,
                builder,
                value,
            } => {
                w_call(
                    output,
                    *dest,
                    &[*builder, *value],
                    "$__sarif_text_builder_append_i32",
                );
            }
            Inst::TextBuilderFinish { dest, builder } => {
                w_call(output, *dest, &[*builder], "$__sarif_text_builder_finish");
            }
            Inst::TextFromF64Fixed {
                dest,
                value,
                digits,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*digits))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_from_f64_fixed")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ArgCount { dest } => {
                writeln!(output, "    call $__sarif_arg_count")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ArgText { dest, index } => {
                writeln!(output, "    local.get ${}", wasm_id(*index))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_arg_text")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::StdinText { dest } => {
                writeln!(output, "    call $__sarif_stdin_text")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::AllocPush => {
                writeln!(output, "    call $__sarif_alloc_push")
                    .expect("writing to a string cannot fail");
            }
            Inst::AllocPop => {
                writeln!(output, "    call $__sarif_alloc_pop")
                    .expect("writing to a string cannot fail");
            }
            Inst::StdoutWrite { text } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_stdout_write")
                    .expect("writing to a string cannot fail");
            }
            Inst::ParseI32 { dest, text } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_parse_i32")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ParseF64 { dest, text } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_parse_f64")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::MakeEnum {
                dest,
                name,
                variant,
                payload,
            } => {
                self.emit_make_enum(output, function, *dest, name, variant, *payload)?;
            }
            Inst::MakeRecord { dest, name, fields } => {
                let record = &self.records[name];
                writeln!(output, "    i32.const {}", record.size)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $alloc").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
                let dest_id = wasm_id(*dest);
                for field in &record.fields {
                    let source = fields
                        .iter()
                        .find(|(n, _)| n == &field.name)
                        .map(|(_, s)| s)
                        .expect("field source should be available");
                    writeln!(output, "    local.get ${}", dest_id)
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                    writeln!(output, "    i32.const {}", field.offset)
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", wasm_id(*source))
                        .expect("writing to a string cannot fail");
                    let store_op = match wasm_type_from_kind_result(&field.kind) {
                        Some(WasmType::I64) => "i64.store",
                        Some(WasmType::F64) => "f64.store",
                        None => "i64.store",
                    };
                    writeln!(output, "    {}", store_op).expect("writing to a string cannot fail");
                }
            }
            Inst::Field { dest, base, name } => {
                let WasmValueKind::Record(record_name) = &kinds[base] else {
                    return Err(WasmError::new("expected record kind for field access"));
                };
                let record = &self.records[record_name];
                let field = record
                    .fields
                    .iter()
                    .find(|f| f.name == *name)
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "record `{record_name}` has no field `{name}` in `{}`",
                            function.name
                        ))
                    })?;
                writeln!(output, "    local.get ${}", wasm_id(*base))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const {}", field.offset)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                let load_op = match wasm_type_from_kind_result(&field.kind) {
                    Some(WasmType::I64) => "i64.load",
                    Some(WasmType::F64) => "f64.load",
                    None => "i64.load",
                };
                writeln!(output, "    {}", load_op).expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::EnumTagEq {
                dest, value, tag, ..
            } => {
                let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                    return Err(WasmError::new("expected enum kind for enum tag comparison"));
                };
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                if !enum_is_payload_free(&self.enums[enum_name]) {
                    writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.load").expect("writing to a string cannot fail");
                }
                writeln!(output, "    i64.const {}", tag).expect("writing to a string cannot fail");
                writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::EnumPayload { dest, value, .. } => {
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const 8").expect("writing to a string cannot fail");
                writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                let load_op = match wasm_type_from_kind_result(&kinds[dest]) {
                    Some(WasmType::I64) => "i64.load",
                    Some(WasmType::F64) => "f64.load",
                    None => "i64.load",
                };
                writeln!(output, "    {}", load_op).expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::EnumToI32 {
                dest,
                value,
                discriminants,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                    return Err(WasmError::new("expected enum kind for enum_to_i32"));
                };
                if !enum_is_payload_free(&self.enums[enum_name]) {
                    writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.load").expect("writing to a string cannot fail");
                }
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                let tag_reg = format!("__tag_{}", dest.0);
                writeln!(output, "    local.set ${}", tag_reg)
                    .expect("writing to a string cannot fail");
                let result_reg = format!("__result_{}", dest.0);
                for (i, &disc) in discriminants.iter().enumerate().rev() {
                    writeln!(output, "    local.get ${}", tag_reg)
                        .expect("writing to string cannot fail");
                    writeln!(output, "    i32.const {}", i).expect("writing to string cannot fail");
                    writeln!(output, "    i32.eq").expect("writing to string cannot fail");
                    writeln!(output, "    i32.const {}", disc)
                        .expect("writing to string cannot fail");
                    if i == discriminants.len() - 1 {
                        writeln!(output, "    local.set ${}", result_reg)
                            .expect("writing to string cannot fail");
                    } else {
                        writeln!(output, "    local.get ${}", result_reg)
                            .expect("writing to string cannot fail");
                        writeln!(output, "    select").expect("writing to string cannot fail");
                        writeln!(output, "    local.set ${}", result_reg)
                            .expect("writing to string cannot fail");
                    }
                }
                writeln!(output, "    local.get ${}", result_reg)
                    .expect("writing to string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to string cannot fail");
            }
            Inst::EnumToText {
                dest,
                value,
                variant_names,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                    return Err(WasmError::new("expected enum kind for enum_to_text"));
                };
                if !enum_is_payload_free(&self.enums[enum_name]) {
                    writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.load").expect("writing to a string cannot fail");
                }
                let mut text_ids: Vec<String> = Vec::new();
                for name in variant_names.iter() {
                    let text_dest = format!("__text_{:?}", name);
                    let bytes = name.as_bytes();
                    writeln!(output, "    i32.const {}", bytes.len())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    call $alloc").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.extend_i32_u")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.set ${}", text_dest)
                        .expect("writing to a string cannot fail");
                    for (index, byte) in bytes.iter().copied().enumerate() {
                        writeln!(output, "    local.get ${}", text_dest)
                            .expect("writing to a string cannot fail");
                        writeln!(output, "    i32.wrap_i64")
                            .expect("writing to a string cannot fail");
                        writeln!(output, "    i32.const {}", index)
                            .expect("writing to a string cannot fail");
                        writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                        writeln!(output, "    i32.const {}", byte)
                            .expect("writing to a string cannot fail");
                        writeln!(output, "    i32.store8")
                            .expect("writing to a string cannot fail");
                    }
                    text_ids.push(text_dest);
                }
                for (i, text_id) in text_ids.iter().enumerate().rev() {
                    writeln!(
                        output,
                        "    i64.const {}",
                        i64::try_from(i).expect("variant index fits i64")
                    )
                    .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", wasm_id(*value))
                        .expect("writing to a string cannot fail");
                    let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                        return Err(WasmError::new("expected enum kind for enum_to_text"));
                    };
                    if !enum_is_payload_free(&self.enums[enum_name]) {
                        writeln!(output, "    i32.wrap_i64")
                            .expect("writing to a string cannot fail");
                        writeln!(output, "    i64.load").expect("writing to a string cannot fail");
                    }
                    writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", text_id)
                        .expect("writing to a string cannot fail");
                    if i < text_ids.len() - 1 {
                        writeln!(output, "    local.get ${}", wasm_id(*dest))
                            .expect("writing to a string cannot fail");
                        writeln!(output, "    select").expect("writing to a string cannot fail");
                    }
                }
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ListNew { dest, len, value } => {
                w_call(output, *dest, &[*len, *value], "$__sarif_list_new");
            }
            Inst::ListLen { dest, list } => {
                w_call(output, *dest, &[*list], "$__sarif_list_len");
            }
            Inst::ListGet { dest, list, index } => {
                w_call(output, *dest, &[*list, *index], "$__sarif_list_get");
            }
            Inst::ListSet {
                dest,
                list,
                index,
                value,
            } => {
                w_call(output, *dest, &[*list, *index, *value], "$__sarif_list_set");
            }
            Inst::ListPush {
                dest,
                list,
                len,
                value,
            } => {
                w_call(output, *dest, &[*list, *len, *value], "$__sarif_list_push");
            }
            Inst::ListSortText { dest, list, len } => {
                w_call(output, *dest, &[*list, *len], "$__sarif_list_sort_text");
            }
            Inst::ListSortRecordTextField {
                dest,
                list,
                len,
                field,
            } => {
                let Some(WasmValueKind::List(element)) = kinds.get(list) else {
                    return Err(WasmError::new(format!(
                        "wasm list_sort_record_text_field input {} is not a list in `{}`",
                        list.render(),
                        function.name
                    )));
                };
                let WasmValueKind::Record(record_name) = element.as_ref() else {
                    return Err(WasmError::new(format!(
                        "wasm list_sort_record_text_field requires List[record], found `{:?}` in `{}`",
                        element, function.name
                    )));
                };
                let record = self.records.get(record_name.as_str()).ok_or_else(|| {
                    WasmError::new(format!("missing wasm record metadata for `{record_name}`"))
                })?;
                let field_desc = record
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "record `{record_name}` has no wasm field `{field}` in `{}`",
                            function.name
                        ))
                    })?;
                if field_desc.kind != WasmValueKind::Text {
                    return Err(WasmError::new(format!(
                        "wasm list_sort_record_text_field requires a Text field, but `{record_name}.{field}` is `{:?}` in `{}`",
                        field_desc.kind, function.name
                    )));
                }
                let offset = i64::from(field_desc.offset);
                writeln!(output, "    local.get ${}", wasm_id(*list))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*len))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const {offset}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_list_sort_record_text_field")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::Add { dest, left, right } => {
                self.emit_binary(output, "add", *dest, *left, *right, kinds)?;
            }
            Inst::Sub { dest, left, right } => {
                self.emit_binary(output, "sub", *dest, *left, *right, kinds)?;
            }
            Inst::Mul { dest, left, right } => {
                self.emit_binary(output, "mul", *dest, *left, *right, kinds)?;
            }
            Inst::Div { dest, left, right } => {
                self.emit_binary(output, "div", *dest, *left, *right, kinds)?;
            }
            Inst::Rem { dest, left, right } => {
                self.emit_binary(output, "rem", *dest, *left, *right, kinds)?;
            }
            Inst::BitAnd { dest, left, right } => {
                self.emit_binary(output, "and", *dest, *left, *right, kinds)?;
            }
            Inst::BitOr { dest, left, right } => {
                self.emit_binary(output, "or", *dest, *left, *right, kinds)?;
            }
            Inst::BitXor { dest, left, right } => {
                self.emit_binary(output, "xor", *dest, *left, *right, kinds)?;
            }
            Inst::Shl { dest, left, right } => {
                self.emit_binary(output, "shl", *dest, *left, *right, kinds)?;
            }
            Inst::Shr { dest, left, right } => {
                self.emit_binary(output, "shr_s", *dest, *left, *right, kinds)?;
            }
            Inst::Eq { dest, left, right } => {
                self.emit_comparison(output, "eq", *dest, *left, *right, kinds)?;
            }
            Inst::Ne { dest, left, right } => {
                self.emit_comparison(output, "ne", *dest, *left, *right, kinds)?;
            }
            Inst::Lt { dest, left, right } => {
                self.emit_comparison(output, "lt", *dest, *left, *right, kinds)?;
            }
            Inst::Le { dest, left, right } => {
                self.emit_comparison(output, "le", *dest, *left, *right, kinds)?;
            }
            Inst::Gt { dest, left, right } => {
                self.emit_comparison(output, "gt", *dest, *left, *right, kinds)?;
            }
            Inst::Ge { dest, left, right } => {
                self.emit_comparison(output, "ge", *dest, *left, *right, kinds)?;
            }
            Inst::And { dest, left, right } => {
                self.emit_binary(output, "and", *dest, *left, *right, kinds)?;
            }
            Inst::Or { dest, left, right } => {
                self.emit_binary(output, "or", *dest, *left, *right, kinds)?;
            }
            Inst::F64FromI32 { dest, value } => {
                w_unary(output, *dest, *value, "f64.convert_i64_s");
            }
            Inst::Sqrt { dest, value } => {
                w_unary(output, *dest, *value, "f64.sqrt");
            }
            Inst::Call { dest, callee, args } => {
                for arg in args {
                    writeln!(output, "    local.get ${}", wasm_id(*arg))
                        .expect("writing to a string cannot fail");
                }
                writeln!(output, "    call ${}", callee).expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::If {
                condition,
                then_insts,
                else_insts,
                then_result,
                else_result,
                dest,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*condition))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                write!(output, "    if").expect("writing to a string cannot fail");
                let result_type = wasm_type_from_kind_result(&kinds[dest]);
                if let Some(ty) = result_type {
                    write!(output, " (result {})", ty.render())
                        .expect("writing to a string cannot fail");
                }
                writeln!(output).expect("writing to a string cannot fail");
                for inst in then_insts {
                    self.emit_inst(output, function, inst, kinds)?;
                }
                if let Some(res) = then_result {
                    writeln!(output, "    local.get ${}", wasm_id(*res))
                        .expect("writing to a string cannot fail");
                } else if result_type.is_some() {
                    writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                }
                writeln!(output, "    else").expect("writing to a string cannot fail");
                for inst in else_insts {
                    self.emit_inst(output, function, inst, kinds)?;
                }
                if let Some(res) = else_result {
                    writeln!(output, "    local.get ${}", wasm_id(*res))
                        .expect("writing to a string cannot fail");
                } else if result_type.is_some() {
                    writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                }
                writeln!(output, "    end").expect("writing to a string cannot fail");
                if result_type.is_some() {
                    writeln!(output, "    local.set ${}", wasm_id(*dest))
                        .expect("writing to a string cannot fail");
                }
            }
            Inst::While {
                condition_insts,
                condition,
                body_insts,
                ..
            } => {
                writeln!(output, "    block").expect("writing to a string cannot fail");
                writeln!(output, "    loop").expect("writing to a string cannot fail");
                for inst in condition_insts {
                    self.emit_inst(output, function, inst, kinds)?;
                }
                writeln!(output, "    local.get ${}", wasm_id(*condition))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.eqz").expect("writing to a string cannot fail");
                writeln!(output, "    br_if 1").expect("writing to a string cannot fail");
                for inst in body_insts {
                    self.emit_inst(output, function, inst, kinds)?;
                }
                writeln!(output, "    br 0").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
            }
            Inst::Repeat {
                count,
                body_insts,
                index_slot,
                ..
            } => {
                let count_id = wasm_id(*count);
                if let Some(slot) = index_slot {
                    writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                    writeln!(output, "    local.set ${}", wasm_slot(*slot))
                        .expect("writing to a string cannot fail");
                }
                writeln!(output, "    block").expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                writeln!(output, "    local.set $repeat_counter_{}", count_id)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    loop").expect("writing to a string cannot fail");
                writeln!(output, "    local.get $repeat_counter_{}", count_id)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", count_id)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.ge_s").expect("writing to a string cannot fail");
                writeln!(output, "    br_if 1").expect("writing to a string cannot fail");
                for inst in body_insts {
                    self.emit_inst(output, function, inst, kinds)?;
                }
                writeln!(output, "    local.get $repeat_counter_{}", count_id)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
                writeln!(output, "    i64.add").expect("writing to a string cannot fail");
                writeln!(output, "    local.tee $repeat_counter_{}", count_id)
                    .expect("writing to a string cannot fail");
                if let Some(slot) = index_slot {
                    writeln!(output, "    local.set ${}", wasm_slot(*slot))
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    drop").expect("writing to a string cannot fail");
                }
                writeln!(output, "    br 0").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
            }
            Inst::Assert { condition, .. } => {
                writeln!(output, "    local.get ${}", wasm_id(*condition))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.eqz").expect("writing to a string cannot fail");
                writeln!(output, "    if").expect("writing to a string cannot fail");
                writeln!(output, "      unreachable").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
            }
            Inst::TextCmp { dest, left, right } => {
                writeln!(output, "    local.get ${}", wasm_id(*left))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*right))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_cmp")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextEqRange {
                dest,
                source,
                start,
                end,
                expected,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*source))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*end))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*expected))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_eq_range")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextFindByteRange {
                dest,
                source,
                start,
                end,
                byte,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*source))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*end))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*byte))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_find_byte_range")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextLineEnd {
                dest,
                source,
                start,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*source))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_line_end")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextNextLine {
                dest,
                source,
                start,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*source))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_next_line")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextFieldEnd {
                dest,
                source,
                start,
                end,
                byte,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*source))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*end))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*byte))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_field_end")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextNextField {
                dest,
                source,
                start,
                end,
                byte,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*source))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*end))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*byte))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_next_field")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ParseI32Range {
                dest,
                text,
                start,
                end,
            } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*start))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*end))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_parse_i32_range")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::Perform { .. } | Inst::Handle { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support effect handlers",
                ));
            }
            Inst::BytesToText { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not support bytes-to-text conversion",
                ));
            }
            Inst::FileOpen { .. } => {
                return Err(WasmError::new("wasm backend does not support file open"));
            }
            Inst::FileIsValid { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not support file validity check",
                ));
            }
            Inst::FileRead { .. } => {
                return Err(WasmError::new("wasm backend does not support file read"));
            }
            Inst::FileReadToEnd { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not support file read to end",
                ));
            }
            Inst::FileWrite { .. } => {
                return Err(WasmError::new("wasm backend does not support file write"));
            }
            Inst::FileClose { .. } => {
                return Err(WasmError::new("wasm backend does not support file close"));
            }
            Inst::FileSeek { .. } => {
                return Err(WasmError::new("wasm backend does not support file seek"));
            }
            Inst::FileSize { .. } => {
                return Err(WasmError::new("wasm backend does not support file size"));
            }
            Inst::FileExists { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not support file exists check",
                ));
            }
            Inst::FileRemove { .. } => {
                return Err(WasmError::new("wasm backend does not support file remove"));
            }
        }
        Ok(())
    }

    fn emit_binary(
        &self,
        output: &mut String,
        op: &str,
        dest: ValueId,
        left: ValueId,
        right: ValueId,
        kinds: &BTreeMap<ValueId, WasmValueKind>,
    ) -> Result<(), WasmError> {
        let kind = &kinds[&left];
        let wasm_type = wasm_type_from_kind(kind);
        writeln!(output, "    local.get ${}", wasm_id(left))
            .expect("writing to a string cannot fail");
        writeln!(output, "    local.get ${}", wasm_id(right))
            .expect("writing to a string cannot fail");
        let full_op = if op == "and" || op == "or" {
            format!("i64.{}", op)
        } else if (op == "div" || op == "rem") && matches!(wasm_type, WasmType::I64) {
            format!("i64.{}_s", op)
        } else {
            format!("{}.{}", wasm_type.render(), op)
        };
        writeln!(output, "    {}", full_op).expect("writing to a string cannot fail");
        writeln!(output, "    local.set ${}", wasm_id(dest))
            .expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_comparison(
        &self,
        output: &mut String,
        op: &str,
        dest: ValueId,
        left: ValueId,
        right: ValueId,
        kinds: &BTreeMap<ValueId, WasmValueKind>,
    ) -> Result<(), WasmError> {
        let kind = &kinds[&left];
        if op == "eq" || op == "ne" {
            match kind {
                WasmValueKind::Text => {
                    writeln!(output, "    local.get ${}", wasm_id(left))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", wasm_id(right))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    call $__sarif_text_eq")
                        .expect("writing to a string cannot fail");
                }
                WasmValueKind::Record(name) => {
                    writeln!(output, "    local.get ${}", wasm_id(left))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", wasm_id(right))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    call {}", record_eq_helper_name(name))
                        .expect("writing to a string cannot fail");
                }
                WasmValueKind::Enum(name) if !enum_is_payload_free(&self.enums[name]) => {
                    writeln!(output, "    local.get ${}", wasm_id(left))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", wasm_id(right))
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    call {}", enum_eq_helper_name(name))
                        .expect("writing to a string cannot fail");
                }
                _ => {}
            }
            let uses_structural_helper = matches!(
                kind,
                WasmValueKind::Text | WasmValueKind::Record(_)
            ) || matches!(kind, WasmValueKind::Enum(name) if !enum_is_payload_free(&self.enums[name]));
            if uses_structural_helper {
                if op == "ne" {
                    writeln!(output, "    i64.eqz").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.extend_i32_u")
                        .expect("writing to a string cannot fail");
                }
                writeln!(output, "    local.set ${}", wasm_id(dest))
                    .expect("writing to a string cannot fail");
                return Ok(());
            }
        }
        let wasm_type = wasm_type_from_kind(kind);
        writeln!(output, "    local.get ${}", wasm_id(left))
            .expect("writing to a string cannot fail");
        writeln!(output, "    local.get ${}", wasm_id(right))
            .expect("writing to a string cannot fail");
        match wasm_type {
            WasmType::I64 => {
                let suffix = if op == "eq" || op == "ne" { "" } else { "_s" };
                writeln!(output, "    i64.{}{}", op, suffix)
                    .expect("writing to a string cannot fail");
            }
            WasmType::F64 => {
                writeln!(output, "    f64.{}", op).expect("writing to a string cannot fail");
            }
        }
        writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
        writeln!(output, "    local.set ${}", wasm_id(dest))
            .expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_make_enum(
        &self,
        output: &mut String,
        _function: &Function,
        dest: ValueId,
        name: &str,
        variant: &str,
        payload: Option<ValueId>,
    ) -> Result<(), WasmError> {
        let enum_ty = self
            .enums
            .get(name)
            .ok_or_else(|| WasmError::new(format!("unknown enum `{name}`")))?;
        let variant_index = enum_ty
            .variants
            .iter()
            .position(|v| v.name == variant)
            .expect("variant should exist");

        if enum_is_payload_free(enum_ty) {
            writeln!(output, "    i64.const {}", variant_index)
                .expect("writing to a string cannot fail");
            writeln!(output, "    local.set ${}", wasm_id(dest))
                .expect("writing to a string cannot fail");
            return Ok(());
        }

        writeln!(output, "    i32.const {}", PAYLOAD_ENUM_SIZE)
            .expect("writing to a string cannot fail");
        writeln!(output, "    call $alloc").expect("writing to a string cannot fail");
        writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
        writeln!(output, "    local.set ${}", wasm_id(dest))
            .expect("writing to a string cannot fail");
        let dest_id = wasm_id(dest);

        writeln!(output, "    local.get ${}", dest_id).expect("writing to a string cannot fail");
        writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
        writeln!(output, "    i64.const {}", variant_index)
            .expect("writing to a string cannot fail");
        writeln!(output, "    i64.store").expect("writing to a string cannot fail");

        if let Some(source) = payload {
            writeln!(output, "    local.get ${}", dest_id)
                .expect("writing to a string cannot fail");
            writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
            writeln!(output, "    i32.const 8").expect("writing to a string cannot fail");
            writeln!(output, "    i32.add").expect("writing to a string cannot fail");
            writeln!(output, "    local.get ${}", wasm_id(source))
                .expect("writing to a string cannot fail");
            let payload_kind = enum_ty
                .variants
                .get(variant_index)
                .and_then(|variant| variant.payload.as_ref())
                .ok_or_else(|| {
                    WasmError::new(format!(
                        "enum `{name}` variant `{variant}` is missing payload metadata"
                    ))
                })?;
            let store_op = match wasm_type_from_kind_result(payload_kind) {
                Some(WasmType::I64) => "i64.store",
                Some(WasmType::F64) => "f64.store",
                None => "i64.store",
            };
            writeln!(output, "    {}", store_op).expect("writing to a string cannot fail");
        }

        Ok(())
    }
}

fn wasm_id(id: ValueId) -> String {
    id.render().replace('%', "")
}

fn wasm_slot(id: LocalSlotId) -> String {
    id.render().replace('#', "")
}

fn w_unary(out: &mut String, dest: ValueId, src: ValueId, op: &str) {
    writeln!(out, "    local.get ${}", wasm_id(src)).expect("writing to a string cannot fail");
    writeln!(out, "    {op}").expect("writing to a string cannot fail");
    writeln!(out, "    local.set ${}", wasm_id(dest)).expect("writing to a string cannot fail");
}

fn w_const(out: &mut String, dest: ValueId, op: &str) {
    writeln!(out, "    {op}").expect("writing to a string cannot fail");
    writeln!(out, "    local.set ${}", wasm_id(dest)).expect("writing to a string cannot fail");
}

fn w_call(out: &mut String, dest: ValueId, args: &[ValueId], func: &str) {
    for arg in args {
        writeln!(out, "    local.get ${}", wasm_id(*arg)).expect("writing to a string cannot fail");
    }
    writeln!(out, "    call {func}").expect("writing to a string cannot fail");
    writeln!(out, "    local.set ${}", wasm_id(dest)).expect("writing to a string cannot fail");
}

fn wasm_helper_suffix(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn record_eq_helper_name(name: &str) -> String {
    format!("$eq_record_{}", wasm_helper_suffix(name))
}

fn enum_eq_helper_name(name: &str) -> String {
    format!("$eq_enum_{}", wasm_helper_suffix(name))
}

fn wasm_value_kind_from_name(
    name: &str,
    structs: &[super::StructType],
    enums: &[super::EnumType],
) -> Result<WasmValueKind, WasmError> {
    match name {
        "I32" => Ok(WasmValueKind::I32),
        "F64" => Ok(WasmValueKind::F64),
        "Bool" => Ok(WasmValueKind::Bool),
        "Text" => Ok(WasmValueKind::Text),
        "Bytes" => Ok(WasmValueKind::Bytes),
        "TextBuilder" => Ok(WasmValueKind::TextBuilder),
        "TextIndex" => Ok(WasmValueKind::TextIndex),
        "Unit" => Ok(WasmValueKind::Unit),
        other => {
            if enums.iter().any(|e| e.name == other) {
                Ok(WasmValueKind::Enum(other.to_owned()))
            } else if structs.iter().any(|s| s.name == other) {
                Ok(WasmValueKind::Record(other.to_owned()))
            } else if let Some(element) = other
                .strip_prefix("List[")
                .and_then(|s| s.strip_suffix(']'))
            {
                let element_kind = wasm_value_kind_from_name(element, structs, enums)?;
                Ok(WasmValueKind::List(Box::new(element_kind)))
            } else if other == "List" {
                Ok(WasmValueKind::List(Box::new(WasmValueKind::F64)))
            } else {
                Err(WasmError::new(format!(
                    "unknown type `{other}` in Wasm codegen"
                )))
            }
        }
    }
}

fn wasm_type_from_kind(kind: &WasmValueKind) -> WasmType {
    match kind {
        WasmValueKind::F64 => WasmType::F64,
        _ => WasmType::I64,
    }
}

fn wasm_type_from_kind_result(kind: &WasmValueKind) -> Option<WasmType> {
    match kind {
        WasmValueKind::F64 => Some(WasmType::F64),
        WasmValueKind::Unit => None,
        _ => Some(WasmType::I64),
    }
}

fn collect_inst_kinds(
    function: &Function,
    instructions: &[Inst],
    structs: &[super::StructType],
    enums: &[super::EnumType],
    all_functions: &[Function],
    kinds: &mut BTreeMap<ValueId, WasmValueKind>,
) -> Result<(), WasmError> {
    for inst in instructions {
        match inst {
            Inst::LoadParam { dest, index } => {
                let ty = &function.params[*index].ty;
                kinds.insert(*dest, wasm_value_kind_from_name(ty, structs, enums)?);
            }
            Inst::LoadLocal { dest, slot } => {
                let ty = function
                    .mutable_local_type(*slot)
                    .expect("mutable local type should be available");
                kinds.insert(*dest, wasm_value_kind_from_name(ty, structs, enums)?);
            }
            Inst::ConstInt { dest, .. }
            | Inst::TextLen { dest, .. }
            | Inst::BytesLen { dest, .. }
            | Inst::TextByte { dest, .. }
            | Inst::BytesByte { dest, .. }
            | Inst::TextCmp { dest, .. }
            | Inst::TextEqRange { dest, .. }
            | Inst::TextFindByteRange { dest, .. }
            | Inst::BytesFindByteRange { dest, .. }
            | Inst::TextLineEnd { dest, .. }
            | Inst::TextNextLine { dest, .. }
            | Inst::TextFieldEnd { dest, .. }
            | Inst::TextNextField { dest, .. }
            | Inst::ArgCount { dest, .. }
            | Inst::ListLen { dest, .. }
            | Inst::ParseI32 { dest, .. }
            | Inst::ParseI32Range { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::ConstF64 { dest, .. }
            | Inst::ParseF64 { dest, .. }
            | Inst::F64FromI32 { dest, .. }
            | Inst::Sqrt { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::F64);
            }
            Inst::ListGet { dest, list, .. } => {
                // Infer element kind from the list's type
                let Some(WasmValueKind::List(element)) = kinds.get(list).cloned() else {
                    return Err(WasmError::new(format!(
                        "wasm list_get input {} is not a list in `{}`",
                        list.render(),
                        function.name
                    )));
                };
                kinds.insert(*dest, *element);
            }
            Inst::ConstBool { dest, .. } | Inst::EnumTagEq { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bool);
            }
            Inst::ConstText { dest, .. }
            | Inst::TextConcat { dest, .. }
            | Inst::TextIntern { dest, .. }
            | Inst::TextSlice { dest, .. }
            | Inst::TextFromF64Fixed { dest, .. }
            | Inst::ArgText { dest, .. }
            | Inst::StdinText { dest } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::BytesSlice { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bytes);
            }
            Inst::StdinBytes { dest } => {
                kinds.insert(*dest, WasmValueKind::Bytes);
            }
            Inst::TextBuilderNew { dest }
            | Inst::TextBuilderAppend { dest, .. }
            | Inst::TextBuilderAppendCodepoint { dest, .. }
            | Inst::TextBuilderAppendAscii { dest, .. }
            | Inst::TextBuilderAppendSlice { dest, .. }
            | Inst::TextBuilderAppendI32 { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::TextBuilder);
            }
            Inst::TextBuilderFinish { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::TextIndexNew { dest } | Inst::TextIndexSet { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::TextIndex);
            }
            Inst::TextIndexKeys { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::TextIndexGet { dest, .. }
            | Inst::TextIndexContains { dest, .. }
            | Inst::TextIndexGetOrInsert { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::StdoutWriteBuilder { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::TextBuilder);
            }
            Inst::ListNew { dest, value, .. } => {
                // Infer element kind from the value being used to fill the list
                let Some(kind) = kinds.get(value).cloned() else {
                    return Err(WasmError::new(format!(
                        "wasm list_new input {} has unknown kind in `{}`",
                        value.render(),
                        function.name
                    )));
                };
                kinds.insert(*dest, WasmValueKind::List(Box::new(kind)));
            }
            Inst::ListSet { dest, list, .. }
            | Inst::ListPush { dest, list, .. }
            | Inst::ListSortText { dest, list, .. }
            | Inst::ListSortRecordTextField { dest, list, .. } => {
                // ListSet returns the same type as the list
                let Some(kind) = kinds.get(list).cloned() else {
                    return Err(WasmError::new(format!(
                        "wasm list mutation input {} has unknown kind in `{}`",
                        list.render(),
                        function.name
                    )));
                };
                kinds.insert(*dest, kind);
            }
            Inst::Perform { dest, .. } | Inst::Handle { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Unit);
            }
            Inst::StdoutWrite { .. } | Inst::AllocPush | Inst::AllocPop => {}
            Inst::MakeEnum { dest, name, .. } => {
                kinds.insert(*dest, WasmValueKind::Enum(name.clone()));
            }
            Inst::MakeRecord { dest, name, .. } => {
                kinds.insert(*dest, WasmValueKind::Record(name.clone()));
            }
            Inst::Field { dest, base, name } => {
                let WasmValueKind::Record(record_name) = kinds[base].clone() else {
                    return Err(WasmError::new("expected record kind for field access"));
                };
                let struct_ty =
                    structs
                        .iter()
                        .find(|s| s.name == record_name)
                        .ok_or_else(|| {
                            WasmError::new(format!(
                                "unknown record `{record_name}` for field `{name}` in `{}`",
                                function.name
                            ))
                        })?;
                let field = struct_ty
                    .fields
                    .iter()
                    .find(|f| f.name == *name)
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "record `{record_name}` has no field `{name}` in `{}`",
                            function.name
                        ))
                    })?;
                kinds.insert(*dest, wasm_value_kind_from_name(&field.ty, structs, enums)?);
            }
            Inst::EnumPayload {
                dest, payload_type, ..
            } => {
                kinds.insert(
                    *dest,
                    wasm_value_kind_from_name(payload_type, structs, enums)?,
                );
            }
            Inst::EnumToI32 { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::EnumToText { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::Add { dest, left, .. }
            | Inst::Sub { dest, left, .. }
            | Inst::Mul { dest, left, .. }
            | Inst::Div { dest, left, .. }
            | Inst::Rem { dest, left, .. } => {
                kinds.insert(*dest, kinds[left].clone());
            }
            Inst::BitAnd { dest, .. }
            | Inst::BitOr { dest, .. }
            | Inst::BitXor { dest, .. }
            | Inst::Shl { dest, .. }
            | Inst::Shr { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::Eq { dest, .. }
            | Inst::Ne { dest, .. }
            | Inst::Lt { dest, .. }
            | Inst::Le { dest, .. }
            | Inst::Gt { dest, .. }
            | Inst::Ge { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bool);
            }
            Inst::And { dest, .. } | Inst::Or { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bool);
            }
            Inst::Call { dest, callee, .. } => {
                let callee_fn = all_functions
                    .iter()
                    .find(|f| f.name == *callee)
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "unknown function `{callee}` in `{}`",
                            function.name
                        ))
                    })?;
                let kind = if let Some(ty) = &callee_fn.return_type {
                    wasm_value_kind_from_name(ty, structs, enums)?
                } else {
                    WasmValueKind::Unit
                };
                kinds.insert(*dest, kind);
            }
            Inst::If {
                dest,
                then_insts,
                else_insts,
                then_result,
                else_result,
                ..
            } => {
                collect_inst_kinds(function, then_insts, structs, enums, all_functions, kinds)?;
                collect_inst_kinds(function, else_insts, structs, enums, all_functions, kinds)?;
                let kind = if let Some(res) = then_result {
                    kinds[res].clone()
                } else if let Some(res) = else_result {
                    kinds[res].clone()
                } else {
                    WasmValueKind::Unit
                };
                kinds.insert(*dest, kind);
            }
            Inst::While {
                dest,
                condition_insts,
                body_insts,
                ..
            } => {
                collect_inst_kinds(
                    function,
                    condition_insts,
                    structs,
                    enums,
                    all_functions,
                    kinds,
                )?;
                collect_inst_kinds(function, body_insts, structs, enums, all_functions, kinds)?;
                kinds.insert(*dest, WasmValueKind::Unit);
            }
            Inst::Repeat {
                dest, body_insts, ..
            } => {
                collect_inst_kinds(function, body_insts, structs, enums, all_functions, kinds)?;
                kinds.insert(*dest, WasmValueKind::Unit);
            }
            Inst::StoreLocal { .. }
            | Inst::Assert { .. }
            | Inst::BytesToText { .. }
            | Inst::FileOpen { .. }
            | Inst::FileIsValid { .. }
            | Inst::FileRead { .. }
            | Inst::FileReadToEnd { .. }
            | Inst::FileWrite { .. }
            | Inst::FileClose { .. }
            | Inst::FileSeek { .. }
            | Inst::FileSize { .. }
            | Inst::FileExists { .. }
            | Inst::FileRemove { .. } => {}
        }
    }
    Ok(())
}
