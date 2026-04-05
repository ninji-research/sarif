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

pub use runtime::{run_function_wasm, run_main_wasm};

pub fn emit_wat(program: &Program) -> Result<String, WasmError> {
    reject_text_builder_program(program)?;
    let emitter = WasmEmitter::new(program)?;
    emitter.emit()
}

pub fn emit_wasm(program: &Program) -> Result<Vec<u8>, WasmError> {
    let wat = emit_wat(program)?;
    if std::env::var("SARIF_DEBUG_WASM").is_ok() {
        eprintln!("{wat}");
    }
    wat::parse_str(&wat).map_err(|error| WasmError::new(error.to_string()))
}

fn reject_text_builder_program(program: &Program) -> Result<(), WasmError> {
    const TEXT_BUILDER_UNSUPPORTED_MESSAGE: &str =
        "wasm backend does not yet support text builder builtins in stage-0";
    for function in &program.functions {
        let has_text_builder_type = function.params.iter().any(|p| p.ty == "TextBuilder")
            || function.return_type.as_deref() == Some("TextBuilder")
            || function
                .mutable_locals
                .iter()
                .any(|local| local.ty == "TextBuilder");
        if has_text_builder_type {
            return Err(WasmError::new(TEXT_BUILDER_UNSUPPORTED_MESSAGE));
        }
        let mut has_text_builder_inst = false;
        for_each_inst_recursive(&function.instructions, &mut |inst| {
            if matches!(
                inst,
                Inst::TextBuilderNew { .. }
                    | Inst::TextBuilderAppend { .. }
                    | Inst::TextBuilderAppendCodepoint { .. }
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
        writeln!(output, "(module").expect("writing to a string cannot fail");
        writeln!(output, "  (memory (export \"memory\") 1)")
            .expect("writing to a string cannot fail");
        writeln!(output, "  (global $heap_ptr (mut i32) (i32.const 0))")
            .expect("writing to a string cannot fail");

        writeln!(
            output,
            "  (func $alloc (param $size i32) (result i32) (local $ptr i32) (local $new_end i32) (local $pages i32)"
        )
        .expect("writing to a string cannot fail");
        writeln!(output, "    global.get $heap_ptr").expect("writing to a string cannot fail");
        writeln!(output, "    i32.const 7").expect("writing to a string cannot fail");
        writeln!(output, "    i32.add").expect("writing to a string cannot fail");
        writeln!(output, "    i32.const -8").expect("writing to a string cannot fail");
        writeln!(output, "    i32.and").expect("writing to a string cannot fail");
        writeln!(output, "    local.tee $ptr").expect("writing to a string cannot fail");
        writeln!(output, "    local.get $size").expect("writing to a string cannot fail");
        writeln!(output, "    i32.add").expect("writing to a string cannot fail");
        writeln!(output, "    local.tee $new_end").expect("writing to a string cannot fail");
        writeln!(output, "    memory.size").expect("writing to a string cannot fail");
        writeln!(output, "    i32.const 16").expect("writing to a string cannot fail");
        writeln!(output, "    i32.shl").expect("writing to a string cannot fail");
        writeln!(output, "    i32.gt_u").expect("writing to a string cannot fail");
        writeln!(output, "    if").expect("writing to a string cannot fail");
        writeln!(output, "      local.get $new_end").expect("writing to a string cannot fail");
        writeln!(output, "      memory.size").expect("writing to a string cannot fail");
        writeln!(output, "      i32.const 16").expect("writing to a string cannot fail");
        writeln!(output, "      i32.shl").expect("writing to a string cannot fail");
        writeln!(output, "      i32.sub").expect("writing to a string cannot fail");
        writeln!(output, "      i32.const 65535").expect("writing to a string cannot fail");
        writeln!(output, "      i32.add").expect("writing to a string cannot fail");
        writeln!(output, "      i32.const 16").expect("writing to a string cannot fail");
        writeln!(output, "      i32.shr_u").expect("writing to a string cannot fail");
        writeln!(output, "      local.set $pages").expect("writing to a string cannot fail");
        writeln!(output, "      local.get $pages").expect("writing to a string cannot fail");
        writeln!(output, "      memory.grow").expect("writing to a string cannot fail");
        writeln!(output, "      i32.const -1").expect("writing to a string cannot fail");
        writeln!(output, "      i32.eq").expect("writing to a string cannot fail");
        writeln!(output, "      if").expect("writing to a string cannot fail");
        writeln!(output, "        unreachable").expect("writing to a string cannot fail");
        writeln!(output, "      end").expect("writing to a string cannot fail");
        writeln!(output, "    end").expect("writing to a string cannot fail");
        writeln!(output, "    local.get $new_end").expect("writing to a string cannot fail");
        writeln!(output, "    global.set $heap_ptr").expect("writing to a string cannot fail");
        writeln!(output, "    local.get $ptr").expect("writing to a string cannot fail");
        writeln!(output, "  )").expect("writing to a string cannot fail");

        self.emit_support_functions(&mut output)?;

        for function in &self.program.functions {
            self.emit_function(&mut output, function)?;
        }

        writeln!(output, ")").expect("writing to a string cannot fail");
        Ok(output)
    }

    fn emit_support_functions(&self, output: &mut String) -> Result<(), WasmError> {
        output.push_str(
            r#"  (func $__sarif_pack_text (param $ptr i32) (param $len i32) (result i64)
    local.get $ptr
    i64.extend_i32_u
    local.get $len
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.or
  )
  (func $__sarif_text_len_i32 (param $text i64) (result i32)
    local.get $text
    i64.const 32
    i64.shr_u
    i32.wrap_i64
  )
  (func $__sarif_is_ascii_space (param $byte i32) (result i32)
    local.get $byte
    i32.const 32
    i32.eq
    local.get $byte
    i32.const 10
    i32.eq
    i32.or
    local.get $byte
    i32.const 13
    i32.eq
    i32.or
    local.get $byte
    i32.const 9
    i32.eq
    i32.or
  )
  (func $__sarif_is_ascii_digit (param $byte i32) (result i32)
    local.get $byte
    i32.const 48
    i32.ge_u
    local.get $byte
    i32.const 57
    i32.le_u
    i32.and
  )
  (func $__sarif_is_utf8_continuation (param $byte i32) (result i32)
    local.get $byte
    i32.const 192
    i32.and
    i32.const 128
    i32.eq
  )
  (func $__sarif_text_eq (param $left i64) (param $right i64) (result i64)
    (local $left_ptr i32)
    (local $right_ptr i32)
    (local $left_len i32)
    (local $right_len i32)
    (local $index i32)
    (local $equal i64)
    local.get $left
    call $__sarif_text_len_i32
    local.set $left_len
    local.get $right
    call $__sarif_text_len_i32
    local.set $right_len
    local.get $left_len
    local.get $right_len
    i32.ne
    if
      i64.const 0
      return
    end
    local.get $left
    i32.wrap_i64
    local.set $left_ptr
    local.get $right
    i32.wrap_i64
    local.set $right_ptr
    i32.const 0
    local.set $index
    i64.const 1
    local.set $equal
    block $done
      loop $loop
        local.get $index
        local.get $left_len
        i32.ge_u
        br_if $done
        local.get $left_ptr
        local.get $index
        i32.add
        i32.load8_u
        local.get $right_ptr
        local.get $index
        i32.add
        i32.load8_u
        i32.ne
        if
          i64.const 0
          local.set $equal
          br $done
        end
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      end
    end
    local.get $equal
  )
  (func $__sarif_text_byte (param $text i64) (param $index i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $offset i32)
    local.get $index
    i64.const 0
    i64.lt_s
    if
      i64.const 0
      return
    end
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $index
    local.get $len
    i64.extend_i32_u
    i64.ge_u
    if
      i64.const 0
      return
    end
    local.get $index
    i32.wrap_i64
    local.set $offset
    local.get $ptr
    local.get $offset
    i32.add
    i32.load8_u
    i64.extend_i32_u
  )
  (func $__sarif_text_concat (param $left i64) (param $right i64) (result i64)
    (local $left_ptr i32)
    (local $right_ptr i32)
    (local $left_len i32)
    (local $right_len i32)
    (local $dest_ptr i32)
    (local $dest_len i32)
    (local $index i32)
    local.get $left
    call $__sarif_text_len_i32
    local.set $left_len
    local.get $right
    call $__sarif_text_len_i32
    local.set $right_len
    local.get $left_len
    i32.eqz
    if
      local.get $right
      return
    end
    local.get $right_len
    i32.eqz
    if
      local.get $left
      return
    end
    local.get $left
    i32.wrap_i64
    local.set $left_ptr
    local.get $right
    i32.wrap_i64
    local.set $right_ptr
    local.get $left_len
    local.get $right_len
    i32.add
    local.tee $dest_len
    call $alloc
    local.set $dest_ptr
    i32.const 0
    local.set $index
    block $copy_left_done
      loop $copy_left
        local.get $index
        local.get $left_len
        i32.ge_u
        br_if $copy_left_done
        local.get $dest_ptr
        local.get $index
        i32.add
        local.get $left_ptr
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy_left
      end
    end
    i32.const 0
    local.set $index
    block $copy_right_done
      loop $copy_right
        local.get $index
        local.get $right_len
        i32.ge_u
        br_if $copy_right_done
        local.get $dest_ptr
        local.get $left_len
        i32.add
        local.get $index
        i32.add
        local.get $right_ptr
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy_right
      end
    end
    local.get $dest_ptr
    local.get $dest_len
    call $__sarif_pack_text
  )
  (func $__sarif_clamp_text_slice_start (param $text i64) (param $index i64) (result i32)
    (local $ptr i32)
    (local $len i32)
    (local $result i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $index
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $result
    else
      local.get $index
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $result
      else
        local.get $index
        i32.wrap_i64
        local.set $result
      end
    end
    block $done
      loop $loop
        local.get $result
        local.get $len
        i32.ge_u
        br_if $done
        local.get $ptr
        local.get $result
        i32.add
        i32.load8_u
        call $__sarif_is_utf8_continuation
        i32.eqz
        br_if $done
        local.get $result
        i32.const 1
        i32.add
        local.set $result
        br $loop
      end
    end
    local.get $result
  )
  (func $__sarif_clamp_text_slice_end (param $text i64) (param $index i64) (result i32)
    (local $ptr i32)
    (local $len i32)
    (local $result i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $index
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $result
    else
      local.get $index
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $result
      else
        local.get $index
        i32.wrap_i64
        local.set $result
      end
    end
    block $done
      loop $loop
        local.get $result
        local.get $len
        i32.ge_u
        br_if $done
        local.get $ptr
        local.get $result
        i32.add
        i32.load8_u
        call $__sarif_is_utf8_continuation
        i32.eqz
        br_if $done
        local.get $result
        i32.const 1
        i32.sub
        local.set $result
        br $loop
      end
    end
    local.get $result
  )
  (func $__sarif_text_slice (param $text i64) (param $start_raw i64) (param $end_raw i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $start i32)
    (local $end i32)
    (local $dest_ptr i32)
    (local $dest_len i32)
    (local $index i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $text
    local.get $start_raw
    call $__sarif_clamp_text_slice_start
    local.set $start
    local.get $text
    local.get $end_raw
    call $__sarif_clamp_text_slice_end
    local.set $end
    local.get $end
    local.get $start
    i32.le_u
    if
      i64.const 0
      return
    end
    local.get $start
    i32.eqz
    local.get $end
    local.get $len
    i32.eq
    i32.and
    if
      local.get $text
      return
    end
    local.get $end
    local.get $start
    i32.sub
    local.tee $dest_len
    call $alloc
    local.set $dest_ptr
    i32.const 0
    local.set $index
    block $copy_done
      loop $copy
        local.get $index
        local.get $dest_len
        i32.ge_u
        br_if $copy_done
        local.get $dest_ptr
        local.get $index
        i32.add
        local.get $ptr
        local.get $start
        i32.add
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy
      end
    end
    local.get $dest_ptr
    local.get $dest_len
    call $__sarif_pack_text
  )
  (func $__sarif_parse_i32 (param $text i64) (result i64)
    (local $ptr i32)
    (local $start i32)
    (local $end i32)
    (local $negative i32)
    (local $result i64)
    (local $byte i32)
    (local $has_digit i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    i32.const 0
    local.set $start
    local.get $text
    call $__sarif_text_len_i32
    local.set $end
    block $trim_start_done
      loop $trim_start
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_start_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_start_done
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $trim_start
      end
    end
    block $trim_end_done
      loop $trim_end
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_end_done
        local.get $ptr
        local.get $end
        i32.const 1
        i32.sub
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_end_done
        local.get $end
        i32.const 1
        i32.sub
        local.set $end
        br $trim_end
      end
    end
    local.get $start
    local.get $end
    i32.ge_u
    if
      unreachable
    end
    i32.const 0
    local.set $negative
    local.get $ptr
    local.get $start
    i32.add
    i32.load8_u
    local.tee $byte
    i32.const 45
    i32.eq
    if
      i32.const 1
      local.set $negative
      local.get $start
      i32.const 1
      i32.add
      local.set $start
    else
      local.get $byte
      i32.const 43
      i32.eq
      if
        local.get $start
        i32.const 1
        i32.add
        local.set $start
      end
    end
    i64.const 0
    local.set $result
    i32.const 0
    local.set $has_digit
    block $parse_done
      loop $parse
        local.get $start
        local.get $end
        i32.ge_u
        br_if $parse_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        local.tee $byte
        call $__sarif_is_ascii_digit
        i32.eqz
        br_if $parse_done
        local.get $result
        i64.const 10
        i64.mul
        local.get $byte
        i32.const 48
        i32.sub
        i64.extend_i32_u
        i64.add
        local.set $result
        i32.const 1
        local.set $has_digit
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $parse
      end
    end
    local.get $has_digit
    i32.eqz
    if
      unreachable
    end
    local.get $start
    local.get $end
    i32.ne
    if
      unreachable
    end
    local.get $negative
    if
      i64.const 0
      local.get $result
      i64.sub
      return
    end
    local.get $result
  )
  (func $__sarif_parse_f64 (param $text i64) (result f64)
    (local $ptr i32)
    (local $start i32)
    (local $end i32)
    (local $negative i32)
    (local $result f64)
    (local $scale f64)
    (local $byte i32)
    (local $has_digit i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    i32.const 0
    local.set $start
    local.get $text
    call $__sarif_text_len_i32
    local.set $end
    block $trim_start_done
      loop $trim_start
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_start_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_start_done
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $trim_start
      end
    end
    block $trim_end_done
      loop $trim_end
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_end_done
        local.get $ptr
        local.get $end
        i32.const 1
        i32.sub
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_end_done
        local.get $end
        i32.const 1
        i32.sub
        local.set $end
        br $trim_end
      end
    end
    local.get $start
    local.get $end
    i32.ge_u
    if
      unreachable
    end
    i32.const 0
    local.set $negative
    local.get $ptr
    local.get $start
    i32.add
    i32.load8_u
    local.tee $byte
    i32.const 45
    i32.eq
    if
      i32.const 1
      local.set $negative
      local.get $start
      i32.const 1
      i32.add
      local.set $start
    else
      local.get $byte
      i32.const 43
      i32.eq
      if
        local.get $start
        i32.const 1
        i32.add
        local.set $start
      end
    end
    f64.const 0
    local.set $result
    i32.const 0
    local.set $has_digit
    block $whole_done
      loop $whole
        local.get $start
        local.get $end
        i32.ge_u
        br_if $whole_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        local.tee $byte
        call $__sarif_is_ascii_digit
        i32.eqz
        br_if $whole_done
        local.get $result
        f64.const 10
        f64.mul
        local.get $byte
        i32.const 48
        i32.sub
        f64.convert_i32_u
        f64.add
        local.set $result
        i32.const 1
        local.set $has_digit
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $whole
      end
    end
    local.get $start
    local.get $end
    i32.lt_u
    if
      local.get $ptr
      local.get $start
      i32.add
      i32.load8_u
      i32.const 46
      i32.eq
      if
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        f64.const 10
        local.set $scale
        block $fraction_done
          loop $fraction
            local.get $start
            local.get $end
            i32.ge_u
            br_if $fraction_done
            local.get $ptr
            local.get $start
            i32.add
            i32.load8_u
            local.tee $byte
            call $__sarif_is_ascii_digit
            i32.eqz
            br_if $fraction_done
            local.get $result
            local.get $byte
            i32.const 48
            i32.sub
            f64.convert_i32_u
            local.get $scale
            f64.div
            f64.add
            local.set $result
            local.get $scale
            f64.const 10
            f64.mul
            local.set $scale
            i32.const 1
            local.set $has_digit
            local.get $start
            i32.const 1
            i32.add
            local.set $start
            br $fraction
          end
        end
      end
    end
    local.get $has_digit
    i32.eqz
    if
      unreachable
    end
    local.get $start
    local.get $end
    i32.ne
    if
      unreachable
    end
    local.get $negative
    if
      f64.const -1
      local.get $result
      f64.mul
      return
    end
    local.get $result
  )
"#,
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
            | WasmValueKind::List(_) => {
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
        if function.name == "main" {
            write!(output, " (export \"main\")").expect("writing to a string cannot fail");
        }

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
                | Inst::TextByte { dest, .. }
                | Inst::TextCmp { dest, .. }
                | Inst::TextEqRange { dest, .. }
                | Inst::TextFindByteRange { dest, .. }
                | Inst::TextConcat { dest, .. }
                | Inst::TextSlice { dest, .. }
                | Inst::TextBuilderNew { dest }
                | Inst::TextIndexNew { dest }
                | Inst::TextBuilderAppend { dest, .. }
                | Inst::TextBuilderAppendCodepoint { dest, .. }
                | Inst::TextBuilderAppendI32 { dest, .. }
                | Inst::TextBuilderFinish { dest, .. }
                | Inst::StdoutWriteBuilder { dest, .. }
                | Inst::TextIndexGet { dest, .. }
                | Inst::TextIndexSet { dest, .. }
                | Inst::TextFromF64Fixed { dest, .. }
                | Inst::ArgCount { dest, .. }
                | Inst::ArgText { dest, .. }
                | Inst::StdinText { dest }
                | Inst::ParseI32 { dest, .. }
                | Inst::ParseI32Range { dest, .. }
                | Inst::ParseF64 { dest, .. }
                | Inst::MakeEnum { dest, .. }
                | Inst::MakeRecord { dest, .. }
                | Inst::Field { dest, .. }
                | Inst::EnumTagEq { dest, .. }
                | Inst::EnumPayload { dest, .. }
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
                | Inst::AllocPop => {}
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
                writeln!(output, "    i64.const {}", value)
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstF64 { dest, bits } => {
                writeln!(output, "    f64.const {}", f64::from_bits(*bits))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::ConstBool { dest, value } => {
                writeln!(output, "    i64.const {}", if *value { 1 } else { 0 })
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
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
            Inst::TextLen { dest, text } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_len_i32")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    i64.extend_i32_u").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextByte { dest, text, index } => {
                writeln!(output, "    local.get ${}", wasm_id(*text))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*index))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_byte")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextConcat { dest, left, right } => {
                writeln!(output, "    local.get ${}", wasm_id(*left))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.get ${}", wasm_id(*right))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    call $__sarif_text_concat")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextSlice {
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
                writeln!(output, "    call $__sarif_text_slice")
                    .expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::TextBuilderNew { .. }
            | Inst::TextIndexNew { .. }
            | Inst::TextBuilderAppend { .. }
            | Inst::TextBuilderAppendCodepoint { .. }
            | Inst::TextBuilderAppendI32 { .. }
            | Inst::TextBuilderFinish { .. }
            | Inst::StdoutWriteBuilder { .. }
            | Inst::TextIndexGet { .. }
            | Inst::TextIndexSet { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text builder/index builtins in stage-0",
                ));
            }
            Inst::TextFromF64Fixed { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support `text_from_f64_fixed` in stage-0",
                ));
            }
            Inst::ArgCount { .. } | Inst::ArgText { .. } | Inst::StdinText { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support runtime input builtins in stage-0",
                ));
            }
            Inst::AllocPush | Inst::AllocPop => {
                return Err(WasmError::new(
                    "wasm backend does not yet support allocation scope builtins in stage-0",
                ));
            }
            Inst::StdoutWrite { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support runtime io builtins in stage-0",
                ));
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
            Inst::ListNew { .. }
            | Inst::ListLen { .. }
            | Inst::ListGet { .. }
            | Inst::ListSet { .. }
            | Inst::ListPush { .. }
            | Inst::ListSortText { .. }
            | Inst::ListSortRecordTextField { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support list values in stage-0",
                ));
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
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    f64.convert_i64_s").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
            }
            Inst::Sqrt { dest, value } => {
                writeln!(output, "    local.get ${}", wasm_id(*value))
                    .expect("writing to a string cannot fail");
                writeln!(output, "    f64.sqrt").expect("writing to a string cannot fail");
                writeln!(output, "    local.set ${}", wasm_id(*dest))
                    .expect("writing to a string cannot fail");
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
            Inst::TextCmp { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text_cmp in stage-0",
                ));
            }
            Inst::TextEqRange { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text_eq_range in stage-0",
                ));
            }
            Inst::TextFindByteRange { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text_find_byte_range in stage-0",
                ));
            }
            Inst::ParseI32Range { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support parse_i32_range in stage-0",
                ));
            }
            Inst::Perform { .. } | Inst::Handle { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support effect handlers",
                ));
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
            | Inst::TextByte { dest, .. }
            | Inst::TextCmp { dest, .. }
            | Inst::TextEqRange { dest, .. }
            | Inst::TextFindByteRange { dest, .. }
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
            | Inst::TextSlice { dest, .. }
            | Inst::TextFromF64Fixed { dest, .. }
            | Inst::ArgText { dest, .. }
            | Inst::StdinText { dest } => {
                kinds.insert(*dest, WasmValueKind::Text);
            }
            Inst::TextBuilderNew { .. }
            | Inst::TextIndexNew { .. }
            | Inst::TextBuilderAppend { .. }
            | Inst::TextBuilderAppendCodepoint { .. }
            | Inst::TextBuilderAppendI32 { .. }
            | Inst::TextBuilderFinish { .. }
            | Inst::StdoutWriteBuilder { .. }
            | Inst::TextIndexGet { .. }
            | Inst::TextIndexSet { .. } => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text builder/index builtins in stage-0",
                ));
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
                let struct_ty = structs.iter().find(|s| s.name == record_name).unwrap();
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
            Inst::Add { dest, left, .. }
            | Inst::Sub { dest, left, .. }
            | Inst::Mul { dest, left, .. }
            | Inst::Div { dest, left, .. } => {
                kinds.insert(*dest, kinds[left].clone());
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
            Inst::StoreLocal { .. } | Inst::Assert { .. } => {}
        }
    }
    Ok(())
}
