use std::collections::BTreeMap;
use std::fmt::Write;

use super::{
    CodegenValueKind as WasmValueKind, Function, Inst, Program, ValueId, for_each_inst_recursive,
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
}

#[derive(Clone, Debug)]
pub(crate) struct WasmRecord {
    pub(crate) name: String,
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
    pub(crate) name: String,
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
        }
    }
}

mod memory;
mod runtime;

pub use runtime::{run_function_wasm, run_main_wasm};

/// # Errors
///
/// Returns an error if the stage-0 Wasm backend cannot represent one of the
/// program's values or instructions.
pub fn emit_wat(program: &Program) -> Result<String, WasmError> {
    reject_text_builder_program(program)?;
    let emitter = WasmEmitter::new(program)?;
    emitter.emit()
}

/// # Errors
///
/// Returns an error if the stage-0 Wasm backend cannot represent one of the
/// program's values or instructions, or if the generated WAT is invalid.
pub fn emit_wasm(program: &Program) -> Result<Vec<u8>, WasmError> {
    let wat = emit_wat(program)?;
    wat::parse_str(&wat).map_err(|error| WasmError::new(error.to_string()))
}

fn reject_text_builder_program(program: &Program) -> Result<(), WasmError> {
    const TEXT_BUILDER_UNSUPPORTED_MESSAGE: &str =
        "wasm backend does not yet support text builder builtins in stage-0";
    let has_text_builder_type = program.functions.iter().any(|function| {
        function
            .mutable_locals
            .iter()
            .any(|local| local.ty == "TextBuilder")
    });
    if has_text_builder_type {
        return Err(WasmError::new(TEXT_BUILDER_UNSUPPORTED_MESSAGE));
    }
    for function in &program.functions {
        let mut has_text_builder_inst = false;
        for_each_inst_recursive(&function.instructions, &mut |inst| {
            if matches!(
                inst,
                Inst::TextBuilderNew { .. }
                    | Inst::TextBuilderAppend { .. }
                    | Inst::TextBuilderFinish { .. }
            ) {
                has_text_builder_inst = true;
            }
        });
        if has_text_builder_inst {
            return Err(WasmError::new(TEXT_BUILDER_UNSUPPORTED_MESSAGE));
        }
    }
    Ok(())
}

struct WasmEmitter<'a> {
    program: &'a Program,
    enums: BTreeMap<String, WasmEnum>,
    records: BTreeMap<String, WasmRecord>,
    signatures: BTreeMap<String, WasmSignature>,
}

struct WasmSignature {
    params: Vec<WasmType>,
    result: Option<WasmType>,
}

impl<'a> WasmEmitter<'a> {
    fn new(program: &'a Program) -> Result<Self, WasmError> {
        let mut enums = BTreeMap::new();
        for enum_ty in &program.enums {
            let mut variants = Vec::new();
            for variant in &enum_ty.variants {
                variants.push(WasmEnumVariant {
                    name: variant.name.clone(),
                    payload: variant
                        .payload_type
                        .as_deref()
                        .map(|ty| wasm_value_kind(ty, program))
                        .transpose()?,
                });
            }
            enums.insert(
                enum_ty.name.clone(),
                WasmEnum {
                    name: enum_ty.name.clone(),
                    variants,
                },
            );
        }

        let mut records = BTreeMap::new();
        for struct_ty in &program.structs {
            let mut fields = Vec::new();
            let mut offset = 0u32;
            for field in &struct_ty.fields {
                let kind = wasm_value_kind(&field.ty, program)?;
                fields.push(WasmField {
                    name: field.name.clone(),
                    kind,
                    offset,
                });
                offset += 8;
            }
            records.insert(
                struct_ty.name.clone(),
                WasmRecord {
                    name: struct_ty.name.clone(),
                    size: offset,
                    fields,
                },
            );
        }

        let mut signatures = BTreeMap::new();
        for function in &program.functions {
            signatures.insert(function.name.clone(), wasm_signature(function)?);
        }

        Ok(Self {
            program,
            enums,
            records,
            signatures,
        })
    }

    fn emit(&self) -> Result<String, WasmError> {
        let mut output = "(module\n".to_owned();
        output.push_str("  (memory (export \"memory\") 1)\n");
        output.push_str("  (global $heap_ptr (mut i32) (i32.const 0))\n\n");

        self.emit_allocator(&mut output);
        self.emit_text_eq(&mut output);
        self.emit_text_concat(&mut output);
        self.emit_text_slice(&mut output);
        self.emit_f64_vec_helpers(&mut output);
        self.emit_structural_eq_helpers(&mut output)?;

        for function in &self.program.functions {
            self.emit_function(&mut output, function)?;
        }

        output.push_str(")\n");
        Ok(output)
    }

    fn emit_allocator(&self, output: &mut String) {
        output.push_str("  (func $alloc (param $size i32) (result i32)\n");
        output.push_str("    (local $ptr i32)\n");
        output.push_str("    global.get $heap_ptr\n");
        output.push_str("    local.set $ptr\n");
        output.push_str("    global.get $heap_ptr\n");
        output.push_str("    local.get $size\n");
        output.push_str("    i32.add\n");
        output.push_str("    global.set $heap_ptr\n");
        output.push_str("    local.get $ptr)\n\n");
    }

    fn emit_text_eq(&self, output: &mut String) {
        output.push_str("  (func $text_eq (param $left i64) (param $right i64) (result i64)\n");
        output.push_str("    (local $left_ptr i32)\n");
        output.push_str("    (local $right_ptr i32)\n");
        output.push_str("    (local $len i32)\n");
        output.push_str("    (local $index i32)\n");
        output.push_str("    (local $equal i64)\n");
        output.push_str("    local.get $left\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shr_u\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $len\n");
        output.push_str("    local.get $right\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shr_u\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.get $len\n");
        output.push_str("    i32.ne\n");
        output.push_str("    if (result i64)\n");
        output.push_str("      i64.const 0\n");
        output.push_str("    else\n");
        output.push_str("      local.get $left\n");
        output.push_str("      i32.wrap_i64\n");
        output.push_str("      local.set $left_ptr\n");
        output.push_str("      local.get $right\n");
        output.push_str("      i32.wrap_i64\n");
        output.push_str("      local.set $right_ptr\n");
        output.push_str("      i32.const 0\n");
        output.push_str("      local.set $index\n");
        output.push_str("      i64.const 1\n");
        output.push_str("      local.set $equal\n");
        output.push_str("      block $done\n");
        output.push_str("        loop $loop\n");
        output.push_str("          local.get $index\n");
        output.push_str("          local.get $len\n");
        output.push_str("          i32.ge_u\n");
        output.push_str("          br_if $done\n");
        output.push_str("          local.get $left_ptr\n");
        output.push_str("          local.get $index\n");
        output.push_str("          i32.add\n");
        output.push_str("          i32.load8_u\n");
        output.push_str("          local.get $right_ptr\n");
        output.push_str("          local.get $index\n");
        output.push_str("          i32.add\n");
        output.push_str("          i32.load8_u\n");
        output.push_str("          i32.ne\n");
        output.push_str("          if\n");
        output.push_str("            i64.const 0\n");
        output.push_str("            local.set $equal\n");
        output.push_str("            br $done\n");
        output.push_str("          end\n");
        output.push_str("          local.get $index\n");
        output.push_str("          i32.const 1\n");
        output.push_str("          i32.add\n");
        output.push_str("          local.set $index\n");
        output.push_str("          br $loop\n");
        output.push_str("        end\n");
        output.push_str("      end\n");
        output.push_str("      local.get $equal\n");
        output.push_str("    end)\n\n");
    }

