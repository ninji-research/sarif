use std::collections::BTreeMap;
use std::fmt::Write;

use crate::{CodegenValueKind, ContractKind, Function, Inst, Program, ValueId};

fn record_field_offset(
    structs: &[crate::StructType],
    record_name: &str,
    field_name: &str,
) -> Option<u32> {
    let record = structs.iter().find(|s| s.name == record_name)?;
    let idx = record.fields.iter().position(|f| f.name == field_name)?;
    Some(u32::try_from(idx).unwrap_or(0).checked_mul(8).unwrap_or(0))
}

fn enum_variant_tag(enums: &[crate::EnumType], enum_name: &str, variant_name: &str) -> Option<u64> {
    let enum_ty = enums.iter().find(|e| e.name == enum_name)?;
    let tag = enum_ty
        .variants
        .iter()
        .position(|v| v.name == variant_name)?;
    Some(tag as u64)
}

pub fn emit_c(program: &Program) -> Result<String, String> {
    let mut out = Output::new();
    writeln!(out.buf, "#include <stdint.h>").map_err(to_string)?;
    writeln!(out.buf, "#include <stdlib.h>").map_err(to_string)?;
    writeln!(out.buf, "#include <string.h>").map_err(to_string)?;
    writeln!(out.buf, "#include <math.h>").map_err(to_string)?;
    writeln!(out.buf).map_err(to_string)?;

    out.line("typedef int64_t (*sarif_effect_handler_t)(uint64_t* args, int32_t nargs);")?;
    out.line("struct SarifEffectHandler {")?;
    out.line("    const char* effect;")?;
    out.line("    const char* operation;")?;
    out.line("    sarif_effect_handler_t handler;")?;
    out.line("};")?;
    out.line("")?;

    out.line("extern void* sarif_record_alloc(uint64_t size);")?;
    out.line(
        "extern void* sarif_text_concat(const unsigned char* left, const unsigned char* right);",
    )?;
    out.line("extern uint64_t sarif_text_len(const unsigned char* text);")?;
    out.line(
        "extern int64_t sarif_text_cmp(const unsigned char* left, const unsigned char* right);",
    )?;
    out.line(
        "extern uint64_t sarif_text_eq(const unsigned char* left, const unsigned char* right);",
    )?;
    out.line(
        "extern void* sarif_text_slice(const unsigned char* text, uint64_t start, uint64_t end);",
    )?;
    out.line(
        "extern void* sarif_bytes_slice(const unsigned char* bytes, uint64_t start, uint64_t end);",
    )?;
    out.line("extern uint64_t sarif_bytes_len(const unsigned char* bytes);")?;
    out.line("extern int64_t sarif_bytes_byte(const unsigned char* bytes, uint64_t index);")?;
    out.line("extern void* sarif_bytes_materialize(const unsigned char* bytes);")?;
    out.line("extern int64_t sarif_parse_i32(const unsigned char* text);")?;
    out.line("extern void* sarif_text_from_f64_fixed(double value, int64_t digits);")?;
    out.line("extern void sarif_stdout_write(const unsigned char* text);")?;
    out.line("extern int64_t sarif_perform_effect(")?;
    out.line("    const char* effect, const char* operation,")?;
    out.line("    uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3,")?;
    out.line("    int32_t nargs);")?;
    out.line("extern uint64_t sarif_arg_count(void);")?;
    out.line("extern void* sarif_arg_text(int64_t index);")?;
    out.line("extern void* sarif_stdin_text(void);")?;
    out.line("extern void* sarif_list_new(int64_t len, uint64_t fill);")?;
    out.line("extern void* sarif_list_push(void* list, int64_t len, uint64_t value);")?;
    out.line("extern uint64_t sarif_list_get(void* list, int64_t index);")?;
    out.line("extern void* sarif_list_set(void* list, int64_t index, uint64_t value);")?;
    out.line("extern int64_t sarif_list_len(void* list);")?;
    out.line("extern uint64_t sarif_text_eq_range(const unsigned char* source, int64_t start, int64_t end, const unsigned char* expected);")?;
    out.line("extern int64_t sarif_text_find_byte_range(const unsigned char* source, int64_t start, int64_t end, int64_t byte);")?;
    out.line("extern int64_t sarif_text_line_end(const unsigned char* source, int64_t start);")?;
    out.line("extern int64_t sarif_text_next_line(const unsigned char* source, int64_t start);")?;
    out.line("extern int64_t sarif_text_next_field(const unsigned char* source, int64_t start, int64_t end, int64_t byte);")?;
    out.line(
        "extern uint64_t sarif_file_open(const unsigned char* path, const unsigned char* mode);",
    )?;
    out.line("extern void sarif_file_close(uint64_t handle);")?;
    out.line("extern uint64_t sarif_file_read(uint64_t handle, int64_t len);")?;
    out.line("extern uint64_t sarif_file_read_to_end(uint64_t handle);")?;
    out.line("extern int64_t sarif_file_write(uint64_t handle, const unsigned char* data);")?;
    out.line("extern int64_t sarif_file_seek(uint64_t handle, int64_t offset, int64_t whence);")?;
    out.line("extern int64_t sarif_file_size(uint64_t handle);")?;
    out.line("extern int64_t sarif_file_exists(const unsigned char* path);")?;
    out.line("extern int64_t sarif_file_remove(const unsigned char* path);")?;
    out.line("extern int64_t sarif_file_is_valid(uint64_t handle);")?;
    out.line("extern uint64_t sarif_file_mmap(const unsigned char* path);")?;
    out.line("extern void* sarif_bytes_to_text(const unsigned char* bytes);")?;
    out.line("extern void* sarif_list_sort_text(void* list, int64_t len);")?;
    out.line(
        "extern void* sarif_list_sort_by_text_field(void* list, int64_t len, int64_t offset);",
    )?;
    out.line("extern void* sarif_text_builder_new(void);")?;
    out.line("extern void* sarif_text_builder_append(void* builder, const unsigned char* text);")?;
    out.line(
        "extern void* sarif_text_builder_append_codepoint(void* builder, int64_t codepoint);",
    )?;
    out.line("extern void* sarif_text_builder_append_ascii(void* builder, int64_t byte);")?;
    out.line("extern void* sarif_text_builder_append_slice(void* builder, const unsigned char* text, int64_t start, int64_t end);")?;
    out.line("extern void* sarif_text_builder_append_i32(void* builder, int64_t value);")?;
    out.line("extern void* sarif_text_builder_finish(void* builder);")?;
    out.line("extern void* sarif_stdout_write_builder(void* builder);")?;
    out.line("extern void* sarif_text_index_new(void);")?;
    out.line("extern void* sarif_text_intern(const unsigned char* text);")?;
    out.line("extern void* sarif_text_index_set(void* index, uint64_t key, int64_t value);")?;
    out.line("extern int64_t sarif_text_index_get(void* index, uint64_t key);")?;
    out.line("extern int sarif_text_index_contains(void* index, uint64_t key);")?;
    out.line(
        "extern int64_t sarif_text_index_get_or_insert(void* index, uint64_t key, int64_t next);",
    )?;
    out.line("extern void* sarif_text_index_keys(void* index);")?;
    out.line("extern double sarif_parse_f64(const unsigned char* text);")?;
    out.line("extern int64_t sarif_parse_i32_range(const unsigned char* text, int64_t start, int64_t end);")?;
    out.line("extern void sarif_alloc_push(void);")?;
    out.line("extern void sarif_alloc_pop(void);")?;
    out.line("extern double sarif_f64_from_i32(int64_t value);")?;
    out.line("")?;

    out.line(
        "static inline uint64_t sarif_load_u64(const unsigned char* base, uint64_t offset) {",
    )?;
    out.indent += 1;
    out.line("uint64_t value;")?;
    out.line("memcpy(&value, base + offset, sizeof(uint64_t));")?;
    out.line("return value;")?;
    out.indent -= 1;
    out.line("}")?;
    out.line("static inline void sarif_store_u64(unsigned char* base, uint64_t offset, uint64_t value) {")?;
    out.indent += 1;
    out.line("memcpy(base + offset, &value, sizeof(uint64_t));")?;
    out.indent -= 1;
    out.line("}")?;
    out.line("static inline double sarif_load_f64(const unsigned char* base, uint64_t offset) {")?;
    out.indent += 1;
    out.line("double value;")?;
    out.line("memcpy(&value, base + offset, sizeof(double));")?;
    out.line("return value;")?;
    out.indent -= 1;
    out.line("}")?;
    out.line("static inline void sarif_store_f64(unsigned char* base, uint64_t offset, double value) {")?;
    out.indent += 1;
    out.line("memcpy(base + offset, &value, sizeof(double));")?;
    out.indent -= 1;
    out.line("}")?;
    out.line("")?;

    // User extern declarations
    for extern_fn in &program.externs {
        let ret_type = func_type_name(extern_fn.return_type.as_deref());
        let mut sig = format!("extern {} {}", ret_type, extern_fn.name);
        write!(sig, "(").map_err(to_string)?;
        for (i, param) in extern_fn.params.iter().enumerate() {
            if i > 0 {
                write!(sig, ", ").map_err(to_string)?;
            }
            let tn = func_type_name(Some(&param.ty));
            write!(sig, "{} p{}", tn, i).map_err(to_string)?;
        }
        if extern_fn.params.is_empty() {
            sig.push_str("void");
        }
        writeln!(sig, ");").map_err(to_string)?;
        out.line(&sig)?;
    }
    out.line("")?;

    // Forward declarations
    for func in &program.functions {
        if func.name == "main" {
            continue;
        }
        emit_function_forward_decl(func, &mut out)?;
    }

    for func in &program.functions {
        if func.name == "main" {
            continue;
        }
        let value_kinds = infer_value_kinds_for_func(func, program)?;
        emit_function(
            func,
            &value_kinds,
            &program.structs,
            &program.enums,
            &mut out,
        )?;
    }

    let main_func = program.functions.iter().find(|f| f.name == "main");
    if let Some(main) = main_func {
        let value_kinds = infer_value_kinds_for_func(main, program)?;
        emit_main_wrapper(
            main,
            &value_kinds,
            &program.structs,
            &program.enums,
            &mut out,
        )?;
    }

    out.line("const struct SarifEffectHandler sarif_effect_table[1] = { {0, 0, 0} };")?;
    out.line("const size_t sarif_effect_table_len = 0;")?;

    Ok(out.buf)
}