    fn emit_text_concat(&self, output: &mut String) {
        output.push_str("  (func $text_concat (param $left i64) (param $right i64) (result i64)\n");
        output.push_str("    (local $left_ptr i32)\n");
        output.push_str("    (local $right_ptr i32)\n");
        output.push_str("    (local $left_len i32)\n");
        output.push_str("    (local $right_len i32)\n");
        output.push_str("    (local $total_len i32)\n");
        output.push_str("    (local $ptr i32)\n");
        output.push_str("    (local $index i32)\n");
        output.push_str("    local.get $left\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $left_ptr\n");
        output.push_str("    local.get $right\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $right_ptr\n");
        output.push_str("    local.get $left\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shr_u\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $left_len\n");
        output.push_str("    local.get $right\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shr_u\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $right_len\n");
        output.push_str("    local.get $left_len\n");
        output.push_str("    local.get $right_len\n");
        output.push_str("    i32.add\n");
        output.push_str("    local.set $total_len\n");
        output.push_str("    local.get $total_len\n");
        output.push_str("    call $alloc\n");
        output.push_str("    local.set $ptr\n");
        output.push_str("    i32.const 0\n");
        output.push_str("    local.set $index\n");
        output.push_str("    block $left_done\n");
        output.push_str("      loop $left_loop\n");
        output.push_str("        local.get $index\n");
        output.push_str("        local.get $left_len\n");
        output.push_str("        i32.ge_u\n");
        output.push_str("        br_if $left_done\n");
        output.push_str("        local.get $ptr\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $left_ptr\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.add\n");
        output.push_str("        i32.load8_u\n");
        output.push_str("        i32.store8\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.const 1\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.set $index\n");
        output.push_str("        br $left_loop\n");
        output.push_str("      end\n");
        output.push_str("    end\n");
        output.push_str("    i32.const 0\n");
        output.push_str("    local.set $index\n");
        output.push_str("    block $right_done\n");
        output.push_str("      loop $right_loop\n");
        output.push_str("        local.get $index\n");
        output.push_str("        local.get $right_len\n");
        output.push_str("        i32.ge_u\n");
        output.push_str("        br_if $right_done\n");
        output.push_str("        local.get $ptr\n");
        output.push_str("        local.get $left_len\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $right_ptr\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.add\n");
        output.push_str("        i32.load8_u\n");
        output.push_str("        i32.store8\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.const 1\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.set $index\n");
        output.push_str("        br $right_loop\n");
        output.push_str("      end\n");
        output.push_str("    end\n");
        output.push_str("    local.get $total_len\n");
        output.push_str("    i64.extend_i32_u\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shl\n");
        output.push_str("    local.get $ptr\n");
        output.push_str("    i64.extend_i32_u\n");
        output.push_str("    i64.or)\n\n");
    }

    fn emit_text_slice(&self, output: &mut String) {
        output.push_str("  (func $text_slice (param $text i64) (param $start i64) (param $end i64) (result i64)\n");
        output.push_str("    (local $ptr i32)\n");
        output.push_str("    (local $len i32)\n");
        output.push_str("    (local $slice_start i32)\n");
        output.push_str("    (local $slice_end i32)\n");
        output.push_str("    (local $slice_len i32)\n");
        output.push_str("    (local $result_ptr i32)\n");
        output.push_str("    (local $index i32)\n");
        output.push_str("    local.get $text\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $ptr\n");
        output.push_str("    local.get $text\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shr_u\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    local.set $len\n");
        output.push_str("    local.get $start\n");
        output.push_str("    i64.const 0\n");
        output.push_str("    i64.lt_s\n");
        output.push_str("    if (result i32)\n");
        output.push_str("      i32.const 0\n");
        output.push_str("    else\n");
        output.push_str("      local.get $start\n");
        output.push_str("      i32.wrap_i64\n");
        output.push_str("    end\n");
        output.push_str("    local.set $slice_start\n");
        output.push_str("    local.get $slice_start\n");
        output.push_str("    local.get $len\n");
        output.push_str("    i32.gt_u\n");
        output.push_str("    if\n");
        output.push_str("      local.get $len\n");
        output.push_str("      local.set $slice_start\n");
        output.push_str("    end\n");
        output.push_str("    local.get $end\n");
        output.push_str("    i64.const 0\n");
        output.push_str("    i64.lt_s\n");
        output.push_str("    if (result i32)\n");
        output.push_str("      i32.const 0\n");
        output.push_str("    else\n");
        output.push_str("      local.get $end\n");
        output.push_str("      i32.wrap_i64\n");
        output.push_str("    end\n");
        output.push_str("    local.set $slice_end\n");
        output.push_str("    local.get $slice_end\n");
        output.push_str("    local.get $len\n");
        output.push_str("    i32.gt_u\n");
        output.push_str("    if\n");
        output.push_str("      local.get $len\n");
        output.push_str("      local.set $slice_end\n");
        output.push_str("    end\n");
        output.push_str("    block $start_done\n");
        output.push_str("      loop $start_loop\n");
        output.push_str("        local.get $slice_start\n");
        output.push_str("        local.get $len\n");
        output.push_str("        i32.ge_u\n");
        output.push_str("        br_if $start_done\n");
        output.push_str("        local.get $ptr\n");
        output.push_str("        local.get $slice_start\n");
        output.push_str("        i32.add\n");
        output.push_str("        i32.load8_u\n");
        output.push_str("        i32.const 192\n");
        output.push_str("        i32.and\n");
        output.push_str("        i32.const 128\n");
        output.push_str("        i32.ne\n");
        output.push_str("        br_if $start_done\n");
        output.push_str("        local.get $slice_start\n");
        output.push_str("        i32.const 1\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.set $slice_start\n");
        output.push_str("        br $start_loop\n");
        output.push_str("      end\n");
        output.push_str("    end\n");
        output.push_str("    block $end_done\n");
        output.push_str("      loop $end_loop\n");
        output.push_str("        local.get $slice_end\n");
        output.push_str("        local.get $len\n");
        output.push_str("        i32.ge_u\n");
        output.push_str("        br_if $end_done\n");
        output.push_str("        local.get $ptr\n");
        output.push_str("        local.get $slice_end\n");
        output.push_str("        i32.add\n");
        output.push_str("        i32.load8_u\n");
        output.push_str("        i32.const 192\n");
        output.push_str("        i32.and\n");
        output.push_str("        i32.const 128\n");
        output.push_str("        i32.ne\n");
        output.push_str("        br_if $end_done\n");
        output.push_str("        local.get $slice_end\n");
        output.push_str("        i32.const 1\n");
        output.push_str("        i32.sub\n");
        output.push_str("        local.set $slice_end\n");
        output.push_str("        br $end_loop\n");
        output.push_str("      end\n");
        output.push_str("    end\n");
        output.push_str("    local.get $slice_end\n");
        output.push_str("    local.get $slice_start\n");
        output.push_str("    i32.le_u\n");
        output.push_str("    if\n");
        output.push_str("      i32.const 0\n");
        output.push_str("      call $alloc\n");
        output.push_str("      local.set $result_ptr\n");
        output.push_str("      i64.const 0\n");
        output.push_str("      local.get $result_ptr\n");
        output.push_str("      i64.extend_i32_u\n");
        output.push_str("      i64.or\n");
        output.push_str("      return\n");
        output.push_str("    end\n");
        output.push_str("    local.get $slice_end\n");
        output.push_str("    local.get $slice_start\n");
        output.push_str("    i32.sub\n");
        output.push_str("    local.set $slice_len\n");
        output.push_str("    local.get $slice_len\n");
        output.push_str("    call $alloc\n");
        output.push_str("    local.set $result_ptr\n");
        output.push_str("    i32.const 0\n");
        output.push_str("    local.set $index\n");
        output.push_str("    block $copy_done\n");
        output.push_str("      loop $copy_loop\n");
        output.push_str("        local.get $index\n");
        output.push_str("        local.get $slice_len\n");
        output.push_str("        i32.ge_u\n");
        output.push_str("        br_if $copy_done\n");
        output.push_str("        local.get $result_ptr\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $ptr\n");
        output.push_str("        local.get $slice_start\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.add\n");
        output.push_str("        i32.load8_u\n");
        output.push_str("        i32.store8\n");
        output.push_str("        local.get $index\n");
        output.push_str("        i32.const 1\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.set $index\n");
        output.push_str("        br $copy_loop\n");
        output.push_str("      end\n");
        output.push_str("    end\n");
        output.push_str("    local.get $slice_len\n");
        output.push_str("    i64.extend_i32_u\n");
        output.push_str("    i64.const 32\n");
        output.push_str("    i64.shl\n");
        output.push_str("    local.get $result_ptr\n");
        output.push_str("    i64.extend_i32_u\n");
        output.push_str("    i64.or)\n\n");
    }

    fn emit_f64_vec_helpers(&self, output: &mut String) {
        output.push_str("  (func $f64_vec_new (param $len i64) (param $value i64) (result i64)\n");
        output.push_str("    (local $ptr i32) (local $idx i32)\n");
        output.push_str("    local.get $len\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    i32.const 8\n");
        output.push_str("    i32.mul\n");
        output.push_str("    i32.const 8\n");
        output.push_str("    i32.add\n");
        output.push_str("    call $alloc\n");
        output.push_str("    local.set $ptr\n");
        output.push_str("    local.get $ptr\n");
        output.push_str("    local.get $len\n");
        output.push_str("    i64.store\n");
        output.push_str("    i32.const 0\n");
        output.push_str("    local.set $idx\n");
        output.push_str("    block $done\n");
        output.push_str("      loop $loop\n");
        output.push_str("        local.get $idx\n");
        output.push_str("        local.get $len\n");
        output.push_str("        i32.wrap_i64\n");
        output.push_str("        i32.ge_u\n");
        output.push_str("        br_if $done\n");
        output.push_str("        local.get $ptr\n");
        output.push_str("        i32.const 8\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $idx\n");
        output.push_str("        i32.const 8\n");
        output.push_str("        i32.mul\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.get $value\n");
        output.push_str("        i64.store\n");
        output.push_str("        local.get $idx\n");
        output.push_str("        i32.const 1\n");
        output.push_str("        i32.add\n");
        output.push_str("        local.set $idx\n");
        output.push_str("        br $loop\n");
        output.push_str("      end\n");
        output.push_str("    end\n");
        output.push_str("    local.get $ptr\n");
        output.push_str("    i64.extend_i32_u\n");
        output.push_str("  )\n\n");
    }

    fn emit_structural_eq_helpers(&self, output: &mut String) -> Result<(), WasmError> {
        for record in self.records.values() {
            self.emit_record_eq(output, record)?;
        }
        for enum_ty in self.enums.values() {
            if !enum_is_payload_free(enum_ty) {
                self.emit_enum_eq(output, enum_ty)?;
            }
        }
        Ok(())
    }

    fn emit_record_eq(&self, output: &mut String, record: &WasmRecord) -> Result<(), WasmError> {
        writeln!(
            output,
            "  (func {} (param $left i64) (param $right i64) (result i64)",
            self.record_eq_function_name(&record.name)
        )
        .expect("writing to a string cannot fail");
        writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
        for field in &record.fields {
            writeln!(output, "    local.get $left").expect("writing to a string cannot fail");
            writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
            writeln!(output, "    i64.load offset={}", field.offset)
                .expect("writing to a string cannot fail");
            writeln!(output, "    local.get $right").expect("writing to a string cannot fail");
            writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
            writeln!(output, "    i64.load offset={}", field.offset)
                .expect("writing to a string cannot fail");
            self.emit_kind_eq(output, &field.kind, "    ")?;
            writeln!(output, "    i64.and").expect("writing to a string cannot fail");
        }
        output.push_str("  )\n\n");
        Ok(())
    }