struct Output {
    buf: String,
    indent: u32,
}

impl Output {
    fn new() -> Self {
        Output {
            buf: String::new(),
            indent: 0,
        }
    }
    fn line(&mut self, s: &str) -> Result<(), String> {
        for _ in 0..self.indent {
            self.buf.push_str("    ");
        }
        writeln!(self.buf, "{}", s).map_err(to_string)
    }
    fn block_open(&mut self, header: &str) -> Result<(), String> {
        self.line(header)?;
        self.indent += 1;
        Ok(())
    }
    fn block_close(&mut self) -> Result<(), String> {
        self.indent = self.indent.saturating_sub(1);
        self.line("}")
    }
}

fn to_string(e: impl std::fmt::Display) -> String {
    format!("{e}")
}

fn count_instructions(func: &Function) -> u32 {
    let mut count = 0u32;
    for inst in &func.instructions {
        if let Some(dest) = inst_dest(inst)
            && dest.0 >= count
        {
            count = dest.0 + 1;
        }
        count_instructions_sub(inst, &mut count);
    }
    count
}

fn count_instructions_sub(inst: &Inst, count: &mut u32) {
    match inst {
        Inst::If {
            then_insts,
            else_insts,
            ..
        } => {
            for sub in then_insts {
                if let Some(d) = inst_dest(sub)
                    && d.0 >= *count
                {
                    *count = d.0 + 1;
                }
                count_instructions_sub(sub, count);
            }
            for sub in else_insts {
                if let Some(d) = inst_dest(sub)
                    && d.0 >= *count
                {
                    *count = d.0 + 1;
                }
                count_instructions_sub(sub, count);
            }
        }
        Inst::While {
            condition_insts,
            body_insts,
            ..
        } => {
            for sub in condition_insts {
                if let Some(d) = inst_dest(sub)
                    && d.0 >= *count
                {
                    *count = d.0 + 1;
                }
                count_instructions_sub(sub, count);
            }
            for sub in body_insts {
                if let Some(d) = inst_dest(sub)
                    && d.0 >= *count
                {
                    *count = d.0 + 1;
                }
                count_instructions_sub(sub, count);
            }
        }
        Inst::Repeat { body_insts, .. } => {
            for sub in body_insts {
                if let Some(d) = inst_dest(sub)
                    && d.0 >= *count
                {
                    *count = d.0 + 1;
                }
                count_instructions_sub(sub, count);
            }
        }
        Inst::Handle { body_insts, .. } => {
            for sub in body_insts {
                if let Some(d) = inst_dest(sub)
                    && d.0 >= *count
                {
                    *count = d.0 + 1;
                }
                count_instructions_sub(sub, count);
            }
        }
        _ => {}
    }
}

fn inst_dest(inst: &Inst) -> Option<ValueId> {
    match inst {
        Inst::ConstInt { dest, .. } => Some(*dest),
        Inst::ConstBool { dest, .. } => Some(*dest),
        Inst::ConstText { dest, .. } => Some(*dest),
        Inst::ConstF64 { dest, .. } => Some(*dest),
        Inst::LoadParam { dest, .. } => Some(*dest),
        Inst::LoadLocal { dest, .. } => Some(*dest),
        Inst::StoreLocal { .. } => None,
        Inst::Add { dest, .. } => Some(*dest),
        Inst::Sub { dest, .. } => Some(*dest),
        Inst::Mul { dest, .. } => Some(*dest),
        Inst::Div { dest, .. } => Some(*dest),
        Inst::Rem { dest, .. } => Some(*dest),
        Inst::BitAnd { dest, .. } => Some(*dest),
        Inst::BitOr { dest, .. } => Some(*dest),
        Inst::BitXor { dest, .. } => Some(*dest),
        Inst::Shl { dest, .. } => Some(*dest),
        Inst::Shr { dest, .. } => Some(*dest),
        Inst::Sqrt { dest, .. } => Some(*dest),
        Inst::And { dest, .. } => Some(*dest),
        Inst::Or { dest, .. } => Some(*dest),
        Inst::Eq { dest, .. } => Some(*dest),
        Inst::Ne { dest, .. } => Some(*dest),
        Inst::Lt { dest, .. } => Some(*dest),
        Inst::Le { dest, .. } => Some(*dest),
        Inst::Gt { dest, .. } => Some(*dest),
        Inst::Ge { dest, .. } => Some(*dest),
        Inst::ArgCount { dest } => Some(*dest),
        Inst::ArgText { dest, .. } => Some(*dest),
        Inst::StdinText { dest } => Some(*dest),
        Inst::StdinBytes { dest } => Some(*dest),
        Inst::StdoutWrite { .. } => None,
        Inst::AllocPush => None,
        Inst::AllocPop => None,
        Inst::ParseI32 { dest, .. } => Some(*dest),
        Inst::ParseI32Range { dest, .. } => Some(*dest),
        Inst::ParseF64 { dest, .. } => Some(*dest),
        Inst::TextLen { dest, .. } => Some(*dest),
        Inst::BytesLen { dest, .. } => Some(*dest),
        Inst::TextByte { dest, .. } => Some(*dest),
        Inst::BytesByte { dest, .. } => Some(*dest),
        Inst::TextCmp { dest, .. } => Some(*dest),
        Inst::TextEqRange { dest, .. } => Some(*dest),
        Inst::TextFindByteRange { dest, .. } => Some(*dest),
Inst::BytesFindByteRange { dest, .. } => Some(*dest),
            Inst::BytesFieldEnd { dest, .. } => Some(*dest),
            Inst::BytesNextField { dest, .. } => Some(*dest),
            Inst::TextLineEnd { dest, .. } => Some(*dest),
        Inst::TextNextLine { dest, .. } => Some(*dest),
        Inst::TextFieldEnd { dest, .. } => Some(*dest),
        Inst::TextNextField { dest, .. } => Some(*dest),
        Inst::TextConcat { dest, .. } => Some(*dest),
        Inst::TextSlice { dest, .. } => Some(*dest),
        Inst::BytesSlice { dest, .. } => Some(*dest),
        Inst::BytesToText { dest, .. } => Some(*dest),
        Inst::BytesMaterialize { dest, .. } => Some(*dest),
        Inst::TextFromF64Fixed { dest, .. } => Some(*dest),
        Inst::F64FromI32 { dest, .. } => Some(*dest),
        Inst::TextBuilderNew { dest } => Some(*dest),
        Inst::TextBuilderAppend { dest, .. } => Some(*dest),
        Inst::TextBuilderAppendCodepoint { dest, .. } => Some(*dest),
        Inst::TextBuilderAppendAscii { dest, .. } => Some(*dest),
        Inst::TextBuilderAppendSlice { dest, .. } => Some(*dest),
        Inst::TextBuilderAppendI32 { dest, .. } => Some(*dest),
        Inst::TextBuilderFinish { dest, .. } => Some(*dest),
        Inst::TextIntern { dest, .. } => Some(*dest),
        Inst::StdoutWriteBuilder { dest, .. } => Some(*dest),
        Inst::TextIndexNew { dest } => Some(*dest),
        Inst::TextIndexSet { dest, .. } => Some(*dest),
        Inst::TextIndexGet { dest, .. } => Some(*dest),
        Inst::TextIndexContains { dest, .. } => Some(*dest),
        Inst::TextIndexGetOrInsert { dest, .. } => Some(*dest),
        Inst::TextIndexKeys { dest, .. } => Some(*dest),
        Inst::ListNew { dest, .. } => Some(*dest),
        Inst::ListLen { dest, .. } => Some(*dest),
        Inst::ListGet { dest, .. } => Some(*dest),
        Inst::ListSet { dest, .. } => Some(*dest),
        Inst::ListPush { dest, .. } => Some(*dest),
        Inst::ListSortText { dest, .. } => Some(*dest),
        Inst::ListSortRecordTextField { dest, .. } => Some(*dest),
        Inst::Call { dest, .. } => Some(*dest),
        Inst::MakeRecord { dest, .. } => Some(*dest),
        Inst::Field { dest, .. } => Some(*dest),
        Inst::MakeEnum { dest, .. } => Some(*dest),
        Inst::EnumPayload { dest, .. } => Some(*dest),
        Inst::EnumTagEq { dest, .. } => Some(*dest),
        Inst::EnumToI32 { dest, .. } => Some(*dest),
        Inst::EnumToText { dest, .. } => Some(*dest),
        Inst::FileOpen { dest, .. } => Some(*dest),
        Inst::FileIsValid { dest, .. } => Some(*dest),
        Inst::FileRead { dest, .. } => Some(*dest),
        Inst::FileReadToEnd { dest, .. } => Some(*dest),
        Inst::FileMmap { dest, .. } => Some(*dest),
        Inst::FileWrite { dest, .. } => Some(*dest),
        Inst::FileSeek { dest, .. } => Some(*dest),
        Inst::FileSize { dest, .. } => Some(*dest),
        Inst::FileExists { dest, .. } => Some(*dest),
        Inst::FileRemove { dest, .. } => Some(*dest),
        Inst::FileClose { .. } => None,
        Inst::If { dest, .. } => Some(*dest),
        Inst::While { dest, .. } => Some(*dest),
        Inst::Repeat { dest, .. } => Some(*dest),
        Inst::Assert { .. } => None,
        Inst::Handle { dest, .. } => Some(*dest),
        Inst::Perform { dest, .. } => Some(*dest),
        Inst::EnvGet { dest, .. } => Some(*dest),
        Inst::EnvSet { dest, .. } => Some(*dest),
        Inst::EnvRemove { dest, .. } => Some(*dest),
        Inst::EnvKeys { dest } => Some(*dest),
        Inst::DirCreate { dest, .. } => Some(*dest),
        Inst::DirRemove { dest, .. } => Some(*dest),
        Inst::DirList { dest, .. } => Some(*dest),
        Inst::DirExists { dest, .. } => Some(*dest),
        Inst::DirCurrent { dest } => Some(*dest),
        Inst::DirChange { dest, .. } => Some(*dest),
        Inst::ProcessId { dest } => Some(*dest),
        Inst::ClockNow { dest } => Some(*dest),
        Inst::ProcessExit { .. } | Inst::ClockSleep { .. } => None,
    }
}

fn emit_function_forward_decl(func: &Function, out: &mut Output) -> Result<(), String> {
    let ret_type = func_type_name(func.return_type.as_deref());
    let mut sig = format!("{} {}", ret_type, func.name);
    write!(sig, "(").map_err(to_string)?;
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 {
            write!(sig, ", ").map_err(to_string)?;
        }
        let tn = func_type_name(Some(&param.ty));
        write!(sig, "{} p{}", tn, i).map_err(to_string)?;
    }
    if func.params.is_empty() {
        sig.push_str("void");
    }
    writeln!(sig, ");").map_err(to_string)?;
    out.line(&sig)
}

fn emit_function(
    func: &Function,
    value_kinds: &BTreeMap<ValueId, CodegenValueKind>,
    structs: &[crate::StructType],
    enums: &[crate::EnumType],
    out: &mut Output,
) -> Result<(), String> {
    let ret_type = func_type_name(func.return_type.as_deref());
    let mut sig = format!("{} {}(", ret_type, func.name);
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 {
            write!(sig, ", ").map_err(to_string)?;
        }
        let tn = func_type_name(Some(&param.ty));
        write!(sig, "{} p{}", tn, i).map_err(to_string)?;
    }
    if func.params.is_empty() {
        sig.push_str("void");
    }
    sig.push_str(") {");
    out.block_open(&sig)?;

    let max_val = count_instructions(func);
    for i in 0..max_val {
        let id = ValueId(i);
        let kind = value_kinds
            .get(&id)
            .cloned()
            .unwrap_or(CodegenValueKind::I32);
        if kind == CodegenValueKind::Unit {
            continue;
        }
        let tn = func_type_name(Some(match kind {
            CodegenValueKind::F64 => "F64",
            _ => "I32",
        }));
        out.line(&format!("{} v{};", tn, i))?;
    }
    for local in &func.mutable_locals {
        let tn = func_type_name(Some(&local.ty));
        let safe_name: String = local
            .ty
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        out.line(&format!("{} {}_local_{};", tn, safe_name, local.slot.0))?;
    }

    emit_instructions(&func.instructions, func, value_kinds, structs, enums, out)?;

    if let Some(result) = &func.result {
        out.line(&format!("return v{};", result.0))?;
    }
    out.block_close()?;
    out.line("")?;
    Ok(())
}

fn safe_local_name(ty: &str) -> String {
    ty.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

fn func_type_name(ty: Option<&str>) -> &'static str {
    match ty {
        Some("F64") => "double",
        Some("Unit") | None => "void",
        _ => "uint64_t",
    }
}

fn is_signed(val: &ValueId, value_kinds: &BTreeMap<ValueId, CodegenValueKind>) -> bool {
    matches!(value_kinds.get(val), Some(CodegenValueKind::I32))
}

fn is_text(val: &ValueId, value_kinds: &BTreeMap<ValueId, CodegenValueKind>) -> bool {
    matches!(value_kinds.get(val), Some(CodegenValueKind::Text))
}

fn is_f64(val: &ValueId, value_kinds: &BTreeMap<ValueId, CodegenValueKind>) -> bool {
    matches!(value_kinds.get(val), Some(CodegenValueKind::F64))
}