    fn emit_enum_eq(&self, output: &mut String, enum_ty: &WasmEnum) -> Result<(), WasmError> {
        writeln!(
            output,
            "  (func {} (param $left i64) (param $right i64) (result i64)",
            self.enum_eq_function_name(&enum_ty.name)
        )
        .expect("writing to a string cannot fail");
        output.push_str("    (local $left_tag i64)\n");
        output.push_str("    (local $right_tag i64)\n");
        output.push_str("    local.get $left\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    i64.load\n");
        output.push_str("    local.set $left_tag\n");
        output.push_str("    local.get $right\n");
        output.push_str("    i32.wrap_i64\n");
        output.push_str("    i64.load\n");
        output.push_str("    local.set $right_tag\n");
        output.push_str("    local.get $left_tag\n");
        output.push_str("    local.get $right_tag\n");
        output.push_str("    i64.eq\n");
        output.push_str("    if (result i64)\n");
        let payload_variants = enum_ty
            .variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| variant.payload.is_some())
            .collect::<Vec<_>>();
        self.emit_enum_payload_eq_chain(output, &payload_variants, 0, "      ")?;
        output.push_str("    else\n");
        output.push_str("      i64.const 0\n");
        output.push_str("    end)\n\n");
        Ok(())
    }

    fn emit_enum_payload_eq_chain(
        &self,
        output: &mut String,
        payload_variants: &[(usize, &WasmEnumVariant)],
        index: usize,
        indent: &str,
    ) -> Result<(), WasmError> {
        let Some((variant_index, variant)) = payload_variants.get(index).copied() else {
            writeln!(output, "{indent}i64.const 1").expect("writing to a string cannot fail");
            return Ok(());
        };
        let payload_kind = variant
            .payload
            .as_ref()
            .ok_or_else(|| WasmError::new("payload equality helper requires payload metadata"))?;
        writeln!(output, "{indent}local.get $left_tag").expect("writing to a string cannot fail");
        writeln!(output, "{indent}i64.const {variant_index}")
            .expect("writing to a string cannot fail");
        writeln!(output, "{indent}i64.eq").expect("writing to a string cannot fail");
        writeln!(output, "{indent}if (result i64)").expect("writing to a string cannot fail");
        let nested = format!("{indent}  ");
        writeln!(output, "{nested}local.get $left").expect("writing to a string cannot fail");
        writeln!(output, "{nested}i32.wrap_i64").expect("writing to a string cannot fail");
        writeln!(output, "{nested}i64.load offset=8").expect("writing to a string cannot fail");
        writeln!(output, "{nested}local.get $right").expect("writing to a string cannot fail");
        writeln!(output, "{nested}i32.wrap_i64").expect("writing to a string cannot fail");
        writeln!(output, "{nested}i64.load offset=8").expect("writing to a string cannot fail");
        self.emit_kind_eq(output, payload_kind, &nested)?;
        writeln!(output, "{indent}else").expect("writing to a string cannot fail");
        self.emit_enum_payload_eq_chain(output, payload_variants, index + 1, &nested)?;
        writeln!(output, "{indent}end").expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_kind_eq(
        &self,
        output: &mut String,
        kind: &WasmValueKind,
        indent: &str,
    ) -> Result<(), WasmError> {
        match kind {
            WasmValueKind::I32 | WasmValueKind::F64 | WasmValueKind::Bool => {
                writeln!(output, "{indent}i64.eq").expect("writing to a string cannot fail");
                writeln!(output, "{indent}i64.extend_i32_u")
                    .expect("writing to a string cannot fail");
            }
            WasmValueKind::TextBuilder => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text builder equality in stage-0",
                ));
            }
            WasmValueKind::F64Vec => {
                writeln!(output, "{indent}i64.eq").expect("writing to a string cannot fail");
            }
            WasmValueKind::Text => {
                writeln!(output, "{indent}call $text_eq").expect("writing to a string cannot fail");
            }
            WasmValueKind::Record(name) => {
                if !self.records.contains_key(name) {
                    return Err(WasmError::new(format!(
                        "missing wasm record metadata for `{name}`"
                    )));
                }
                writeln!(
                    output,
                    "{indent}call {}",
                    self.record_eq_function_name(name)
                )
                .expect("writing to a string cannot fail");
            }
            WasmValueKind::Enum(name) => {
                let enum_ty = self.enums.get(name).ok_or_else(|| {
                    WasmError::new(format!("missing wasm enum metadata for `{name}`"))
                })?;
                if enum_is_payload_free(enum_ty) {
                    writeln!(output, "{indent}i64.eq").expect("writing to a string cannot fail");
                    writeln!(output, "{indent}i64.extend_i32_u")
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "{indent}call {}", self.enum_eq_function_name(name))
                        .expect("writing to a string cannot fail");
                }
            }
        }
        Ok(())
    }

    fn record_eq_function_name(&self, name: &str) -> String {
        format!("$record_eq_{}", sanitize_wasm_symbol(name))
    }

    fn enum_eq_function_name(&self, name: &str) -> String {
        format!("$enum_eq_{}", sanitize_wasm_symbol(name))
    }

    fn emit_const_text(
        &self,
        output: &mut String,
        dest: ValueId,
        value: &str,
    ) -> Result<(), WasmError> {
        let len = u32::try_from(value.len())
            .map_err(|_| WasmError::new("wasm text literal exceeds 32-bit limits"))?;
        writeln!(output, "    i32.const {len}").expect("writing to a string cannot fail");
        writeln!(output, "    call $alloc").expect("writing to a string cannot fail");
        writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
        writeln!(output, "    local.set ${}", dest.render())
            .expect("writing to a string cannot fail");
        for (index, byte) in value.as_bytes().iter().copied().enumerate() {
            writeln!(output, "    local.get ${}", dest.render())
                .expect("writing to a string cannot fail");
            writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
            writeln!(output, "    i32.const {index}").expect("writing to a string cannot fail");
            writeln!(output, "    i32.add").expect("writing to a string cannot fail");
            writeln!(output, "    i32.const {byte}").expect("writing to a string cannot fail");
            writeln!(output, "    i32.store8").expect("writing to a string cannot fail");
        }
        writeln!(output, "    local.get ${}", dest.render())
            .expect("writing to a string cannot fail");
        writeln!(output, "    i64.const {len}").expect("writing to a string cannot fail");
        writeln!(output, "    i64.const 32").expect("writing to a string cannot fail");
        writeln!(output, "    i64.shl").expect("writing to a string cannot fail");
        writeln!(output, "    i64.or").expect("writing to a string cannot fail");
        writeln!(output, "    local.set ${}", dest.render())
            .expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_make_enum(
        &self,
        output: &mut String,
        function: &Function,
        dest: ValueId,
        name: &str,
        variant: &str,
        payload: Option<ValueId>,
    ) -> Result<(), WasmError> {
        let enum_ty = self
            .enums
            .get(name)
            .ok_or_else(|| WasmError::new(format!("missing wasm enum metadata for `{name}`")))?;
        let tag = enum_ty
            .variants
            .iter()
            .position(|candidate| candidate.name == variant)
            .ok_or_else(|| {
                WasmError::new(format!(
                    "enum `{name}` has no variant `{variant}` in `{}`",
                    function.name
                ))
            })?;
        let tag = i64::try_from(tag).expect("enum tag should fit i64");
        if enum_is_payload_free(enum_ty) {
            writeln!(output, "    i64.const {tag}").expect("writing to a string cannot fail");
            writeln!(output, "    local.set ${}", dest.render())
                .expect("writing to a string cannot fail");
            return Ok(());
        }

        writeln!(output, "    i32.const {PAYLOAD_ENUM_SIZE}")
            .expect("writing to a string cannot fail");
        writeln!(output, "    call $alloc").expect("writing to a string cannot fail");
        writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
        writeln!(output, "    local.tee ${}", dest.render())
            .expect("writing to a string cannot fail");
        writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
        writeln!(output, "    i64.const {tag}").expect("writing to a string cannot fail");
        writeln!(output, "    i64.store").expect("writing to a string cannot fail");
        writeln!(output, "    local.get ${}", dest.render())
            .expect("writing to a string cannot fail");
        writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
        match (
            enum_ty
                .variants
                .iter()
                .find(|candidate| candidate.name == variant)
                .and_then(|candidate| candidate.payload.as_ref()),
            payload,
        ) {
            (Some(_), Some(payload)) => {
                writeln!(output, "    local.get ${}", payload.render())
                    .expect("writing to a string cannot fail");
            }
            (None, None) => {
                writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
            }
            (Some(_), None) => {
                return Err(WasmError::new(format!(
                    "enum `{name}.{variant}` is missing its payload in `{}`",
                    function.name
                )));
            }
            (None, Some(_)) => {
                return Err(WasmError::new(format!(
                    "enum `{name}.{variant}` unexpectedly carries a payload in `{}`",
                    function.name
                )));
            }
        }
        writeln!(output, "    i64.store offset=8").expect("writing to a string cannot fail");
        Ok(())
    }

    fn emit_function(&self, output: &mut String, function: &Function) -> Result<(), WasmError> {
        let signature = &self.signatures[&function.name];
        write!(output, "  (func ${}", function.name).expect("writing to a string cannot fail");
        if !function.name.is_empty() {
            output.push_str(" (export \"");
            output.push_str(&function.name);
            output.push_str("\")");
        }

        for (index, _) in signature.params.iter().enumerate() {
            write!(output, " (param $p{index} i64)").expect("writing to a string cannot fail");
        }
        if let Some(result) = signature.result {
            write!(output, " (result {})", result.render())
                .expect("writing to a string cannot fail");
        }
        output.push('\n');

        let mut locals = BTreeMap::<ValueId, WasmType>::new();
        let mut repeat_locals = Vec::new();
        for local in &function.mutable_locals {
            writeln!(output, "    (local ${} i64)", local.slot.render())
                .expect("writing to a string cannot fail");
        }

        self.collect_locals(function, &function.instructions, &mut locals);
        self.collect_repeat_locals(&function.instructions, &mut repeat_locals);
        for (id, ty) in &locals {
            writeln!(output, "    (local ${} {})", id.render(), ty.render())
                .expect("writing to a string cannot fail");
        }
        for local in &repeat_locals {
            writeln!(output, "    (local {local} i64)").expect("writing to a string cannot fail");
        }

        self.emit_insts(output, function, &function.instructions, &locals)?;

        if let Some(result) = function.result {
            writeln!(output, "    local.get ${}", result.render())
                .expect("writing to a string cannot fail");
        }

        output.push_str("  )\n\n");
        Ok(())
    }

    fn collect_locals(
        &self,
        function: &Function,
        instructions: &[Inst],
        locals: &mut BTreeMap<ValueId, WasmType>,
    ) {
        for inst in instructions {
            match inst {
                Inst::LoadParam { dest, .. }
                | Inst::LoadLocal { dest, .. }
                | Inst::ConstInt { dest, .. }
                | Inst::ConstF64 { dest, .. }
                | Inst::ConstBool { dest, .. }
                | Inst::ConstText { dest, .. }
                | Inst::TextBuilderNew { dest, .. }
                | Inst::TextBuilderAppend { dest, .. }
                | Inst::F64VecNew { dest, .. }
                | Inst::F64VecLen { dest, .. }
                | Inst::F64VecGet { dest, .. }
                | Inst::F64VecSet { dest, .. }
                | Inst::F64FromI32 { dest, .. }
                | Inst::TextLen { dest, .. }
                | Inst::TextConcat { dest, .. }
                | Inst::TextSlice { dest, .. }
                | Inst::TextByte { dest, .. }
                | Inst::TextBuilderFinish { dest, .. }
                | Inst::TextFromF64Fixed { dest, .. }
                | Inst::ParseI32 { dest, .. }
                | Inst::ArgCount { dest, .. }
                | Inst::ArgText { dest, .. }
                | Inst::StdinText { dest, .. }
                | Inst::MakeEnum { dest, .. }
                | Inst::MakeRecord { dest, .. }
                | Inst::Field { dest, .. }
                | Inst::EnumTagEq { dest, .. }
                | Inst::EnumPayload { dest, .. }
                | Inst::Add { dest, .. }
                | Inst::Sub { dest, .. }
                | Inst::Mul { dest, .. }
                | Inst::Div { dest, .. }
                | Inst::Sqrt { dest, .. }
                | Inst::And { dest, .. }
                | Inst::Or { dest, .. }
                | Inst::Eq { dest, .. }
                | Inst::Ne { dest, .. }
                | Inst::Lt { dest, .. }
                | Inst::Le { dest, .. }
                | Inst::Gt { dest, .. }
                | Inst::Ge { dest, .. }
                | Inst::Call { dest, .. }
                | Inst::If { dest, .. }
                | Inst::While { dest, .. }
                | Inst::Repeat { dest, .. } => {
                    locals.insert(*dest, WasmType::I64);
                }
                Inst::StoreLocal { .. } | Inst::Assert { .. } | Inst::StdoutWrite { .. } => {}
            }

            match inst {
                Inst::If {
                    then_insts,
                    else_insts,
                    ..
                } => {
                    self.collect_locals(function, then_insts, locals);
                    self.collect_locals(function, else_insts, locals);
                }
                Inst::While {
                    condition_insts,
                    body_insts,
                    ..
                } => {
                    self.collect_locals(function, condition_insts, locals);
                    self.collect_locals(function, body_insts, locals);
                }
                Inst::Repeat { body_insts, .. } => {
                    self.collect_locals(function, body_insts, locals);
                }
                _ => {}
            }
        }
    }

    fn collect_repeat_locals(&self, instructions: &[Inst], locals: &mut Vec<String>) {
        for inst in instructions {
            match inst {
                Inst::If {
                    then_insts,
                    else_insts,
                    ..
                } => {
                    self.collect_repeat_locals(then_insts, locals);
                    self.collect_repeat_locals(else_insts, locals);
                }
                Inst::While {
                    condition_insts,
                    body_insts,
                    ..
                } => {
                    self.collect_repeat_locals(condition_insts, locals);
                    self.collect_repeat_locals(body_insts, locals);
                }
                Inst::Repeat {
                    dest,
                    index_slot,
                    body_insts,
                    ..
                } => {
                    locals.push(repeat_counter_local(*dest));
                    if index_slot.is_none() {
                        locals.push(repeat_index_local(*dest));
                    }
                    self.collect_repeat_locals(body_insts, locals);
                }
                _ => {}
            }
        }
    }

    fn emit_insts(
        &self,
        output: &mut String,
        function: &Function,
        instructions: &[Inst],
        locals: &BTreeMap<ValueId, WasmType>,
    ) -> Result<(), WasmError> {
        for inst in instructions {
            self.emit_inst(output, function, inst, locals)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn emit_inst(
        &self,
        output: &mut String,
        function: &Function,
        inst: &Inst,
        locals: &BTreeMap<ValueId, WasmType>,
    ) -> Result<(), WasmError> {
        match inst {
            Inst::LoadParam { dest, index } => {
                writeln!(output, "    local.get $p{index}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::LoadLocal { dest, slot } => {
                writeln!(output, "    local.get ${}", slot.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::StoreLocal { slot, src } => {
                writeln!(output, "    local.get ${}", src.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", slot.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstInt { dest, value } => {
                writeln!(output, "    i64.const {value}").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstBool { dest, value } => {
                writeln!(output, "    i64.const {}", if *value { 1 } else { 0 })
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::F64VecNew { dest, len, value } => {
                writeln!(output, "    local.get ${}", len.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", value.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $f64_vec_new").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::F64VecLen { dest, vec } => {
                writeln!(output, "    local.get ${}", vec.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i64.load").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::F64VecGet { dest, vec, index } => {
                writeln!(output, "    local.get ${}", vec.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const 8").expect("writing to a string cannot fail");
                writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", index.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const 8").expect("writing to a string cannot fail");
                writeln!(output, "    i32.mul").expect("writing to a string cannot fail");
                writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                writeln!(output, "    i64.load").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::F64VecSet {
                dest,
                vec,
                index,
                value,
            } => {
                writeln!(output, "    local.get ${}", vec.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const 8").expect("writing to a string cannot fail");
                writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", index.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i32.const 8").expect("writing to a string cannot fail");
                writeln!(output, "    i32.mul").expect("writing to a string cannot fail");
                writeln!(output, "    i32.add").expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", value.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.store").expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", vec.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::F64FromI32 { dest, value } => {
                writeln!(output, "    local.get ${}", value.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    f64.convert_i64_s").expect("writing to a string cannot fail");
                writeln!(output, "    i64.reinterpret_f64")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstText { dest, value } => {
                self.emit_const_text(output, *dest, value)?;
            }
            Inst::TextBuilderNew { .. }
            | Inst::TextBuilderAppend { .. }
            | Inst::TextBuilderFinish { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text builder builtins in stage-0",
                ));
            }
            Inst::TextLen { dest, text } => {
                writeln!(output, "    local.get ${}", text.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 32").expect("writing to a string cannot fail");
                writeln!(output, "    i64.shr_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::TextConcat { dest, left, right } => {
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", right.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $text_concat").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::TextSlice {
                dest,
                text,
                start,
                end,
            } => {
                writeln!(output, "    local.get ${}", text.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", start.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", end.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $text_slice").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::TextByte { dest, text, index } => {
                writeln!(output, "    local.get ${}", index.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", text.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 32").expect("writing to a string cannot fail");
                writeln!(output, "    i64.shr_u").expect("writing to a string cannot fail");
                writeln!(output, "    i64.lt_u").expect("writing to a string cannot fail");
                writeln!(output, "    if (result i64)").expect("writing to a string cannot fail");
                writeln!(output, "      local.get ${}", text.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "      i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "      local.get ${}", index.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "      i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "      i32.add").expect("writing to a string cannot fail");
                writeln!(output, "      i64.load8_u").expect("writing to a string cannot fail");
                writeln!(output, "    else").expect("writing to a string cannot fail");
                writeln!(output, "      i64.const 0").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstF64 { dest, bits } => {
                writeln!(output, "    i64.const {}", *bits as i64)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::TextFromF64Fixed { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support `text_from_f64_fixed` in stage-0",
                ));
            }
            Inst::Sqrt { dest, value } => {
                writeln!(output, "    local.get ${}", value.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    f64.reinterpret_i64")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    f64.sqrt").expect("writing to a string cannot fail");
                writeln!(output, "    i64.reinterpret_f64")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::ArgCount { .. } | Inst::ArgText { .. } | Inst::StdinText { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support runtime input builtins in stage-0",
                ));
            }
            Inst::StdoutWrite { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support runtime io builtins in stage-0",
                ));
            }
            Inst::ParseI32 { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support `parse_i32` in stage-0",
                ));
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
                writeln!(output, "    local.tee ${}", dest.render())
                    .expect("writing to a string cannot fail");
                let dest_local = format!("${}", dest.render());
                for field in &record.fields {
                    let source = fields
                        .iter()
                        .find_map(|(f, v)| (f == &field.name).then_some(*v))
                        .ok_or_else(|| {
                            WasmError::new(format!(
                                "record `{}` is missing field `{}` in `{}`",
                                name, field.name, function.name
                            ))
                        })?;
                    writeln!(output, "    local.get {dest_local}")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", source.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.store offset={}", field.offset)
                        .expect("writing to a string cannot fail");
                }
                writeln!(output, "    drop").expect("writing to a string cannot fail");
            }
            Inst::Field { dest, base, name } => {
                let base_kind = self.infer_value_kind(*base, function)?;
                let WasmValueKind::Record(record_name) = base_kind else {
                    return Err(WasmError::new("expected record kind for field access"));
                };
                let record = &self.records[&record_name];
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
                writeln!(output, "    local.get ${}", base.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i64.load offset={}", field.offset)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::EnumTagEq { dest, value, tag } => {
                let left = match self.infer_value_kind(*value, function) {
                    Ok(WasmValueKind::Enum(enum_name)) => {
                        let enum_ty = self.enums.get(&enum_name).ok_or_else(|| {
                            WasmError::new(format!("missing wasm enum metadata for `{enum_name}`"))
                        })?;
                        if enum_is_payload_free(enum_ty) {
                            format!("    local.get ${}", value.render())
                        } else {
                            format!(
                                "    local.get ${}\n    i32.wrap_i64\n    i64.load",
                                value.render()
                            )
                        }
                    }
                    _ => format!("    local.get ${}", value.render()),
                };
                writeln!(output, "{left}").expect("writing to a string cannot fail");
                writeln!(output, "    i64.const {tag}").expect("writing to a string cannot fail");
                writeln!(output, "    i64.eq").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::EnumPayload { dest, value, .. } => {
                let enum_kind = self.infer_value_kind(*value, function)?;
                let WasmValueKind::Enum(enum_name) = enum_kind else {
                    return Err(WasmError::new("expected enum kind for payload extraction"));
                };
                let enum_ty = self.enums.get(&enum_name).ok_or_else(|| {
                    WasmError::new(format!("missing wasm enum metadata for `{enum_name}`"))
                })?;
                if enum_is_payload_free(enum_ty) {
                    return Err(WasmError::new(format!(
                        "cannot extract a payload from payload-free enum `{enum_name}` in `{}`",
                        function.name
                    )));
                }
                writeln!(output, "    local.get ${}", value.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    i64.load offset=8").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Add { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.add").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.reinterpret_f64")
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.add").expect("writing to a string cannot fail");
                }
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Sub { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.sub").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.reinterpret_f64")
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.sub").expect("writing to a string cannot fail");
                }
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Mul { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.mul").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.reinterpret_f64")
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.mul").expect("writing to a string cannot fail");
                }
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Div { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.div").expect("writing to a string cannot fail");
                    writeln!(output, "    i64.reinterpret_f64")
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.div_s").expect("writing to a string cannot fail");
                }
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::And { dest, left, right } => {
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", right.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.and").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Or { dest, left, right } => {
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", right.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.or").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Eq { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", right.render())
                    .expect("writing to a string cannot fail");
                self.emit_kind_eq(output, &kind, "    ")?;
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Ne { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", right.render())
                    .expect("writing to a string cannot fail");
                self.emit_kind_eq(output, &kind, "    ")?;
                writeln!(output, "    i64.eqz").expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Lt { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.lt").expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.lt_s").expect("writing to a string cannot fail");
                }
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Le { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.le").expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.le_s").expect("writing to a string cannot fail");
                }
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Gt { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.gt").expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.gt_s").expect("writing to a string cannot fail");
                }
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Ge { dest, left, right } => {
                let kind = self.infer_value_kind(*left, function)?;
                writeln!(output, "    local.get ${}", left.render())
                    .expect("writing to a string cannot fail");
                if kind == WasmValueKind::F64 {
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.reinterpret_i64")
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    f64.ge").expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    local.get ${}", right.render())
                        .expect("writing to a string cannot fail");
                    writeln!(output, "    i64.ge_s").expect("writing to a string cannot fail");
                }
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Call { dest, callee, args } => {
                for arg in args {
                    writeln!(output, "    local.get ${}", arg.render())
                        .expect("writing to a string cannot fail");
                }
                writeln!(output, "    call ${callee}").expect("writing to a string cannot fail");
                if self.signatures[callee].result.is_some() {
                    writeln!(output, "    local.set ${}", dest.render())
                        .expect("writing to a string cannot fail");
                } else {
                    writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                    writeln!(output, "    local.set ${}", dest.render())
                        .expect("writing to a string cannot fail");
                }
            }
            Inst::If {
                dest,
                condition,
                then_insts,
                then_result,
                else_insts,
                else_result,
            } => {
                writeln!(output, "    local.get ${}", condition.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i32.wrap_i64").expect("writing to a string cannot fail");
                writeln!(output, "    if (result i64)").expect("writing to a string cannot fail");
                self.emit_insts(output, function, then_insts, locals)?;
                if let Some(res) = then_result {
                    writeln!(output, "    local.get ${}", res.render())
                        .expect("writing to a string cannot fail");
                } else {
                    output.push_str("    i64.const 0\n");
                }
                output.push_str("    else\n");
                self.emit_insts(output, function, else_insts, locals)?;
                if let Some(res) = else_result {
                    writeln!(output, "    local.get ${}", res.render())
                        .expect("writing to a string cannot fail");
                } else {
                    output.push_str("    i64.const 0\n");
                }
                output.push_str("    end\n");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Repeat {
                dest,
                count,
                index_slot,
                body_insts,
            } => {
                let loop_label = format!("$repeat_{}", dest.render());
                let counter_local = repeat_counter_local(*dest);
                let index_local = index_slot
                    .map_or_else(|| repeat_index_local(*dest), |s| format!("${}", s.render()));

                writeln!(output, "    local.get ${}", count.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set {counter_local}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                writeln!(output, "    local.set {index_local}")
                    .expect("writing to a string cannot fail");

                writeln!(output, "    block $exit_{}", dest.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    loop {loop_label}").expect("writing to a string cannot fail");

                writeln!(output, "    local.get {counter_local}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                writeln!(output, "    i64.le_s").expect("writing to a string cannot fail");
                writeln!(output, "    br_if $exit_{}", dest.render())
                    .expect("writing to a string cannot fail");

                self.emit_insts(output, function, body_insts, locals)?;

                writeln!(output, "    local.get {counter_local}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
                writeln!(output, "    i64.sub").expect("writing to a string cannot fail");
                writeln!(output, "    local.set {counter_local}")
                    .expect("writing to a string cannot fail");

                writeln!(output, "    local.get {index_local}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.const 1").expect("writing to a string cannot fail");
                writeln!(output, "    i64.add").expect("writing to a string cannot fail");
                writeln!(output, "    local.set {index_local}")
                    .expect("writing to a string cannot fail");

                writeln!(output, "    br {loop_label}").expect("writing to a string cannot fail");
                output.push_str("    end\n");
                output.push_str("    end\n");
                writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::While {
                dest,
                condition_insts,
                condition,
                body_insts,
            } => {
                let loop_label = format!("$while_{}", dest.render());
                let exit_label = format!("$exit_{}", dest.render());
                writeln!(output, "    block {exit_label}")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    loop {loop_label}").expect("writing to a string cannot fail");
                self.emit_insts(output, function, condition_insts, locals)?;
                writeln!(output, "    local.get ${}", condition.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.eqz").expect("writing to a string cannot fail");
                writeln!(output, "    br_if {exit_label}")
                    .expect("writing to a string cannot fail");
                self.emit_insts(output, function, body_insts, locals)?;
                writeln!(output, "    br {loop_label}").expect("writing to a string cannot fail");
                output.push_str("    end\n");
                output.push_str("    end\n");
                writeln!(output, "    i64.const 0").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", dest.render())
                    .expect("writing to a string cannot fail");
            }
            Inst::Assert { condition, .. } => {
                writeln!(output, "    local.get ${}", condition.render())
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.eqz").expect("writing to a string cannot fail");
                writeln!(output, "    if").expect("writing to a string cannot fail");
                writeln!(output, "      unreachable").expect("writing to a string cannot fail");
                writeln!(output, "    end").expect("writing to a string cannot fail");
            }
        }
        Ok(())
    }

    fn infer_value_kind(
        &self,
        id: ValueId,
        function: &Function,
    ) -> Result<WasmValueKind, WasmError> {
        let kinds = infer_value_kinds(
            function,
            &self.records,
            &self.enums,
            &self.program.functions,
        )?;
        kinds
            .get(&id)
            .cloned()
            .ok_or_else(|| WasmError::new("failed to infer value kind"))
    }
}

fn wasm_value_kind(ty: &str, program: &Program) -> Result<WasmValueKind, WasmError> {
    match ty {
        "I32" => Ok(WasmValueKind::I32),
        "F64" => Ok(WasmValueKind::F64),
        "Bool" => Ok(WasmValueKind::Bool),
        "Text" => Ok(WasmValueKind::Text),
        other => {
            if program.enums.iter().any(|e| e.name == other) {
                Ok(WasmValueKind::Enum(other.to_owned()))
            } else if program.structs.iter().any(|s| s.name == other) {
                Ok(WasmValueKind::Record(other.to_owned()))
            } else {
                Err(WasmError::new(format!("unsupported wasm type `{other}`")))
            }
        }
    }
}

fn wasm_signature(function: &Function) -> Result<WasmSignature, WasmError> {
    let mut params = Vec::new();
    for _ in &function.params {
        params.push(WasmType::I64);
    }
    let result = function.return_type.as_deref().map(|_| WasmType::I64);
    Ok(WasmSignature { params, result })
}

pub(crate) fn enum_is_payload_free(enum_ty: &WasmEnum) -> bool {
    enum_ty.variants.iter().all(|v| v.payload.is_none())
}

fn repeat_local_suffix(id: ValueId) -> String {
    id.render()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>()
}

fn repeat_counter_local(id: ValueId) -> String {
    format!("$repeat_count_{}", repeat_local_suffix(id))
}

fn repeat_index_local(id: ValueId) -> String {
    format!("$repeat_index_{}", repeat_local_suffix(id))
}

fn sanitize_wasm_symbol(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn infer_value_kinds(
    function: &Function,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    all_functions: &[Function],
) -> Result<BTreeMap<ValueId, WasmValueKind>, WasmError> {
    let mut kinds = BTreeMap::new();
    for (index, param) in function.params.iter().enumerate() {
        kinds.insert(
            ValueId(index as u32),
            wasm_value_kind_from_name(&param.ty, records, enums)?,
        );
    }
    collect_inst_kinds(
        function,
        &function.instructions,
        records,
        enums,
        all_functions,
        &mut kinds,
    )?;
    Ok(kinds)
}

fn wasm_value_kind_from_name(
    name: &str,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
) -> Result<WasmValueKind, WasmError> {
    match name {
        "I32" => Ok(WasmValueKind::I32),
        "F64" => Ok(WasmValueKind::F64),
        "Bool" => Ok(WasmValueKind::Bool),
        "Text" => Ok(WasmValueKind::Text),
        other => {
            if enums.contains_key(other) {
                Ok(WasmValueKind::Enum(other.to_owned()))
            } else if records.contains_key(other) {
                Ok(WasmValueKind::Record(other.to_owned()))
            } else {
                Err(WasmError::new(format!("unknown wasm type `{other}`")))
            }
        }
    }
}

fn collect_inst_kinds(
    function: &Function,
    instructions: &[Inst],
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    all_functions: &[Function],
    kinds: &mut BTreeMap<ValueId, WasmValueKind>,
) -> Result<(), WasmError> {
    for inst in instructions {
        match inst {
            Inst::LoadParam { dest, index } => {
                let ty = &function.params[*index].ty;
                kinds.insert(*dest, wasm_value_kind_from_name(ty, records, enums)?);
            }
            Inst::LoadLocal { dest, slot } => {
                let ty = function
                    .mutable_local_type(*slot)
                    .expect("mutable local type should be available");
                kinds.insert(*dest, wasm_value_kind_from_name(ty, records, enums)?);
            }
            Inst::ConstInt { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::ConstF64 { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::F64);
            }
            Inst::ConstBool { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bool);
            }
            Inst::ConstText { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::TextBuilderNew { .. }
            | Inst::TextBuilderAppend { .. }
            | Inst::TextBuilderFinish { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text builder builtins in stage-0",
                ));
            }
            Inst::F64VecNew { dest, .. } | Inst::F64VecSet { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::F64Vec);
            }
            Inst::F64VecLen { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::F64VecGet { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::F64);
            }
            Inst::TextConcat { dest, .. }
            | Inst::TextSlice { dest, .. }
            | Inst::TextFromF64Fixed { dest, .. }
            | Inst::ArgText { dest, .. }
            | Inst::StdinText { dest } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::StdoutWrite { .. } => {}
            Inst::TextLen { dest, .. }
            | Inst::TextByte { dest, .. }
            | Inst::ArgCount { dest, .. }
            | Inst::ParseI32 { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::I32);
            }
            Inst::Sqrt { dest, .. } | Inst::F64FromI32 { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::F64);
            }
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
                let record = &records[&record_name];
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
                kinds.insert(*dest, field.kind.clone());
            }
            Inst::EnumTagEq { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bool);
            }
            Inst::EnumPayload { dest, value, .. } => {
                let WasmValueKind::Enum(enum_name) = kinds[value].clone() else {
                    return Err(WasmError::new("expected enum kind for payload extraction"));
                };
                let enum_ty = &enums[&enum_name];
                let payload = enum_ty
                    .variants
                    .iter()
                    .find_map(|v| v.payload.clone())
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "enum `{enum_name}` has no payload for payload extraction in `{}`",
                            function.name
                        ))
                    })?;
                kinds.insert(*dest, payload);
            }
            Inst::Add { dest, left, .. }
            | Inst::Sub { dest, left, .. }
            | Inst::Mul { dest, left, .. }
            | Inst::Div { dest, left, .. } => {
                let kind = kinds
                    .get(left)
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "arithmetic input `{}` has unknown kind in `{}`",
                            left.render(),
                            function.name
                        ))
                    })?
                    .clone();
                kinds.insert(*dest, kind);
            }
            Inst::And { dest, .. }
            | Inst::Or { dest, .. }
            | Inst::Eq { dest, .. }
            | Inst::Ne { dest, .. }
            | Inst::Lt { dest, .. }
            | Inst::Le { dest, .. }
            | Inst::Gt { dest, .. }
            | Inst::Ge { dest, .. } => {
                kinds.insert(*dest, WasmValueKind::Bool);
            }
            Inst::Call { dest, callee, .. } => {
                let callee_fn = all_functions
                    .iter()
                    .find(|f| f.name == *callee)
                    .ok_or_else(|| {
                        WasmError::new(format!("missing callee `{callee}` in `{}`", function.name))
                    })?;
                if let Some(ty) = &callee_fn.return_type {
                    kinds.insert(*dest, wasm_value_kind_from_name(ty, records, enums)?);
                } else {
                    kinds.insert(*dest, WasmValueKind::I32); // Unit as 0
                }
            }
            Inst::If {
                dest,
                then_insts,
                else_insts,
                then_result,
                else_result,
                ..
            } => {
                collect_inst_kinds(function, then_insts, records, enums, all_functions, kinds)?;
                collect_inst_kinds(function, else_insts, records, enums, all_functions, kinds)?;
                let kind = if let Some(res) = then_result {
                    kinds[res].clone()
                } else if let Some(res) = else_result {
                    kinds[res].clone()
                } else {
                    WasmValueKind::I32 // Unit
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
                    records,
                    enums,
                    all_functions,
                    kinds,
                )?;
                collect_inst_kinds(function, body_insts, records, enums, all_functions, kinds)?;
                kinds.insert(*dest, WasmValueKind::I32); // Unit
            }
            Inst::Repeat {
                dest, body_insts, ..
            } => {
                collect_inst_kinds(function, body_insts, records, enums, all_functions, kinds)?;
                kinds.insert(*dest, WasmValueKind::I32); // Unit
            }
            Inst::StoreLocal { .. } | Inst::Assert { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    use crate::{
        RuntimeEnum, RuntimeRecord, RuntimeValue, lower, run_function_wasm, run_main, run_main_wasm,
    };

    fn lower_source(source: &str) -> crate::MirLowering {
        let lexed = lex(source);
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        lower(&hir.module)
    }

    fn assert_main_wasm_equivalence(source: &str) -> RuntimeValue {
        let mir = lower_source(source);
        let interpreted = run_main(&mir.program).expect("interpreter should run");
        let result = run_main_wasm(&mir.program).expect("wasm should run");
        assert_eq!(interpreted, result);
        result
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_integer_programs() {
        let source = "fn add(left: I32, right: I32) -> I32 { left + right }\nfn main() -> I32 { add(20, 22) }";
        let mir = lower_source(source);
        let interpreted = assert_main_wasm_equivalence(source);
        let wasm = crate::wasm::emit_wasm(&mir.program).expect("wasm emission should work");
        assert!(wasm.starts_with(b"\0asm"));
        assert_eq!(interpreted, RuntimeValue::Int(42));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_text_programs() {
        let interpreted = assert_main_wasm_equivalence("fn main() -> Text { \"hello\" }");
        assert_eq!(interpreted, RuntimeValue::Text("hello".to_owned()));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_text_concat_programs() {
        let interpreted =
            assert_main_wasm_equivalence("fn main() -> Text { text_concat(\"sa\", \"rif\") }");
        assert_eq!(interpreted, RuntimeValue::Text("sarif".to_owned()));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_text_slice_programs() {
        let interpreted = assert_main_wasm_equivalence(
            "fn main() -> Bool { text_slice(\"sarif\", 1, 4) == \"ari\" and text_slice(\"sarif\", 3, 99) == \"if\" and text_slice(\"sarif\", 4, 2) == \"\" }",
        );
        assert_eq!(interpreted, RuntimeValue::Bool(true));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_boolean_programs() {
        let interpreted =
            assert_main_wasm_equivalence("fn main() -> Bool { (1 + 2 == 3) and (4 > 1) }");
        assert_eq!(interpreted, RuntimeValue::Bool(true));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_text_builtins() {
        let interpreted = assert_main_wasm_equivalence(
            "fn main() -> I32 { text_len(\"hello\") + text_byte(\"hello\", 1) + text_byte(\"hello\", 99) }",
        );
        assert_eq!(interpreted, RuntimeValue::Int(106));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_if_and_text_equality_programs() {
        let interpreted = assert_main_wasm_equivalence(
            "fn main() -> Bool { let flag = true; if flag { \"hello\" == \"hello\" } else { false } }",
        );
        assert_eq!(interpreted, RuntimeValue::Bool(true));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_payload_enum_programs() {
        let interpreted = assert_main_wasm_equivalence(
            "enum OptionText { none, some(Text) }\nfn main() -> OptionText { OptionText.some(\"hello\") }",
        );
        assert_eq!(
            interpreted,
            RuntimeValue::Enum(RuntimeEnum {
                name: "OptionText".to_owned(),
                variant: "some".to_owned(),
                payload: Some(Box::new(RuntimeValue::Text("hello".to_owned()))),
            }),
        );
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_payload_enum_equality_programs() {
        let interpreted = assert_main_wasm_equivalence(
            "enum OptionText { none, some(Text) }\nfn main() -> Bool { OptionText.some(\"hello\") == OptionText.some(\"hello\") }",
        );
        assert_eq!(interpreted, RuntimeValue::Bool(true));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_record_programs() {
        let interpreted = assert_main_wasm_equivalence(
            "struct Pair { left: I32, right: Bool }\nfn main() -> Bool { Pair { left: 7, right: true }.right == true }",
        );
        assert_eq!(interpreted, RuntimeValue::Bool(true));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_record_equality_programs() {
        let interpreted = assert_main_wasm_equivalence(
            "struct Pair { left: I32, right: Bool }\nfn main() -> Bool { Pair { left: 7, right: true } == Pair { left: 7, right: true } }",
        );
        assert_eq!(interpreted, RuntimeValue::Bool(true));
    }

    #[test]
    fn emits_wasm_with_interpreter_equivalence_for_nested_record_results() {
        let interpreted = assert_main_wasm_equivalence(
            "struct Inner { value: I32 }\nstruct Outer { inner: Inner, label: Text }\nfn main() -> Outer { Outer { inner: Inner { value: 7 }, label: \"hello\" } }",
        );
        assert_eq!(
            interpreted,
            RuntimeValue::Record(RuntimeRecord {
                name: "Outer".to_owned(),
                fields: vec![
                    (
                        "inner".to_owned(),
                        RuntimeValue::Record(RuntimeRecord {
                            name: "Inner".to_owned(),
                            fields: vec![("value".to_owned(), RuntimeValue::Int(7))],
                        }),
                    ),
                    ("label".to_owned(), RuntimeValue::Text("hello".to_owned())),
                ],
            }),
        );
    }

    #[test]
    fn runs_public_record_functions_with_wasm_equivalence() {
        let mir = lower_source(
            "struct Pair { left: I32, right: I32 }\nfn echo(pair: Pair) -> Pair { pair }\nfn main() -> I32 { 0 }",
        );
        let argument = RuntimeValue::Record(RuntimeRecord {
            name: "Pair".to_owned(),
            fields: vec![
                ("left".to_owned(), RuntimeValue::Int(20)),
                ("right".to_owned(), RuntimeValue::Int(22)),
            ],
        });

        let result = run_function_wasm(&mir.program, "echo", std::slice::from_ref(&argument))
            .expect("wasm should run");
        assert_eq!(result, argument);
    }

    #[test]
    fn runs_public_payload_enum_functions_with_wasm_equivalence() {
        let mir = lower_source(
            "enum OptionText { none, some(Text) }\nfn keep(value: OptionText) -> OptionText { value }\nfn unwrap(value: OptionText) -> Text { match value { OptionText.none => { \"none\" }, OptionText.some(text) => { text } } }\nfn main() -> I32 { 0 }",
        );
        let argument = RuntimeValue::Enum(RuntimeEnum {
            name: "OptionText".to_owned(),
            variant: "some".to_owned(),
            payload: Some(Box::new(RuntimeValue::Text("hello".to_owned()))),
        });

        let echoed = run_function_wasm(&mir.program, "keep", std::slice::from_ref(&argument))
            .expect("wasm should run");
        let unwrapped = run_function_wasm(&mir.program, "unwrap", std::slice::from_ref(&argument))
            .expect("wasm should run");
        assert_eq!(echoed, argument);
        assert_eq!(unwrapped, RuntimeValue::Text("hello".to_owned()));
    }
}