fn emit_instructions(
    insts: &[Inst],
    func: &Function,
    value_kinds: &BTreeMap<ValueId, CodegenValueKind>,
    structs: &[crate::StructType],
    enums: &[crate::EnumType],
    out: &mut Output,
) -> Result<(), String> {
    for inst in insts {
        emit_inst(inst, func, value_kinds, structs, enums, out)?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn emit_inst(
    inst: &Inst,
    func: &Function,
    value_kinds: &BTreeMap<ValueId, CodegenValueKind>,
    structs: &[crate::StructType],
    enums: &[crate::EnumType],
    out: &mut Output,
) -> Result<(), String> {
    match inst {
        Inst::ConstInt { dest, value } => {
            out.line(&format!("v{} = {}llu;", dest.0, value))?;
        }
        Inst::ConstBool { dest, value } => {
            out.line(&format!(
                "v{} = {}llu;",
                dest.0,
                if *value { 1u64 } else { 0u64 }
            ))?;
        }
        Inst::ConstText { dest, value } => {
            let name = format!("_text_{}", dest.0);
            let len = value.len();
            out.line(&format!("static unsigned char {}[] = {{", name))?;
            let mut line = format!(
                "    {}u, {}u, 0u, 0u, 0u, 0u, 0u, 0u /* len={} */,",
                len as u8,
                (len >> 8) as u8,
                len
            );
            for (i, b) in value.bytes().enumerate() {
                if i > 0 && i % 16 == 0 {
                    out.line(&line)?;
                    line = "    ".to_string();
                }
                write!(line, "{}u,", b).map_err(to_string)?;
            }
            out.line(&line)?;
            out.line("};")?;
            out.line(&format!(
                "v{} = (uint64_t)(unsigned char*){};",
                dest.0, name
            ))?;
        }
        Inst::ConstF64 { dest, bits } => {
            let val = f64::from_bits(*bits);
            out.line(&format!("v{} = {};", dest.0, val))?;
        }
        Inst::Add { dest, left, right } => {
            out.line(&format!("v{} = {} + {};", dest.0, vref(left), vref(right)))?;
        }
        Inst::Sub { dest, left, right } => {
            out.line(&format!("v{} = {} - {};", dest.0, vref(left), vref(right)))?;
        }
        Inst::Mul { dest, left, right } => {
            out.line(&format!("v{} = {} * {};", dest.0, vref(left), vref(right)))?;
        }
        Inst::Div { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = (int64_t){} / (int64_t){};",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!("v{} = {} / {};", dest.0, vref(left), vref(right)))?;
            }
        }
        Inst::Rem { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = (int64_t){} % (int64_t){};",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!("v{} = {} % {};", dest.0, vref(left), vref(right)))?;
            }
        }
        Inst::BitAnd { dest, left, right } => {
            out.line(&format!("v{} = {} & {};", dest.0, vref(left), vref(right)))?;
        }
        Inst::BitOr { dest, left, right } => {
            out.line(&format!("v{} = {} | {};", dest.0, vref(left), vref(right)))?;
        }
        Inst::BitXor { dest, left, right } => {
            out.line(&format!("v{} = {} ^ {};", dest.0, vref(left), vref(right)))?;
        }
        Inst::Shl { dest, left, right } => {
            out.line(&format!(
                "v{} = {} << (int){};",
                dest.0,
                vref(left),
                vref(right)
            ))?;
        }
        Inst::Shr { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = (int64_t){} >> (int){};",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!(
                    "v{} = {} >> (int){};",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            }
        }
        Inst::Sqrt { dest, value } => {
            out.line(&format!("v{} = sqrt({});", dest.0, vref(value)))?;
        }
        Inst::And { dest, left, right } => {
            out.line(&format!(
                "v{} = ({} && {}) ? 1 : 0;",
                dest.0,
                vref(left),
                vref(right)
            ))?;
        }
        Inst::Or { dest, left, right } => {
            out.line(&format!(
                "v{} = ({} || {}) ? 1 : 0;",
                dest.0,
                vref(left),
                vref(right)
            ))?;
        }
    Inst::Eq { dest, left, right } => {
        if is_text(left, value_kinds) {
            out.line(&format!(
                "v{} = sarif_text_eq((const unsigned char*){}, (const unsigned char*){}) ? 1 : 0;",
                dest.0, vref(left), vref(right)
            ))?;
        } else {
            out.line(&format!(
                "v{} = ({} == {}) ? 1 : 0;",
                dest.0, vref(left), vref(right)
            ))?;
        }
    }
    Inst::Ne { dest, left, right } => {
        if is_text(left, value_kinds) {
            out.line(&format!(
                "v{} = sarif_text_eq((const unsigned char*){}, (const unsigned char*){}) ? 0 : 1;",
                dest.0, vref(left), vref(right)
            ))?;
        } else {
            out.line(&format!(
                "v{} = ({} != {}) ? 1 : 0;",
                dest.0, vref(left), vref(right)
            ))?;
        }
    }
    Inst::Lt { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = ((int64_t){} < (int64_t){}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!(
                    "v{} = ({} < {}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            }
        }
        Inst::Le { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = ((int64_t){} <= (int64_t){}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!(
                    "v{} = ({} <= {}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            }
        }
        Inst::Gt { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = ((int64_t){} > (int64_t){}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!(
                    "v{} = ({} > {}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            }
        }
        Inst::Ge { dest, left, right } => {
            if is_signed(left, value_kinds) {
                out.line(&format!(
                    "v{} = ((int64_t){} >= (int64_t){}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            } else {
                out.line(&format!(
                    "v{} = ({} >= {}) ? 1 : 0;",
                    dest.0,
                    vref(left),
                    vref(right)
                ))?;
            }
        }
        Inst::F64FromI32 { dest, value } => {
            out.line(&format!("v{} = (double)(int64_t){};", dest.0, vref(value)))?;
        }
        Inst::TextLen { dest, text } => {
            out.line(&format!(
                "v{} = sarif_text_len((const unsigned char*){});",
                dest.0,
                vref(text)
            ))?;
        }
        Inst::BytesLen { dest, bytes } => {
            out.line(&format!(
                "v{} = (int64_t)sarif_bytes_len((const unsigned char*){});",
                dest.0,
                vref(bytes)
            ))?;
        }
        Inst::TextByte { dest, text, index } => {
            out.line(&format!(
                "v{} = (uint64_t)((const unsigned char*){})[8 + (uint64_t){}];",
                dest.0,
                vref(text),
                vref(index)
            ))?;
        }
        Inst::BytesByte { dest, bytes, index } => {
            out.line(&format!(
                "v{} = sarif_bytes_byte((const unsigned char*){}, (uint64_t)(int64_t){});",
                dest.0,
                vref(bytes),
                vref(index)
            ))?;
        }
        Inst::TextCmp { dest, left, right } => {
            out.line(&format!("v{} = (uint64_t)(int64_t)sarif_text_cmp((const unsigned char*){}, (const unsigned char*){});", dest.0, vref(left), vref(right)))?;
        }
        Inst::TextEqRange {
            dest,
            source,
            start,
            end,
            expected,
        } => {
            out.line(&format!("v{} = sarif_text_eq_range((const unsigned char*){}, (int64_t){}, (int64_t){}, (const unsigned char*){});", dest.0, vref(source), vref(start), vref(end), vref(expected)))?;
        }
        Inst::TextFindByteRange {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_find_byte_range((const unsigned char*){}, (int64_t){}, (int64_t){}, (int64_t){});", dest.0, vref(source), vref(start), vref(end), vref(byte)))?;
        }
Inst::BytesFindByteRange {
        dest,
        source,
        start,
        end,
        byte,
    } => {
        out.line(&format!("v{} = (uint64_t)sarif_text_find_byte_range((const unsigned char*){}, (int64_t){}, (int64_t){}, (int64_t){});", dest.0, vref(source), vref(start), vref(end), vref(byte)))?;
    }
    Inst::BytesFieldEnd {
        dest,
        source,
        start,
        end,
        byte,
    } => {
        out.line(&format!("v{} = (uint64_t)sarif_text_find_byte_range((const unsigned char*){}, (int64_t){}, (int64_t){}, (int64_t){});", dest.0, vref(source), vref(start), vref(end), vref(byte)))?;
    }
    Inst::BytesNextField {
        dest,
        source,
        start,
        end,
        byte,
    } => {
        out.line(&format!("v{} = (uint64_t)sarif_text_next_field((const unsigned char*){}, (int64_t){}, (int64_t){}, (int64_t){});", dest.0, vref(source), vref(start), vref(end), vref(byte)))?;
    }
    Inst::TextLineEnd {
            dest,
            source,
            start,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_line_end((const unsigned char*){}, (int64_t){});",
                dest.0,
                vref(source),
                vref(start)
            ))?;
        }
        Inst::TextNextLine {
            dest,
            source,
            start,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_next_line((const unsigned char*){}, (int64_t){});",
                dest.0,
                vref(source),
                vref(start)
            ))?;
        }
        Inst::TextFieldEnd {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_find_byte_range((const unsigned char*){}, (int64_t){}, (int64_t){}, (int64_t){});", dest.0, vref(source), vref(start), vref(end), vref(byte)))?;
        }
        Inst::TextNextField {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_next_field((const unsigned char*){}, (int64_t){}, (int64_t){}, (int64_t){});", dest.0, vref(source), vref(start), vref(end), vref(byte)))?;
        }
        Inst::TextConcat { dest, left, right } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_concat((const unsigned char*){}, (const unsigned char*){});", dest.0, vref(left), vref(right)))?;
        }
        Inst::TextSlice {
            dest,
            text,
            start,
            end,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_slice((const unsigned char*){}, (uint64_t)(int64_t){}, (uint64_t)(int64_t){});", dest.0, vref(text), vref(start), vref(end)))?;
        }
        Inst::BytesSlice {
            dest,
            bytes,
            start,
            end,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_bytes_slice((const unsigned char*){}, (uint64_t)(int64_t){}, (uint64_t)(int64_t){});", dest.0, vref(bytes), vref(start), vref(end)))?;
        }
        Inst::BytesToText { dest, bytes } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_bytes_to_text((const unsigned char*){});",
                dest.0,
                vref(bytes)
            ))?;
        }
        Inst::BytesMaterialize { dest, bytes } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_bytes_materialize((const unsigned char*){});",
                dest.0,
                vref(bytes)
            ))?;
        }
        Inst::TextFromF64Fixed {
            dest,
            value,
            digits,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_from_f64_fixed({}, (int64_t){});",
                dest.0,
                vref(value),
                vref(digits)
            ))?;
        }
        Inst::Call { dest, callee, args } => {
            let callee_is_main = callee == "main" && func.name != "main";
            let callee_name = if callee_is_main {
                "sarif_user_main"
            } else {
                callee
            };
            let mut c = format!("{}(", callee_name);
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(c, ", ").map_err(to_string)?;
                }
                write!(c, "{}", vref(arg)).map_err(to_string)?;
            }
            c.push(')');
            let dest_kind = value_kinds.get(dest);
            if dest_kind == Some(&CodegenValueKind::Unit) {
                out.line(&format!("{};", c))?;
            } else {
                out.line(&format!("v{} = (uint64_t){};", dest.0, c))?;
            }
        }
        Inst::LoadParam { dest, index } => {
            out.line(&format!("v{} = p{};", dest.0, index))?;
        }
        Inst::LoadLocal { dest, slot } => {
            let decl = func.mutable_locals.iter().find(|l| l.slot == *slot);
            if let Some(local) = decl {
                out.line(&format!(
                    "v{} = {}_local_{};",
                    dest.0,
                    safe_local_name(&local.ty),
                    slot.0
                ))?;
            } else {
                out.line(&format!(
                    "v{} = 0; /* unknown local slot {} */",
                    dest.0, slot.0
                ))?;
            }
        }
        Inst::StoreLocal { slot, src } => {
            let decl = func.mutable_locals.iter().find(|l| l.slot == *slot);
            if let Some(local) = decl {
                out.line(&format!(
                    "{}_local_{} = {};",
                    safe_local_name(&local.ty),
                    slot.0,
                    vref(src)
                ))?;
            } else {
                out.line(&format!(
                    "/* unknown local slot {} = {} */",
                    slot.0,
                    vref(src)
                ))?;
            }
        }
        Inst::AllocPush => {
            out.line("sarif_alloc_push();")?;
        }
        Inst::AllocPop => {
            out.line("sarif_alloc_pop();")?;
        }
        Inst::ArgCount { dest } => {
            out.line(&format!("v{} = sarif_arg_count();", dest.0))?;
        }
        Inst::ArgText { dest, index } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_arg_text((int64_t){});",
                dest.0,
                vref(index)
            ))?;
        }
        Inst::StdinText { dest } | Inst::StdinBytes { dest } => {
            out.line(&format!("v{} = (uint64_t)sarif_stdin_text();", dest.0))?;
        }
        Inst::StdoutWrite { text } => {
            out.line(&format!(
                "sarif_stdout_write((const unsigned char*){});",
                vref(text)
            ))?;
        }
        Inst::ParseI32 { dest, text } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_parse_i32((const unsigned char*){});",
                dest.0,
                vref(text)
            ))?;
        }
        Inst::ParseI32Range {
            dest,
            text,
            start,
            end,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_parse_i32_range((const unsigned char*){}, (int64_t){}, (int64_t){});", dest.0, vref(text), vref(start), vref(end)))?;
        }
        Inst::ParseF64 { dest, text } => {
            out.line(&format!(
                "v{} = sarif_parse_f64((const unsigned char*){});",
                dest.0,
                vref(text)
            ))?;
        }
        Inst::Assert { condition, kind } => {
            let msg = match kind {
                ContractKind::Requires => "precondition failed",
                ContractKind::Ensures => "postcondition failed",
                ContractKind::Bounds => "bounds check failed",
            };
            out.line(&format!(
                "if (!({})) {{ sarif_fatal_error(\"{}\"); }}",
                vref(condition),
                msg
            ))?;
        }
        Inst::TextBuilderNew { dest } | Inst::TextIndexNew { dest } => {
            let func_name = if matches!(inst, Inst::TextBuilderNew { .. }) {
                "sarif_text_builder_new"
            } else {
                "sarif_text_index_new"
            };
            out.line(&format!("v{} = (uint64_t){}();", dest.0, func_name))?;
        }
        Inst::TextBuilderAppend {
            dest,
            builder,
            text,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_builder_append((void*){}, (const unsigned char*){});",
                dest.0,
                vref(builder),
                vref(text)
            ))?;
        }
        Inst::TextBuilderAppendCodepoint {
            dest,
            builder,
            codepoint,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_builder_append_codepoint((void*){}, (int64_t){});",
                dest.0,
                vref(builder),
                vref(codepoint)
            ))?;
        }
        Inst::TextBuilderAppendAscii {
            dest,
            builder,
            byte,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_builder_append_ascii((void*){}, (int64_t){});",
                dest.0,
                vref(builder),
                vref(byte)
            ))?;
        }
        Inst::TextBuilderAppendSlice {
            dest,
            builder,
            text,
            start,
            end,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_builder_append_slice((void*){}, (const unsigned char*){}, (int64_t){}, (int64_t){});", dest.0, vref(builder), vref(text), vref(start), vref(end)))?;
        }
        Inst::TextBuilderAppendI32 {
            dest,
            builder,
            value,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_builder_append_i32((void*){}, (int64_t){});",
                dest.0,
                vref(builder),
                vref(value)
            ))?;
        }
        Inst::TextBuilderFinish { dest, builder } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_builder_finish((void*){});",
                dest.0,
                vref(builder)
            ))?;
        }
        Inst::TextIntern { dest, text } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_intern((const unsigned char*){});",
                dest.0,
                vref(text)
            ))?;
        }
        Inst::StdoutWriteBuilder { dest, builder } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_stdout_write_builder((void*){});",
                dest.0,
                vref(builder)
            ))?;
        }
        Inst::TextIndexSet {
            dest,
            index,
            key,
            value,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_index_set((void*){}, (uint64_t){}, (int64_t){});",
                dest.0,
                vref(index),
                vref(key),
                vref(value)
            ))?;
        }
        Inst::TextIndexGet { dest, index, key } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_index_get((void*){}, (uint64_t){});",
                dest.0,
                vref(index),
                vref(key)
            ))?;
        }
        Inst::TextIndexContains { dest, index, key } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_index_contains((void*){}, (uint64_t){});",
                dest.0,
                vref(index),
                vref(key)
            ))?;
        }
        Inst::TextIndexGetOrInsert {
            dest,
            index,
            key,
            next,
        } => {
            out.line(&format!("v{} = (uint64_t)sarif_text_index_get_or_insert((void*){}, (uint64_t){}, (int64_t){});", dest.0, vref(index), vref(key), vref(next)))?;
        }
        Inst::TextIndexKeys { dest, index } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_text_index_keys((void*){});",
                dest.0,
                vref(index)
            ))?;
        }
    Inst::ListNew { dest, len, value } => {
        if is_f64(value, value_kinds) {
            out.line("{")?;
            out.indent += 1;
            out.line("uint64_t _tmp;")?;
            out.line(&format!("memcpy(&_tmp, &v{}, sizeof(double));", value.0))?;
            out.line(&format!("v{} = (uint64_t)sarif_list_new((int64_t){}, _tmp);", dest.0, vref(len)))?;
            out.indent -= 1;
            out.line("}")?;
        } else {
            out.line(&format!(
                "v{} = (uint64_t)sarif_list_new((int64_t){}, (uint64_t){});",
                dest.0, vref(len), vref(value)
            ))?;
        }
    }
        Inst::ListLen { dest, list } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_list_len((void*){});",
                dest.0,
                vref(list)
            ))?;
        }
    Inst::ListGet { dest, list, index } => {
        if is_f64(dest, value_kinds) {
            out.line("{")?;
            out.indent += 1;
            out.line(&format!("uint64_t _tmp = sarif_list_get((void*){}, (int64_t){});", vref(list), vref(index)))?;
            out.line(&format!("memcpy(&v{}, &_tmp, sizeof(double));", dest.0))?;
            out.indent -= 1;
            out.line("}")?;
        } else {
            out.line(&format!(
                "v{} = sarif_list_get((void*){}, (int64_t){});",
                dest.0, vref(list), vref(index)
            ))?;
        }
    }
    Inst::ListSet {
        dest,
        list,
        index,
        value,
    } => {
        if is_f64(value, value_kinds) {
            out.line("{")?;
            out.indent += 1;
            out.line("uint64_t _tmp;")?;
            out.line(&format!("memcpy(&_tmp, &v{}, sizeof(double));", value.0))?;
            out.line(&format!("v{} = (uint64_t)sarif_list_set((void*){}, (int64_t){}, _tmp);", dest.0, vref(list), vref(index)))?;
            out.indent -= 1;
            out.line("}")?;
        } else {
            out.line(&format!(
                "v{} = (uint64_t)sarif_list_set((void*){}, (int64_t){}, (uint64_t){});",
                dest.0, vref(list), vref(index), vref(value)
            ))?;
        }
    }
    Inst::ListPush {
        dest,
        list,
        len,
        value,
    } => {
        if is_f64(value, value_kinds) {
            out.line("{")?;
            out.indent += 1;
            out.line("uint64_t _tmp;")?;
            out.line(&format!("memcpy(&_tmp, &v{}, sizeof(double));", value.0))?;
            out.line(&format!("v{} = (uint64_t)sarif_list_push((void*){}, (int64_t){}, _tmp);", dest.0, vref(list), vref(len)))?;
            out.indent -= 1;
            out.line("}")?;
        } else {
            out.line(&format!(
                "v{} = (uint64_t)sarif_list_push((void*){}, (int64_t){}, (uint64_t){});",
                dest.0, vref(list), vref(len), vref(value)
            ))?;
        }
    }
        Inst::ListSortText { dest, list, len } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_list_sort_text((void*){}, (int64_t){});",
                dest.0,
                vref(list),
                vref(len)
            ))?;
        }
        Inst::ListSortRecordTextField {
            dest,
            list,
            len,
            field,
        } => {
            let offset = match value_kinds.get(list) {
                Some(CodegenValueKind::List(elem)) => match &**elem {
                    CodegenValueKind::Record(record_name) => {
                        record_field_offset(structs, record_name, field).unwrap_or(0)
                    }
                    _ => 0,
                },
                _ => 0,
            };
            out.line(&format!(
                "v{} = (uint64_t)sarif_list_sort_by_text_field((void*){}, (int64_t){}, {}u);",
                dest.0,
                vref(list),
                vref(len),
                offset
            ))?;
        }
        Inst::MakeRecord {
            dest,
            name: _name,
            fields,
        } => {
            let size = fields.len() * 8;
            out.line(&format!(
                "v{} = (uint64_t)sarif_record_alloc({}u);",
                dest.0,
                size.max(8)
            ))?;
        for (i, (_fname, fval)) in fields.iter().enumerate() {
            if is_f64(fval, value_kinds) {
                out.line(&format!(
                    "sarif_store_f64((unsigned char*)v{}, {}u, {});",
                    dest.0, i * 8, vref(fval)
                ))?;
            } else {
                out.line(&format!(
                    "sarif_store_u64((unsigned char*)v{}, {}u, {});",
                    dest.0, i * 8, vref(fval)
                ))?;
            }
        }
        }
        Inst::Field { dest, base, name } => {
            let offset = value_kinds
                .get(base)
                .and_then(|kind| {
                    if let CodegenValueKind::Record(record_name) = kind {
                        record_field_offset(structs, record_name, name)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
        if is_f64(dest, value_kinds) {
            out.line(&format!(
                "v{} = sarif_load_f64((const unsigned char*){}, {}u);",
                dest.0, vref(base), offset
            ))?;
        } else {
            out.line(&format!(
                "v{} = sarif_load_u64((const unsigned char*){}, {}u);",
                dest.0, vref(base), offset
            ))?;
        }
        }
        Inst::MakeEnum {
            dest,
            name,
            variant,
            payload,
        } => {
            let tag = enum_variant_tag(enums, name, variant).unwrap_or(0);
            let enum_ty = enums.iter().find(|e| e.name == *name);
            let has_payload = enum_ty
                .map(|e| e.variants.iter().any(|v| v.payload_type.is_some()))
                .unwrap_or(false);
            if has_payload {
                out.line(&format!("v{} = (uint64_t)sarif_record_alloc(16u);", dest.0))?;
                out.line(&format!(
                    "sarif_store_u64((unsigned char*)v{}, 0u, {}llu);",
                    dest.0, tag
                ))?;
                if let Some(payload) = payload {
                    out.line(&format!(
                        "sarif_store_u64((unsigned char*)v{}, 8u, {});",
                        dest.0,
                        vref(payload)
                    ))?;
                } else {
                    out.line(&format!(
                        "sarif_store_u64((unsigned char*)v{}, 8u, 0llu);",
                        dest.0
                    ))?;
                }
            } else {
                out.line(&format!("v{} = {}llu;", dest.0, tag))?;
            }
        }
        Inst::EnumPayload {
            dest,
            value,
            payload_type: _,
        } => {
            out.line(&format!(
                "v{} = sarif_load_u64((const unsigned char*){}, 8u);",
                dest.0,
                vref(value)
            ))?;
        }
        Inst::EnumTagEq { dest, value, tag } => {
            let has_payload = value_kinds
                .get(value)
                .and_then(|kind| {
                    if let CodegenValueKind::Enum(enum_name) = kind {
                        let enum_ty = enums.iter().find(|e| e.name == *enum_name)?;
                        Some(enum_ty.variants.iter().any(|v| v.payload_type.is_some()))
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            if has_payload {
                out.line(&format!(
                    "v{} = (sarif_load_u64((const unsigned char*){}, 0u) == {}llu) ? 1 : 0;",
                    dest.0,
                    vref(value),
                    tag
                ))?;
            } else {
                out.line(&format!(
                    "v{} = ({} == {}llu) ? 1 : 0;",
                    dest.0,
                    vref(value),
                    tag
                ))?;
            }
        }
        Inst::EnumToI32 {
            dest,
            value,
            discriminants,
        } => {
            let has_payload = value_kinds
                .get(value)
                .and_then(|kind| {
                    if let CodegenValueKind::Enum(enum_name) = kind {
                        let enum_ty = enums.iter().find(|e| e.name == *enum_name)?;
                        Some(enum_ty.variants.iter().any(|v| v.payload_type.is_some()))
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            let tag_expr = if has_payload {
                format!("sarif_load_u64((const unsigned char*){}, 0u)", vref(value))
            } else {
                vref(value)
            };
            if discriminants.is_empty() {
                out.line(&format!("v{} = (int64_t){};", dest.0, tag_expr))?;
            } else {
                let mut s = format!("v{} = (int64_t)({}", dest.0, tag_expr);
                for (i, disc) in discriminants.iter().enumerate() {
                    s.push_str(&format!(" == {}llu ? {}llu : ", i, disc));
                }
                s.push_str("0);");
                out.line(&s)?;
            }
        }
        Inst::EnumToText {
            dest,
            value,
            variant_names,
        } => {
            let has_payload = value_kinds
                .get(value)
                .and_then(|kind| {
                    if let CodegenValueKind::Enum(enum_name) = kind {
                        let enum_ty = enums.iter().find(|e| e.name == *enum_name)?;
                        Some(enum_ty.variants.iter().any(|v| v.payload_type.is_some()))
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            let tag_expr = if has_payload {
                format!("sarif_load_u64((const unsigned char*){}, 0u)", vref(value))
            } else {
                vref(value)
            };
            if variant_names.is_empty() {
                out.line(&format!("v{} = (const unsigned char*)\"\";", dest.0))?;
            } else {
                let mut s = format!("v{} = ", dest.0);
                for (i, vn) in variant_names.iter().enumerate() {
                    if i > 0 {
                        s.push_str(" : ");
                    }
                    s.push_str(&format!(
                        "{} == {}llu ? (const unsigned char*)\"{}\"",
                        tag_expr, i, vn
                    ));
                }
                s.push_str(" : (const unsigned char*)\"\"");
                out.line(&format!("{};", s))?;
            }
        }
        Inst::FileOpen { dest, path, mode } => {
            out.line(&format!(
                "v{} = sarif_file_open((const unsigned char*){}, (const unsigned char*){});",
                dest.0,
                vref(path),
                vref(mode)
            ))?;
        }
        Inst::FileIsValid { dest, handle } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_file_is_valid({});",
                dest.0,
                vref(handle)
            ))?;
        }
        Inst::FileRead { dest, handle, len } => {
            out.line(&format!(
                "v{} = sarif_file_read({}, (int64_t){});",
                dest.0,
                vref(handle),
                vref(len)
            ))?;
        }
        Inst::FileReadToEnd { dest, handle } => {
            out.line(&format!(
                "v{} = sarif_file_read_to_end({});",
                dest.0,
                vref(handle)
            ))?;
        }
        Inst::FileMmap { dest, path } => {
            out.line(&format!(
                "v{} = sarif_file_mmap((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::FileWrite { dest, handle, data } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_file_write({}, (const unsigned char*){});",
                dest.0,
                vref(handle),
                vref(data)
            ))?;
        }
        Inst::FileSeek {
            dest,
            handle,
            offset,
            whence,
        } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_file_seek({}, (int64_t){}, (int64_t){});",
                dest.0,
                vref(handle),
                vref(offset),
                vref(whence)
            ))?;
        }
        Inst::FileSize { dest, handle } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_file_size({});",
                dest.0,
                vref(handle)
            ))?;
        }
        Inst::FileExists { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_file_exists((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::FileRemove { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_file_remove((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::FileClose { handle } => {
            out.line(&format!("sarif_file_close({});", vref(handle)))?;
        }
        Inst::If {
            dest,
            condition,
            then_insts,
            then_result,
            else_insts,
            else_result,
        } => {
            out.block_open(&format!("if ({}) {{", vref(condition)))?;
            emit_instructions(then_insts, func, value_kinds, structs, enums, out)?;
            if let Some(r) = then_result {
                out.line(&format!("v{} = v{};", dest.0, r.0))?;
            }
            out.block_close()?;
            if !else_insts.is_empty() || else_result.is_some() {
                out.block_open("else {")?;
                emit_instructions(else_insts, func, value_kinds, structs, enums, out)?;
                if let Some(r) = else_result {
                    out.line(&format!("v{} = v{};", dest.0, r.0))?;
                }
                out.block_close()?;
            }
        }
        Inst::While {
            dest: _dest,
            condition_insts,
            condition,
            body_insts,
        } => {
            out.block_open("while (1) {")?;
            emit_instructions(condition_insts, func, value_kinds, structs, enums, out)?;
            out.line(&format!("if (!({})) break;", vref(condition)))?;
            emit_instructions(body_insts, func, value_kinds, structs, enums, out)?;
            out.block_close()?;
        }
        Inst::Repeat {
            dest: _dest,
            count,
            index_slot,
            body_insts,
        } => {
            out.block_open(&format!(
                "for (uint64_t _i = 0; _i < (uint64_t)(int64_t){}; _i++) {{",
                vref(count)
            ))?;
            if let Some(slot) = index_slot {
                if let Some(local) = func.mutable_locals.iter().find(|l| l.slot == *slot) {
                    out.line(&format!(
                        "{}_local_{} = (uint64_t)_i;",
                        safe_local_name(&local.ty),
                        slot.0
                    ))?;
                } else {
                    out.line(&format!("uint64_t __local_{} = (uint64_t)_i;", slot.0))?;
                }
            }
            emit_instructions(body_insts, func, value_kinds, structs, enums, out)?;
            out.block_close()?;
        }
        Inst::Perform {
            dest,
            effect,
            operation,
            args,
        } => {
            let nargs = args.len().min(4);
            for (i, arg) in args.iter().enumerate().take(nargs) {
                out.line(&format!("uint64_t __perf_arg{} = {};", i, vref(arg)))?;
            }
            let a0 = if nargs > 0 { "__perf_arg0" } else { "0" };
            let a1 = if nargs > 1 { "__perf_arg1" } else { "0" };
            let a2 = if nargs > 2 { "__perf_arg2" } else { "0" };
            let a3 = if nargs > 3 { "__perf_arg3" } else { "0" };
            out.line(&format!(
                "v{} = sarif_perform_effect(\"{}\", \"{}\", {}, {}, {}, {}, {});",
                dest.0, effect, operation, a0, a1, a2, a3, nargs
            ))?;
        }
        Inst::EnvGet { dest, key } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_env_get((const unsigned char*){});",
                dest.0,
                vref(key)
            ))?;
        }
        Inst::EnvSet { dest, key, value } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_env_set((const unsigned char*){}, (const unsigned char*){});",
                dest.0,
                vref(key),
                vref(value)
            ))?;
        }
        Inst::EnvRemove { dest, key } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_env_remove((const unsigned char*){});",
                dest.0,
                vref(key)
            ))?;
        }
        Inst::EnvKeys { dest } => {
            out.line(&format!("v{} = (uint64_t)sarif_env_keys();", dest.0))?;
        }
        Inst::DirCreate { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_dir_create((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::DirRemove { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_dir_remove((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::DirList { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_dir_list((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::DirExists { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_dir_exists((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::DirCurrent { dest } => {
            out.line(&format!("v{} = (uint64_t)sarif_dir_current();", dest.0))?;
        }
        Inst::DirChange { dest, path } => {
            out.line(&format!(
                "v{} = (uint64_t)sarif_dir_change((const unsigned char*){});",
                dest.0,
                vref(path)
            ))?;
        }
        Inst::ProcessExit { code } => {
            out.line(&format!("sarif_process_exit((int32_t){});", vref(code)))?;
        }
        Inst::ProcessId { dest } => {
            out.line(&format!("v{} = (uint64_t)sarif_process_id();", dest.0))?;
        }
        Inst::ClockNow { dest } => {
            out.line(&format!("v{} = sarif_clock_now();", dest.0))?;
        }
        Inst::ClockSleep { ms } => {
            out.line(&format!("sarif_clock_sleep((int32_t){});", vref(ms)))?;
        }
        Inst::Handle {
            dest,
            body_insts,
            body_result,
            arms: _arms,
        } => {
            emit_instructions(body_insts, func, value_kinds, structs, enums, out)?;
            if let Some(r) = body_result {
                out.line(&format!("v{} = v{};", dest.0, r.0))?;
            }
        }
    }
    Ok(())
}

fn vref(id: &ValueId) -> String {
    format!("v{}", id.0)
}

fn emit_main_wrapper(
    main: &Function,
    value_kinds: &BTreeMap<ValueId, CodegenValueKind>,
    structs: &[crate::StructType],
    enums: &[crate::EnumType],
    out: &mut Output,
) -> Result<(), String> {
    let ret_type = match main.return_type.as_deref() {
        Some("I32") => "int32_t",
        Some("Bool") => "uint32_t",
        Some("Text") | Some("Bytes") => "uintptr_t",
        Some("F64") => "double",
        _ => "void",
    };

    let sig = if main.params.is_empty() {
        format!("{} sarif_user_main(void)", ret_type)
    } else {
        format!("{} sarif_user_main(uint64_t p0)", ret_type)
    };

    out.block_open(&format!("{} {{", sig))?;

    let max_val = count_instructions(main);
    for i in 0..max_val {
        let id = ValueId(i);
        let kind = value_kinds
            .get(&id)
            .cloned()
            .unwrap_or(CodegenValueKind::I32);
        if kind == CodegenValueKind::Unit {
            continue;
        }
        let tn = func_type_name(Some(match kind {
            CodegenValueKind::F64 => "F64",
            _ => "I32",
        }));
        out.line(&format!("{} v{};", tn, i))?;
    }
    for local in &main.mutable_locals {
        let tn = func_type_name(Some(&local.ty));
        out.line(&format!(
            "{} {}_local_{};",
            tn,
            safe_local_name(&local.ty),
            local.slot.0
        ))?;
    }

    emit_instructions(&main.instructions, main, value_kinds, structs, enums, out)?;

    if let Some(result) = &main.result {
        out.line(&format!("return v{};", result.0))?;
    }
    out.indent -= 1;
    out.line("}")?;

    Ok(())
}

fn infer_value_kinds_for_func(
    func: &Function,
    program: &Program,
) -> Result<BTreeMap<ValueId, CodegenValueKind>, String> {
    let mut kinds = BTreeMap::new();
    infer_insts_kind_c(&func.instructions, func, program, &mut kinds)?;
    Ok(kinds)
}

fn infer_insts_kind_c(
    insts: &[Inst],
    func: &Function,
    program: &Program,
    kinds: &mut BTreeMap<ValueId, CodegenValueKind>,
) -> Result<(), String> {
    for inst in insts {
        infer_inst_kind_c(inst, func, program, kinds)?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn infer_inst_kind_c(
    inst: &Inst,
    func: &Function,
    program: &Program,
    kinds: &mut BTreeMap<ValueId, CodegenValueKind>,
) -> Result<(), String> {
    match inst {
        Inst::LoadParam { dest, index } => {
            let ty = func
                .params
                .get(*index)
                .map(|p| p.ty.as_str())
                .unwrap_or("I32");
            kinds.insert(*dest, str_to_kind(ty, program));
        }
        Inst::LoadLocal { dest, slot } => {
            let ty = func
                .mutable_locals
                .iter()
                .find(|l| l.slot == *slot)
                .map(|l| l.ty.as_str())
                .unwrap_or("I32");
            kinds.insert(*dest, str_to_kind(ty, program));
        }
        Inst::ConstInt { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::ConstBool { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bool);
        }
        Inst::ConstText { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::ConstF64 { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::F64);
        }
        Inst::Add { dest, left, .. }
        | Inst::Sub { dest, left, .. }
        | Inst::Mul { dest, left, .. }
        | Inst::Div { dest, left, .. } => {
            let lk = kinds.get(left).cloned().unwrap_or(CodegenValueKind::I32);
            kinds.insert(*dest, lk);
        }
        Inst::Rem { dest, .. }
        | Inst::BitAnd { dest, .. }
        | Inst::BitOr { dest, .. }
        | Inst::BitXor { dest, .. }
        | Inst::Shl { dest, .. }
        | Inst::Shr { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::Sqrt { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::F64);
        }
        Inst::And { dest, .. } | Inst::Or { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bool);
        }
        Inst::Eq { dest, .. }
        | Inst::Ne { dest, .. }
        | Inst::Lt { dest, .. }
        | Inst::Le { dest, .. }
        | Inst::Gt { dest, .. }
        | Inst::Ge { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bool);
        }
        Inst::F64FromI32 { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::F64);
        }
        Inst::TextLen { dest, .. }
        | Inst::BytesLen { dest, .. }
        | Inst::TextByte { dest, .. }
        | Inst::BytesByte { dest, .. }
        | Inst::TextCmp { dest, .. }
        | Inst::TextEqRange { dest, .. }
        | Inst::TextFindByteRange { dest, .. }
| Inst::BytesFindByteRange { dest, .. }
            | Inst::BytesFieldEnd { dest, .. }
            | Inst::BytesNextField { dest, .. }
            | Inst::TextLineEnd { dest, .. }
        | Inst::TextNextLine { dest, .. }
        | Inst::TextFieldEnd { dest, .. }
        | Inst::TextNextField { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::TextConcat { dest, .. }
        | Inst::TextSlice { dest, .. }
        | Inst::BytesSlice { dest, .. }
        | Inst::BytesToText { dest, .. }
        | Inst::TextFromF64Fixed { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::BytesMaterialize { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bytes);
        }
        Inst::TextBuilderNew { dest }
        | Inst::TextBuilderAppend { dest, .. }
        | Inst::TextBuilderAppendCodepoint { dest, .. }
        | Inst::TextBuilderAppendAscii { dest, .. }
        | Inst::TextBuilderAppendSlice { dest, .. }
        | Inst::TextBuilderAppendI32 { dest, .. }
        | Inst::StdoutWriteBuilder { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::TextBuilder);
        }
        Inst::TextBuilderFinish { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::TextIntern { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::TextIndexNew { dest } | Inst::TextIndexSet { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::TextIndex);
        }
        Inst::TextIndexGet { dest, .. }
        | Inst::TextIndexContains { dest, .. }
        | Inst::TextIndexGetOrInsert { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::TextIndexKeys { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::ListNew { dest, value, .. } => {
            let elem = kinds.get(value).cloned().unwrap_or(CodegenValueKind::I32);
            kinds.insert(*dest, CodegenValueKind::List(Box::new(elem)));
        }
        Inst::ListSet { dest, list, .. }
        | Inst::ListPush { dest, list, .. }
        | Inst::ListSortText { dest, list, .. }
        | Inst::ListSortRecordTextField { dest, list, .. } => {
            let lk = kinds
                .get(list)
                .cloned()
                .unwrap_or_else(|| CodegenValueKind::List(Box::new(CodegenValueKind::I32)));
            kinds.insert(*dest, lk);
        }
        Inst::ListLen { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::ListGet { dest, list, .. } => {
            let elem = match kinds.get(list) {
                Some(CodegenValueKind::List(elem)) => (**elem).clone(),
                _ => CodegenValueKind::I32,
            };
            kinds.insert(*dest, elem);
        }
        Inst::Call { dest, callee, .. } => {
            let callee_func = program.functions.iter().find(|f| f.name == *callee);
            if let Some(func) = callee_func {
                let ty = func.return_type.as_deref().unwrap_or("Unit");
                kinds.insert(*dest, str_to_kind(ty, program));
            } else if let Some(extern_fn) = program.externs.iter().find(|f| f.name == *callee) {
                let ty = extern_fn.return_type.as_deref().unwrap_or("Unit");
                kinds.insert(*dest, str_to_kind(ty, program));
            } else {
                kinds.insert(*dest, CodegenValueKind::I32);
            }
        }
        Inst::MakeRecord { dest, name, .. } => {
            kinds.insert(*dest, CodegenValueKind::Record(name.clone()));
        }
        Inst::Field { dest, base, name } => {
            let base_kind = kinds.get(base);
            let field_kind = match base_kind {
                Some(CodegenValueKind::Record(record_name)) => program
                    .structs
                    .iter()
                    .find(|s| s.name == *record_name)
                    .and_then(|s| s.fields.iter().find(|f| f.name == *name))
                    .map(|f| str_to_kind(&f.ty, program))
                    .unwrap_or(CodegenValueKind::I32),
                _ => CodegenValueKind::I32,
            };
            kinds.insert(*dest, field_kind);
        }
        Inst::MakeEnum { dest, name, .. } => {
            kinds.insert(*dest, CodegenValueKind::Enum(name.clone()));
        }
        Inst::EnumPayload {
            dest, payload_type, ..
        } => {
            kinds.insert(*dest, str_to_kind(payload_type, program));
        }
        Inst::EnumTagEq { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bool);
        }
        Inst::EnumToI32 { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::EnumToText { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::ArgCount { dest }
        | Inst::ParseI32 { dest, .. }
        | Inst::ParseI32Range { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::ParseF64 { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::F64);
        }
        Inst::ArgText { dest, .. } | Inst::StdinText { dest } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::StdinBytes { dest } => {
            kinds.insert(*dest, CodegenValueKind::Bytes);
        }
        Inst::FileOpen { dest, .. } | Inst::FileIsValid { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::File);
        }
        Inst::FileRead { dest, .. }
        | Inst::FileReadToEnd { dest, .. }
        | Inst::FileMmap { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bytes);
        }
        Inst::FileWrite { dest, .. }
        | Inst::FileSeek { dest, .. }
        | Inst::FileSize { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::FileExists { dest, .. } | Inst::FileRemove { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::Bool);
        }
        Inst::EnvGet { dest, .. }
        | Inst::EnvKeys { dest }
        | Inst::DirList { dest, .. }
        | Inst::DirCurrent { dest } => {
            kinds.insert(*dest, CodegenValueKind::Text);
        }
        Inst::EnvSet { dest, .. }
        | Inst::EnvRemove { dest, .. }
        | Inst::DirCreate { dest, .. }
        | Inst::DirRemove { dest, .. }
        | Inst::DirExists { dest, .. }
        | Inst::DirChange { dest, .. } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::ProcessId { dest } => {
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::ClockNow { dest } => {
            kinds.insert(*dest, CodegenValueKind::F64);
        }
        Inst::If {
            dest,
            then_insts,
            then_result,
            else_insts,
            ..
        } => {
            infer_insts_kind_c(then_insts, func, program, kinds)?;
            infer_insts_kind_c(else_insts, func, program, kinds)?;
            if let Some(r) = then_result {
                if let Some(k) = kinds.get(r) {
                    kinds.insert(*dest, k.clone());
                } else {
                    kinds.insert(*dest, CodegenValueKind::Unit);
                }
            } else {
                kinds.insert(*dest, CodegenValueKind::Unit);
            }
        }
        Inst::While {
            dest,
            condition_insts,
            body_insts,
            ..
        } => {
            infer_insts_kind_c(condition_insts, func, program, kinds)?;
            infer_insts_kind_c(body_insts, func, program, kinds)?;
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::Repeat {
            dest, body_insts, ..
        } => {
            infer_insts_kind_c(body_insts, func, program, kinds)?;
            kinds.insert(*dest, CodegenValueKind::I32);
        }
        Inst::Handle {
            dest,
            body_insts,
            body_result,
            ..
        } => {
            infer_insts_kind_c(body_insts, func, program, kinds)?;
            if let Some(r) = body_result
                && let Some(k) = kinds.get(r)
            {
                kinds.insert(*dest, k.clone());
            }
        }
        Inst::Assert { .. }
        | Inst::StdoutWrite { .. }
        | Inst::AllocPush
        | Inst::AllocPop
        | Inst::FileClose { .. }
        | Inst::ProcessExit { .. }
        | Inst::ClockSleep { .. }
        | Inst::StoreLocal { .. } => {}
        _ => {}
    }
    Ok(())
}

fn str_to_kind(ty: &str, program: &Program) -> CodegenValueKind {
    if let Some(element) = ty.strip_prefix("List[").and_then(|s| s.strip_suffix(']')) {
        return CodegenValueKind::List(Box::new(str_to_kind(element, program)));
    }
    match ty {
        "I32" => CodegenValueKind::I32,
        "F64" => CodegenValueKind::F64,
        "Bool" => CodegenValueKind::Bool,
        "Text" => CodegenValueKind::Text,
        "Bytes" => CodegenValueKind::Bytes,
        "TextBuilder" => CodegenValueKind::TextBuilder,
        "TextIndex" => CodegenValueKind::TextIndex,
        "File" => CodegenValueKind::File,
        "Unit" => CodegenValueKind::Unit,
        other if program.enums.iter().any(|e| e.name == other) => {
            CodegenValueKind::Enum(other.to_owned())
        }
        other if program.structs.iter().any(|s| s.name == other) => {
            CodegenValueKind::Record(other.to_owned())
        }
        _ => CodegenValueKind::I32,
    }
}
