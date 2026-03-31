#![allow(clippy::only_used_in_recursion)]
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use sarif_frontend::hir::{BinaryOp, Expr, Item, Module, Stmt};
use sarif_syntax::{Diagnostic, Span};

#[cfg(feature = "backend-native")]
mod native;
#[cfg(feature = "backend-native")]
mod object;
#[cfg(feature = "backend-wasm")]
mod wasm;

#[cfg(feature = "backend-native")]
pub use native::{
    NativeEnum, NativeRecord, NativeRecordField, NativeValueKind, collect_native_enums,
    collect_native_records, native_enum_is_payload_free,
};
#[cfg(feature = "backend-native")]
pub use object::{ENTRYPOINT_SYMBOL, ObjectError, emit_object};
#[cfg(feature = "backend-wasm")]
pub use wasm::{WasmError, emit_wasm, emit_wat, run_function_wasm, run_main_wasm};

#[derive(Clone, Debug, Default)]
pub struct MirLowering {
    pub program: Program,
    pub const_values: BTreeMap<String, RuntimeValue>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub enums: Vec<EnumType>,
    pub structs: Vec<StructType>,
    pub functions: Vec<Function>,
}

impl Program {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        writeln!(&mut output, "MIR").expect("writing to a string cannot fail");

        for enum_ty in &self.enums {
            writeln!(
                &mut output,
                "  enum {} {{ {} }}",
                enum_ty.name,
                enum_ty
                    .variants
                    .iter()
                    .map(|variant| {
                        variant.payload_type.as_ref().map_or_else(
                            || variant.name.clone(),
                            |payload| format!("{}({payload})", variant.name),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .expect("writing to a string cannot fail");
        }

        for struct_ty in &self.structs {
            writeln!(
                &mut output,
                "  struct {} {{ {} }}",
                struct_ty.name,
                struct_ty
                    .fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, field.ty))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .expect("writing to a string cannot fail");
        }

        for function in &self.functions {
            writeln!(
                &mut output,
                "  fn {}({}){}",
                function.name,
                function
                    .params
                    .iter()
                    .map(|param| format!("{}: {}", param.name, param.ty))
                    .collect::<Vec<_>>()
                    .join(", "),
                function
                    .return_type
                    .as_ref()
                    .map_or_else(String::new, |ty| format!(" -> {ty}")),
            )
            .expect("writing to a string cannot fail");
            for local in &function.mutable_locals {
                writeln!(
                    &mut output,
                    "    {} {}: {}",
                    if local.mutable {
                        format!("mut {}", local.slot.render())
                    } else {
                        local.slot.render()
                    },
                    local.name,
                    local.ty,
                )
                .expect("writing to a string cannot fail");
            }
            for inst in &function.instructions {
                writeln!(&mut output, "    {}", inst.pretty())
                    .expect("writing to a string cannot fail");
            }
            writeln!(
                &mut output,
                "    return {}",
                function
                    .result
                    .map_or_else(|| "unit".to_owned(), ValueId::render),
            )
            .expect("writing to a string cannot fail");
        }

        output
    }
}

#[derive(Clone, Debug)]
pub struct EnumType {
    pub name: String,
    pub variants: Vec<EnumVariantType>,
}

#[derive(Clone, Debug)]
pub struct EnumVariantType {
    pub name: String,
    pub payload_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StructType {
    pub name: String,
    pub fields: Vec<StructField>,
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub name: String,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub effects: Vec<String>,
    pub mutable_locals: Vec<MutableLocal>,
    pub instructions: Vec<Inst>,
    pub result: Option<ValueId>,
    pub span: Span,
}

impl Function {
    #[cfg(any(feature = "backend-native", feature = "backend-wasm"))]
    pub fn mutable_local_type(&self, slot: LocalSlotId) -> Option<&str> {
        self.mutable_locals
            .iter()
            .find(|local| local.slot == slot)
            .map(|local| local.ty.as_str())
    }
}

pub fn for_each_inst_recursive<F>(insts: &[Inst], f: &mut F)
where
    F: FnMut(&Inst),
{
    for inst in insts {
        f(inst);
        match inst {
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                for_each_inst_recursive(then_insts, f);
                for_each_inst_recursive(else_insts, f);
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                for_each_inst_recursive(condition_insts, f);
                for_each_inst_recursive(body_insts, f);
            }
            Inst::Repeat { body_insts, .. } => {
                for_each_inst_recursive(body_insts, f);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodegenValueKind {
    Unit,
    I32,
    F64,
    Bool,
    Text,
    TextBuilder,
    List(Box<Self>),
    Enum(String),
    Record(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractKind {
    Requires,
    Ensures,
    Bounds,
}

impl ContractKind {
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Requires => "requires",
            Self::Ensures => "ensures",
            Self::Bounds => "bounds",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct MutableLocal {
    pub slot: LocalSlotId,
    pub name: String,
    pub ty: String,
    pub mutable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(u32);

impl ValueId {
    #[must_use]
    pub fn render(self) -> String {
        format!("%{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalSlotId(u32);

impl LocalSlotId {
    #[must_use]
    pub fn render(self) -> String {
        format!("slot{}", self.0)
    }
}

#[derive(Clone, Debug)]
pub struct HandleArm {
    pub effect: String,
    pub operation: String,
    pub params: Vec<String>,
    pub body_insts: Vec<Inst>,
    pub body_result: Option<ValueId>,
}

#[derive(Clone, Debug)]
pub enum Inst {
    TextBuilderNew {
        dest: ValueId,
    },
    TextBuilderAppend {
        dest: ValueId,
        builder: ValueId,
        text: ValueId,
    },
    TextBuilderAppendCodepoint {
        dest: ValueId,
        builder: ValueId,
        codepoint: ValueId,
    },
    TextBuilderFinish {
        dest: ValueId,
        builder: ValueId,
    },
    ListNew {
        dest: ValueId,
        len: ValueId,
        value: ValueId,
    },
    ListLen {
        dest: ValueId,
        list: ValueId,
    },
    ListGet {
        dest: ValueId,
        list: ValueId,
        index: ValueId,
    },
    ListSet {
        dest: ValueId,
        list: ValueId,
        index: ValueId,
        value: ValueId,
    },
    F64FromI32 {
        dest: ValueId,
        value: ValueId,
    },
    TextLen {
        dest: ValueId,
        text: ValueId,
    },
    TextConcat {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    TextSlice {
        dest: ValueId,
        text: ValueId,
        start: ValueId,
        end: ValueId,
    },
    TextByte {
        dest: ValueId,
        text: ValueId,
        index: ValueId,
    },
    TextCmp {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    TextEqRange {
        dest: ValueId,
        source: ValueId,
        start: ValueId,
        end: ValueId,
        expected: ValueId,
    },
    TextFindByteRange {
        dest: ValueId,
        source: ValueId,
        start: ValueId,
        end: ValueId,
        byte: ValueId,
    },
    TextFromF64Fixed {
        dest: ValueId,
        value: ValueId,
        digits: ValueId,
    },
    ArgCount {
        dest: ValueId,
    },
    AllocPush,
    AllocPop,
    ArgText {
        dest: ValueId,
        index: ValueId,
    },
    StdinText {
        dest: ValueId,
    },
    StdoutWrite {
        text: ValueId,
    },
    ParseI32 {
        dest: ValueId,
        text: ValueId,
    },
    ParseI32Range {
        dest: ValueId,
        text: ValueId,
        start: ValueId,
        end: ValueId,
    },
    ParseF64 {
        dest: ValueId,
        text: ValueId,
    },
    LoadParam {
        dest: ValueId,
        index: usize,
    },
    LoadLocal {
        dest: ValueId,
        slot: LocalSlotId,
    },
    StoreLocal {
        slot: LocalSlotId,
        src: ValueId,
    },
    ConstInt {
        dest: ValueId,
        value: i64,
    },
    ConstF64 {
        dest: ValueId,
        bits: u64,
    },
    ConstBool {
        dest: ValueId,
        value: bool,
    },
    ConstText {
        dest: ValueId,
        value: String,
    },
    MakeEnum {
        dest: ValueId,
        name: String,
        variant: String,
        payload: Option<ValueId>,
    },
    MakeRecord {
        dest: ValueId,
        name: String,
        fields: Vec<(String, ValueId)>,
    },
    Field {
        dest: ValueId,
        base: ValueId,
        name: String,
    },
    EnumTagEq {
        dest: ValueId,
        value: ValueId,
        tag: i64,
    },
    EnumPayload {
        dest: ValueId,
        value: ValueId,
        payload_type: String,
    },
    If {
        dest: ValueId,
        condition: ValueId,
        then_insts: Vec<Self>,
        then_result: Option<ValueId>,
        else_insts: Vec<Self>,
        else_result: Option<ValueId>,
    },
    While {
        dest: ValueId,
        condition_insts: Vec<Self>,
        condition: ValueId,
        body_insts: Vec<Self>,
    },
    Repeat {
        dest: ValueId,
        count: ValueId,
        index_slot: Option<LocalSlotId>,
        body_insts: Vec<Self>,
    },
    Add {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Sub {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Mul {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Div {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Sqrt {
        dest: ValueId,
        value: ValueId,
    },
    And {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Or {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Eq {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Ne {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Lt {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Le {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Gt {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Ge {
        dest: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Call {
        dest: ValueId,
        callee: String,
        args: Vec<ValueId>,
    },
    Assert {
        condition: ValueId,
        kind: ContractKind,
    },
    Perform {
        dest: ValueId,
        effect: String,
        operation: String,
        args: Vec<ValueId>,
    },
    Handle {
        dest: ValueId,
        body_insts: Vec<Self>,
        body_result: Option<ValueId>,
        arms: Vec<HandleArm>,
    },
}

impl Inst {
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn pretty(&self) -> String {
        match self {
            Self::LoadParam { dest, index } => format!("{} = param {index}", dest.render()),
            Self::LoadLocal { dest, slot } => {
                format!("{} = load {}", dest.render(), slot.render())
            }
            Self::StoreLocal { slot, src } => {
                format!("store {}, {}", slot.render(), src.render())
            }
            Self::ConstInt { dest, value } => format!("{} = int {value}", dest.render()),
            Self::ConstF64 { dest, bits } => {
                format!("{} = f64 {}", dest.render(), f64::from_bits(*bits))
            }
            Self::ConstBool { dest, value } => format!("{} = bool {value}", dest.render()),
            Self::ConstText { dest, value } => format!("{} = text {:?}", dest.render(), value),
            Self::TextBuilderNew { dest } => {
                format!("{} = text-builder-new", dest.render())
            }
            Self::TextBuilderAppend {
                dest,
                builder,
                text,
            } => format!(
                "{} = text-builder-append {}, {}",
                dest.render(),
                builder.render(),
                text.render()
            ),
            Self::TextBuilderAppendCodepoint {
                dest,
                builder,
                codepoint,
            } => format!(
                "{} = text-builder-append-codepoint {}, {}",
                dest.render(),
                builder.render(),
                codepoint.render()
            ),
            Self::TextBuilderFinish { dest, builder } => format!(
                "{} = text-builder-finish {}",
                dest.render(),
                builder.render()
            ),
            Self::ListNew { dest, len, value } => format!(
                "{} = list-new {}, {}",
                dest.render(),
                len.render(),
                value.render()
            ),
            Self::ListLen { dest, list } => {
                format!("{} = list-len {}", dest.render(), list.render())
            }
            Self::ListGet { dest, list, index } => {
                format!(
                    "{} = list-get {}, {}",
                    dest.render(),
                    list.render(),
                    index.render()
                )
            }
            Self::ListSet {
                dest,
                list,
                index,
                value,
            } => format!(
                "{} = list-set {}, {}, {}",
                dest.render(),
                list.render(),
                index.render(),
                value.render()
            ),
            Self::F64FromI32 { dest, value } => {
                format!("{} = f64-from-i32 {}", dest.render(), value.render())
            }
            Self::TextLen { dest, text } => {
                format!("{} = text-len {}", dest.render(), text.render())
            }
            Self::TextConcat { dest, left, right } => format!(
                "{} = text-concat {}, {}",
                dest.render(),
                left.render(),
                right.render()
            ),
            Self::TextSlice {
                dest,
                text,
                start,
                end,
            } => format!(
                "{} = text-slice {}, {}, {}",
                dest.render(),
                text.render(),
                start.render(),
                end.render()
            ),
            Self::TextByte { dest, text, index } => format!(
                "{} = text-byte {}, {}",
                dest.render(),
                text.render(),
                index.render()
            ),
            Self::TextCmp { dest, left, right } => format!(
                "{} = text-cmp {}, {}",
                dest.render(),
                left.render(),
                right.render()
            ),
            Self::TextEqRange {
                dest,
                source,
                start,
                end,
                expected,
            } => format!(
                "{} = text-eq-range {}, {}, {}, {}",
                dest.render(),
                source.render(),
                start.render(),
                end.render(),
                expected.render()
            ),
            Self::TextFindByteRange {
                dest,
                source,
                start,
                end,
                byte,
            } => format!(
                "{} = text-find-byte-range {}, {}, {}, {}",
                dest.render(),
                source.render(),
                start.render(),
                end.render(),
                byte.render()
            ),
            Self::TextFromF64Fixed {
                dest,
                value,
                digits,
            } => format!(
                "{} = text-from-f64-fixed {}, {}",
                dest.render(),
                value.render(),
                digits.render()
            ),
            Self::ArgCount { dest } => format!("{} = arg-count", dest.render()),
            Self::AllocPush => "alloc-push".to_owned(),
            Self::AllocPop => "alloc-pop".to_owned(),
            Self::ArgText { dest, index } => {
                format!("{} = arg-text {}", dest.render(), index.render())
            }
            Self::StdinText { dest } => format!("{} = stdin-text", dest.render()),
            Self::StdoutWrite { text } => format!("stdout-write {}", text.render()),
            Self::ParseI32 { dest, text } => {
                format!("{} = parse-i32 {}", dest.render(), text.render())
            }
            Self::ParseI32Range {
                dest,
                text,
                start,
                end,
            } => format!(
                "{} = parse-i32-range {}, {}, {}",
                dest.render(),
                text.render(),
                start.render(),
                end.render()
            ),
            Self::ParseF64 { dest, text } => {
                format!("{} = parse-f64 {}", dest.render(), text.render())
            }
            Self::MakeEnum {
                dest,
                name,
                variant,
                payload,
            } => format!(
                "{} = enum {}.{}{}",
                dest.render(),
                name,
                variant,
                payload
                    .map(|value| format!("({})", value.render()))
                    .unwrap_or_default(),
            ),
            Self::MakeRecord { dest, name, fields } => format!(
                "{} = record {} {{ {} }}",
                dest.render(),
                name,
                fields
                    .iter()
                    .map(|(field, value)| format!("{field}: {}", value.render()))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Field { dest, base, name } => {
                format!("{} = field {}, {}", dest.render(), base.render(), name)
            }
            Self::EnumTagEq { dest, value, tag } => format!(
                "{} = enum-tag-eq {}, {}",
                dest.render(),
                value.render(),
                tag
            ),
            Self::EnumPayload {
                dest,
                value,
                payload_type,
            } => {
                format!(
                    "{} = enum-payload {} as {}",
                    dest.render(),
                    value.render(),
                    payload_type
                )
            }
            Self::If {
                dest,
                condition,
                then_insts,
                then_result,
                else_insts,
                else_result,
            } => format!(
                "{} = if {} then [{}] -> {} else [{}] -> {}",
                dest.render(),
                condition.render(),
                then_insts
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join("; "),
                then_result.map_or_else(|| "unit".to_owned(), ValueId::render),
                else_insts
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join("; "),
                else_result.map_or_else(|| "unit".to_owned(), ValueId::render),
            ),
            Self::While {
                dest,
                condition_insts,
                condition,
                body_insts,
            } => format!(
                "{} = while [{}] -> {} [{}]",
                dest.render(),
                condition_insts
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join("; "),
                condition.render(),
                body_insts
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            Self::Repeat {
                dest,
                count,
                index_slot,
                body_insts,
            } => format!(
                "{} = repeat {}{} [{}]",
                dest.render(),
                count.render(),
                index_slot.map_or_else(String::new, |slot| format!(" with {}", slot.render())),
                body_insts
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            Self::Add { dest, left, right } => {
                format!(
                    "{} = add {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Sub { dest, left, right } => {
                format!(
                    "{} = sub {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Mul { dest, left, right } => {
                format!(
                    "{} = mul {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Div { dest, left, right } => {
                format!(
                    "{} = div {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Sqrt { dest, value } => {
                format!("{} = sqrt {}", dest.render(), value.render())
            }
            Self::And { dest, left, right } => {
                format!(
                    "{} = and {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Or { dest, left, right } => {
                format!(
                    "{} = or {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Eq { dest, left, right } => {
                format!(
                    "{} = eq {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Ne { dest, left, right } => {
                format!(
                    "{} = ne {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Lt { dest, left, right } => {
                format!(
                    "{} = lt {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Le { dest, left, right } => {
                format!(
                    "{} = le {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Gt { dest, left, right } => {
                format!(
                    "{} = gt {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Ge { dest, left, right } => {
                format!(
                    "{} = ge {}, {}",
                    dest.render(),
                    left.render(),
                    right.render()
                )
            }
            Self::Call { dest, callee, args } => format!(
                "{} = call {}({})",
                dest.render(),
                callee,
                args.iter()
                    .map(|value| value.render())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Assert { condition, kind } => {
                format!("assert {} {}", kind.keyword(), condition.render())
            }
            Self::Perform {
                dest,
                effect,
                operation,
                args,
            } => {
                let args_str = args
                    .iter()
                    .map(|id| id.render())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{} = perform {}.{}({})",
                    dest.render(),
                    effect,
                    operation,
                    args_str
                )
            }
            Self::Handle {
                dest,
                body_insts,
                body_result,
                arms,
            } => {
                let mut out = format!("{} = handle {{\n", dest.render());
                for inst in body_insts {
                    out.push_str(&format!("  {}\n", inst.pretty().replace('\n', "\n  ")));
                }
                if let Some(result) = body_result {
                    out.push_str(&format!("  yield {}\n", result.render()));
                }
                out.push_str("} with {\n");
                for arm in arms {
                    out.push_str(&format!(
                        "  {}.{}({}) => {{\n",
                        arm.effect,
                        arm.operation,
                        arm.params.join(", ")
                    ));
                    for inst in &arm.body_insts {
                        out.push_str(&format!("    {}\n", inst.pretty().replace('\n', "\n    ")));
                    }
                    if let Some(result) = arm.body_result {
                        out.push_str(&format!("    yield {}\n", result.render()));
                    }
                    out.push_str("  }\n");
                }
                out.push('}');
                out
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeValue {
    Int(i64),
    F64(f64),
    Bool(bool),
    Text(String),
    TextBuilder(u64),
    List(u64),
    Enum(RuntimeEnum),
    Record(RuntimeRecord),
    Unit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeEnum {
    pub name: String,
    pub variant: String,
    pub payload: Option<Box<RuntimeValue>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeRecord {
    pub name: String,
    pub fields: Vec<(String, RuntimeValue)>,
}

impl RuntimeValue {
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::F64(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Text(value) => value.clone(),
            Self::TextBuilder(_) => "<text-builder>".to_owned(),
            Self::List(_) => "<list>".to_owned(),
            Self::Enum(value) => value.payload.as_ref().map_or_else(
                || format!("{}.{}", value.name, value.variant),
                |payload| format!("{}.{}({})", value.name, value.variant, payload.render()),
            ),
            Self::Record(record) => render_runtime_record(record),
            Self::Unit => "unit".to_owned(),
        }
    }
}

#[allow(clippy::too_many_lines)]
#[must_use]
pub fn lower(module: &Module) -> MirLowering {
    let mut diagnostics = Vec::new();
    let mut enums = Vec::new();
    let mut structs = Vec::new();
    let mut functions = Vec::new();
    let mut generated_arrays = BTreeMap::<String, GeneratedArrayType>::new();
    let consts = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Const(const_item) => Some((const_item.name.clone(), const_item.clone())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let enum_variants = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(enum_item) => Some((
                enum_item.name.clone(),
                enum_item
                    .variants
                    .iter()
                    .map(|variant| EnumVariantType {
                        name: variant.name.clone(),
                        payload_type: variant.payload.as_ref().map(|payload| payload.path.clone()),
                    })
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let struct_fields = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(struct_item) => Some((
                struct_item.name.clone(),
                struct_item
                    .fields
                    .iter()
                    .map(|field| field.name.clone())
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let struct_layouts = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(struct_item) => Some((
                struct_item.name.clone(),
                struct_item
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.path.clone()))
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let function_items = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(function) => Some((function.name.clone(), function)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let function_returns = function_items
        .iter()
        .map(|(name, function)| {
            (
                name.clone(),
                function
                    .return_type
                    .as_ref()
                    .map_or_else(|| "Unit".to_owned(), |ty| ty.path.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let evaluated_consts = evaluate_const_values(
        &consts,
        &function_items,
        &enum_variants,
        &struct_fields,
        &mut diagnostics,
    );
    let mut all_functions = BTreeMap::new();
    let mut generic_functions = BTreeMap::new();
    for item in &module.items {
        if let Item::Function(function) = item {
            all_functions.insert(function.name.clone(), function);
            if !function.type_params.is_empty() {
                generic_functions.insert(function.name.clone(), function);
            }
        }
    }

    let mut shared = LowerShared {
        enum_variants: &enum_variants,
        struct_fields: &struct_fields,
        struct_layouts: &struct_layouts,
        function_returns: &function_returns,
        evaluated_consts: &evaluated_consts,
        generated_arrays: &mut generated_arrays,
        generic_functions: &generic_functions,
        all_functions: &all_functions,
        monomorphized_functions: Vec::new(),
    };
    for item in &module.items {
        match item {
            Item::Const(_) => {}
            Item::Function(function) if function.type_params.is_empty() => {
                functions.push(lower_function(function, &mut shared, &mut diagnostics));
            }
            Item::Function(_) => {} // Generic functions handled on demand
            Item::Effect(_) => {}
            Item::Enum(enum_item) => {
                enums.push(EnumType {
                    name: enum_item.name.clone(),
                    variants: enum_item
                        .variants
                        .iter()
                        .map(|variant| EnumVariantType {
                            name: variant.name.clone(),
                            payload_type: variant
                                .payload
                                .as_ref()
                                .map(|payload| payload.path.clone()),
                        })
                        .collect(),
                });
            }
            Item::Struct(struct_item) => {
                structs.push(StructType {
                    name: struct_item.name.clone(),
                    fields: struct_item
                        .fields
                        .iter()
                        .map(|field| StructField {
                            name: field.name.clone(),
                            ty: field.ty.path.clone(),
                        })
                        .collect(),
                });
            }
        }
    }
    for generated in shared.generated_arrays.values() {
        structs.push(StructType {
            name: generated.name.clone(),
            fields: (0..generated.len)
                .map(|index| StructField {
                    name: array_field_name(index),
                    ty: generated.element_ty.clone(),
                })
                .collect(),
        });
    }

    functions.extend(shared.monomorphized_functions);

    MirLowering {
        program: Program {
            enums,
            structs,
            functions,
        },
        const_values: evaluated_consts,
        diagnostics,
    }
}

fn evaluate_const_values<'a>(
    consts: &'a BTreeMap<String, sarif_frontend::hir::Const>,
    functions: &'a BTreeMap<String, &'a sarif_frontend::hir::Function>,
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantType>>,
    struct_fields: &'a BTreeMap<String, Vec<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, RuntimeValue> {
    let mut evaluator = ConstEvaluator {
        consts,
        functions,
        enum_variants,
        struct_fields,
        values: BTreeMap::new(),
        active_consts: Vec::new(),
        active_functions: Vec::new(),
        next_slot: 0,
        contract_result: None,
        diagnostics,
    };
    for name in consts.keys() {
        let _ = evaluator.eval_const(name);
    }
    evaluator.values
}

struct ConstEvaluator<'a, 'diag> {
    consts: &'a BTreeMap<String, sarif_frontend::hir::Const>,
    functions: &'a BTreeMap<String, &'a sarif_frontend::hir::Function>,
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantType>>,
    struct_fields: &'a BTreeMap<String, Vec<String>>,
    values: BTreeMap<String, RuntimeValue>,
    active_consts: Vec<String>,
    active_functions: Vec<String>,
    next_slot: u32,
    contract_result: Option<RuntimeValue>,
    diagnostics: &'diag mut Vec<Diagnostic>,
}

#[derive(Debug)]
struct ConstEvalError {
    span: Span,
    message: String,
}

impl ConstEvalError {
    fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

enum ConstFlow {
    Value(RuntimeValue),
}

enum PatternMatch {
    NoMatch,
    Match(Option<(String, RuntimeValue)>),
}

#[derive(Clone, Debug, Default)]
struct ConstEnv {
    bindings: BTreeMap<String, ConstBinding>,
    slots: BTreeMap<ConstSlotId, RuntimeValue>,
}

#[derive(Clone, Debug)]
enum ConstBinding {
    Value(RuntimeValue),
    Slot(ConstSlotId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ConstSlotId(u32);

enum ConstAssignResult {
    Assigned,
    Immutable,
    Unknown,
}

impl ConstEnv {
    fn read(&self, name: &str) -> Option<RuntimeValue> {
        match self.bindings.get(name)? {
            ConstBinding::Value(value) => Some(value.clone()),
            ConstBinding::Slot(slot) => self.slots.get(slot).cloned(),
        }
    }

    fn bind_value(&mut self, name: String, value: RuntimeValue) {
        self.bindings.insert(name, ConstBinding::Value(value));
    }

    fn bind_slot(&mut self, name: String, slot: ConstSlotId, value: RuntimeValue) {
        self.slots.insert(slot, value);
        self.bindings.insert(name, ConstBinding::Slot(slot));
    }

    fn assign(&mut self, name: &str, value: RuntimeValue) -> ConstAssignResult {
        match self.bindings.get(name) {
            Some(ConstBinding::Slot(slot)) => {
                self.slots.insert(*slot, value);
                ConstAssignResult::Assigned
            }
            Some(ConstBinding::Value(_)) => ConstAssignResult::Immutable,
            None => ConstAssignResult::Unknown,
        }
    }

    fn assign_array_index(
        &mut self,
        name: &str,
        index: i64,
        value: RuntimeValue,
    ) -> Result<ConstAssignResult, String> {
        match self.bindings.get(name) {
            Some(ConstBinding::Slot(slot)) => {
                let Some(current) = self.slots.get(slot).cloned() else {
                    return Err(format!(
                        "compile-time mutable local `{name}` is unavailable"
                    ));
                };
                let updated = update_runtime_array_index(current, index, value)?;
                self.slots.insert(*slot, updated);
                Ok(ConstAssignResult::Assigned)
            }
            Some(ConstBinding::Value(_)) => Ok(ConstAssignResult::Immutable),
            None => Ok(ConstAssignResult::Unknown),
        }
    }

    fn outer_slot_ids(&self) -> Vec<ConstSlotId> {
        self.slots.keys().copied().collect()
    }

    fn commit_outer_slots_from(&mut self, scoped: &Self, outer_slots: &[ConstSlotId]) {
        for slot in outer_slots {
            if let Some(value) = scoped.slots.get(slot) {
                self.slots.insert(*slot, value.clone());
            }
        }
    }
}

impl ConstEvaluator<'_, '_> {
    const fn fresh_const_slot(&mut self) -> ConstSlotId {
        let slot = ConstSlotId(self.next_slot);
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .expect("compile-time local slot ids should not overflow");
        slot
    }

    fn eval_const(&mut self, name: &str) -> Option<RuntimeValue> {
        if let Some(value) = self.values.get(name) {
            return Some(value.clone());
        }
        let const_item = self.consts.get(name)?;
        if self.active_consts.iter().any(|active| active == name) {
            self.diagnostics.push(Diagnostic::new(
                "mir.const-eval",
                format!("failed to evaluate cyclic const `{name}` at compile time"),
                const_item.span,
                Some("Break the cycle in the const dependency graph.".to_owned()),
            ));
            return None;
        }
        self.active_consts.push(name.to_owned());
        let mut env = ConstEnv::default();
        let result = self.eval_expr_value(&const_item.value, &mut env);
        self.active_consts.pop();
        match result {
            Ok(value) => {
                self.values.insert(name.to_owned(), value.clone());
                Some(value)
            }
            Err(error) => {
                self.diagnostics.push(Diagnostic::new(
                    "mir.const-eval",
                    format!(
                        "failed to evaluate const `{name}` at compile time: {}",
                        error.message
                    ),
                    error.span,
                    Some(
                        "Keep stage-0 const items within the pure compile-time evaluator subset."
                            .to_owned(),
                    ),
                ));
                None
            }
        }
    }

    fn eval_function(
        &mut self,
        name: &str,
        args: &[RuntimeValue],
        span: Span,
    ) -> Result<RuntimeValue, ConstEvalError> {
        let function = self.functions.get(name).copied().ok_or_else(|| {
            ConstEvalError::new(span, format!("unknown helper function `{name}`"))
        })?;
        if !function.effects.is_empty() {
            return Err(ConstEvalError::new(
                function.span,
                format!("helper function `{name}` declares effects"),
            ));
        }
        let body = function.body.as_ref().ok_or_else(|| {
            ConstEvalError::new(
                function.span,
                format!("helper function `{name}` has no body"),
            )
        })?;
        if function.params.len() != args.len() {
            return Err(ConstEvalError::new(
                span,
                format!(
                    "helper function `{name}` expected {} arguments but received {}",
                    function.params.len(),
                    args.len()
                ),
            ));
        }
        if self.active_functions.iter().any(|active| active == name) {
            return Err(ConstEvalError::new(
                function.span,
                format!("helper function `{name}` is recursively referenced"),
            ));
        }
        let mut param_env = ConstEnv::default();
        for (param, arg) in function.params.iter().zip(args) {
            param_env.bind_value(param.name.clone(), arg.clone());
        }
        if let Some(requires) = &function.requires {
            self.eval_contract_clause(
                requires,
                &mut param_env,
                format!(
                    "helper function `{name}` failed its `requires` contract during compile-time evaluation"
                ),
            )?;
        }
        self.active_functions.push(name.to_owned());
        let mut body_env = param_env.clone();
        let flow = self.eval_body(body, &mut body_env);
        self.active_functions.pop();
        let ConstFlow::Value(result) = flow?;
        if let Some(ensures) = &function.ensures {
            let saved = self.contract_result.replace(result.clone());
            let ensured = self.eval_contract_clause(
                ensures,
                &mut param_env,
                format!(
                    "helper function `{name}` failed its `ensures` contract during compile-time evaluation"
                ),
            );
            self.contract_result = saved;
            ensured?;
        }
        Ok(result)
    }

    fn eval_body(
        &mut self,
        body: &sarif_frontend::hir::Body,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        for stmt in &body.statements {
            match stmt {
                Stmt::Let(binding) => {
                    let value = self.eval_expr_value(&binding.value, env)?;
                    if binding.mutable {
                        let slot = self.fresh_const_slot();
                        env.bind_slot(binding.name.clone(), slot, value);
                    } else {
                        env.bind_value(binding.name.clone(), value);
                    }
                }
                Stmt::Assign(statement) => {
                    let value = self.eval_expr_value(&statement.value, env)?;
                    match &statement.target {
                        Expr::Name(target) => match env.assign(&target.name, value) {
                            ConstAssignResult::Assigned => {}
                            ConstAssignResult::Immutable => {
                                return Err(ConstEvalError::new(
                                    statement.span,
                                    format!(
                                        "compile-time assignment targets immutable local `{}`",
                                        target.name
                                    ),
                                ));
                            }
                            ConstAssignResult::Unknown => {
                                return Err(ConstEvalError::new(
                                    statement.span,
                                    format!(
                                        "compile-time assignment targets unknown local `{}`",
                                        target.name
                                    ),
                                ));
                            }
                        },
                        Expr::Index(target) => {
                            let Expr::Name(base) = target.base.as_ref() else {
                                return Err(ConstEvalError::new(
                                    statement.span,
                                    "compile-time indexed assignment must target a local array",
                                ));
                            };
                            let index = self.eval_expr_value(&target.index, env)?;
                            let RuntimeValue::Int(index) = index else {
                                return Err(ConstEvalError::new(
                                    target.index.span(),
                                    "compile-time indexed assignment expects an Int index",
                                ));
                            };
                            match env
                                .assign_array_index(&base.name, index, value)
                                .map_err(|message| ConstEvalError::new(statement.span, message))?
                            {
                                ConstAssignResult::Assigned => {}
                                ConstAssignResult::Immutable => {
                                    return Err(ConstEvalError::new(
                                        statement.span,
                                        format!(
                                            "compile-time assignment targets immutable local `{}`",
                                            base.name
                                        ),
                                    ));
                                }
                                ConstAssignResult::Unknown => {
                                    return Err(ConstEvalError::new(
                                        statement.span,
                                        format!(
                                            "compile-time assignment targets unknown local `{}`",
                                            base.name
                                        ),
                                    ));
                                }
                            }
                        }
                        _ => {
                            return Err(ConstEvalError::new(
                                statement.span,
                                "compile-time assignment target must be a local or local array element",
                            ));
                        }
                    }
                }
                Stmt::Expr(stmt) => {
                    let _ = self.eval_expr_value(&stmt.expr, env)?;
                }
            }
        }
        body.tail
            .as_ref()
            .map_or(Ok(ConstFlow::Value(RuntimeValue::Unit)), |tail| {
                self.eval_expr_flow(tail, env)
            })
    }

    fn eval_nested_body(
        &mut self,
        body: &sarif_frontend::hir::Body,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let outer_slots = env.outer_slot_ids();
        let mut scoped = env.clone();
        let flow = self.eval_body(body, &mut scoped)?;
        env.commit_outer_slots_from(&scoped, &outer_slots);
        Ok(flow)
    }

    fn eval_nested_body_with_binding(
        &mut self,
        body: &sarif_frontend::hir::Body,
        env: &mut ConstEnv,
        binding: Option<(String, RuntimeValue)>,
    ) -> Result<ConstFlow, ConstEvalError> {
        let outer_slots = env.outer_slot_ids();
        let mut scoped = env.clone();
        if let Some((name, value)) = binding {
            scoped.bind_value(name, value);
        }
        let flow = self.eval_body(body, &mut scoped)?;
        env.commit_outer_slots_from(&scoped, &outer_slots);
        Ok(flow)
    }

    fn eval_expr_value(
        &mut self,
        expr: &Expr,
        env: &mut ConstEnv,
    ) -> Result<RuntimeValue, ConstEvalError> {
        match self.eval_expr_flow(expr, env)? {
            ConstFlow::Value(value) => Ok(value),
        }
    }

    fn eval_expr_flow(
        &mut self,
        expr: &Expr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        match expr {
            Expr::Integer(expr) => Ok(ConstFlow::Value(RuntimeValue::Int(expr.value))),
            Expr::Float(expr) => Ok(ConstFlow::Value(RuntimeValue::F64(expr.value))),
            Expr::String(expr) => Ok(ConstFlow::Value(RuntimeValue::Text(expr.value.clone()))),
            Expr::Bool(expr) => Ok(ConstFlow::Value(RuntimeValue::Bool(expr.value))),
            Expr::Name(expr) => env.read(&expr.name).map_or_else(
                || {
                    self.eval_const(&expr.name)
                        .map(ConstFlow::Value)
                        .ok_or_else(|| {
                            ConstEvalError::new(
                                expr.span,
                                format!("unknown compile-time name `{}`", expr.name),
                            )
                        })
                },
                |value| Ok(ConstFlow::Value(value)),
            ),
            Expr::ContractResult(expr) => self
                .contract_result
                .clone()
                .map(ConstFlow::Value)
                .ok_or_else(|| {
                    ConstEvalError::new(
                        expr.span,
                        "`result` is not available during compile-time evaluation",
                    )
                }),
            Expr::Call(expr) => self.eval_call_expr(expr, env),
            Expr::Array(expr) => self.eval_array_expr(expr, env),
            Expr::Field(expr) => self.eval_field_expr(expr, env),
            Expr::Index(expr) => self.eval_index_expr(expr, env),
            Expr::If(expr) => {
                let condition = self.eval_expr_value(&expr.condition, env)?;
                let RuntimeValue::Bool(condition) = condition else {
                    return Err(ConstEvalError::new(
                        expr.condition.span(),
                        "compile-time `if` conditions must be `Bool`",
                    ));
                };
                if condition {
                    self.eval_nested_body(&expr.then_body, env)
                } else {
                    self.eval_nested_body(&expr.else_body, env)
                }
            }
            Expr::Match(expr) => self.eval_match_expr(expr, env),
            Expr::While(expr) => self.eval_while_expr(expr, env),
            Expr::Repeat(expr) => self.eval_repeat_expr(expr, env),
            Expr::Record(expr) => self.eval_record_expr(expr, env),
            Expr::Unary(expr) => self.eval_unary_expr(expr, env),
            Expr::Binary(expr) => self.eval_binary_expr(expr, env),
            Expr::Group(expr) => self.eval_expr_flow(&expr.inner, env),
            Expr::Comptime(body) => self.eval_body(body, &mut ConstEnv::default()),
            Expr::Handle(expr) => Err(ConstEvalError::new(
                expr.body.span,
                "effect handlers are not yet supported in stage-0 compile-time evaluation",
            )),
            Expr::Perform(expr) => Err(ConstEvalError::new(
                expr.span,
                "effect operations are not yet supported in stage-0 compile-time evaluation",
            )),
        }
    }

    fn eval_call_expr(
        &mut self,
        expr: &sarif_frontend::hir::CallExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        if expr.callee == "len" && !self.functions.contains_key("len") {
            return self.eval_len_expr(expr, env);
        }
        if expr.callee == "text_len" && !self.functions.contains_key("text_len") {
            let [arg] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_len expects 1 argument",
                ));
            };
            let value = self.eval_expr_value(arg, env)?;
            let RuntimeValue::Text(text) = value else {
                return Err(ConstEvalError::new(expr.span, "text_len expects a Text"));
            };
            return Ok(ConstFlow::Value(RuntimeValue::Int(text.len() as i64)));
        }
        if expr.callee == "text_concat" && !self.functions.contains_key("text_concat") {
            let [arg0, arg1] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_concat expects 2 arguments",
                ));
            };
            let left_val = self.eval_expr_value(arg0, env)?;
            let right_val = self.eval_expr_value(arg1, env)?;
            let RuntimeValue::Text(left) = left_val else {
                return Err(ConstEvalError::new(expr.span, "text_concat expects Text"));
            };
            let RuntimeValue::Text(right) = right_val else {
                return Err(ConstEvalError::new(expr.span, "text_concat expects Text"));
            };
            let mut value = left;
            value.push_str(&right);
            return Ok(ConstFlow::Value(RuntimeValue::Text(value)));
        }
        if expr.callee == "text_slice" && !self.functions.contains_key("text_slice") {
            let [arg0, arg1, arg2] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_slice expects 3 arguments",
                ));
            };
            let text_val = self.eval_expr_value(arg0, env)?;
            let start_val = self.eval_expr_value(arg1, env)?;
            let end_val = self.eval_expr_value(arg2, env)?;
            let RuntimeValue::Text(text) = text_val else {
                return Err(ConstEvalError::new(expr.span, "text_slice expects Text"));
            };
            let RuntimeValue::Int(start) = start_val else {
                return Err(ConstEvalError::new(expr.span, "text_slice expects Int"));
            };
            let RuntimeValue::Int(end) = end_val else {
                return Err(ConstEvalError::new(expr.span, "text_slice expects Int"));
            };
            return Ok(ConstFlow::Value(RuntimeValue::Text(slice_text(
                &text, start, end,
            ))));
        }
        if expr.callee == "text_byte" && !self.functions.contains_key("text_byte") {
            let [arg0, arg1] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_byte expects 2 arguments",
                ));
            };
            let text_val = self.eval_expr_value(arg0, env)?;
            let index_val = self.eval_expr_value(arg1, env)?;
            let RuntimeValue::Text(text) = text_val else {
                return Err(ConstEvalError::new(expr.span, "text_byte expects Text"));
            };
            let RuntimeValue::Int(index) = index_val else {
                return Err(ConstEvalError::new(expr.span, "text_byte expects Int"));
            };
            let byte = text.as_bytes().get(index as usize).copied().unwrap_or(0);
            return Ok(ConstFlow::Value(RuntimeValue::Int(byte as i64)));
        }
        if expr.callee == "text_from_f64_fixed"
            && !self.functions.contains_key("text_from_f64_fixed")
        {
            let [arg0, arg1] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_from_f64_fixed expects 2 arguments",
                ));
            };
            let value = self.eval_expr_value(arg0, env)?;
            let digits = self.eval_expr_value(arg1, env)?;
            let RuntimeValue::F64(value) = value else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_from_f64_fixed expects F64",
                ));
            };
            let RuntimeValue::Int(digits) = digits else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "text_from_f64_fixed expects Int",
                ));
            };
            return Ok(ConstFlow::Value(RuntimeValue::Text(format_f64_fixed(
                value, digits,
            ))));
        }
        if expr.callee == "sqrt" && !self.functions.contains_key("sqrt") {
            let [arg] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(expr.span, "sqrt expects 1 argument"));
            };
            let value = self.eval_expr_value(arg, env)?;
            let RuntimeValue::F64(value) = value else {
                return Err(ConstEvalError::new(expr.span, "sqrt expects F64"));
            };
            return Ok(ConstFlow::Value(RuntimeValue::F64(value.sqrt())));
        }
        if expr.callee == "f64_from_i32" && !self.functions.contains_key("f64_from_i32") {
            let [arg] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "f64_from_i32 expects 1 argument",
                ));
            };
            let value = self.eval_expr_value(arg, env)?;
            let RuntimeValue::Int(value) = value else {
                return Err(ConstEvalError::new(expr.span, "f64_from_i32 expects Int"));
            };
            return Ok(ConstFlow::Value(RuntimeValue::F64(value as f64)));
        }
        if expr.callee == "parse_i32" && !self.functions.contains_key("parse_i32") {
            let [arg] = expr.args.as_slice() else {
                return Err(ConstEvalError::new(
                    expr.span,
                    "parse_i32 expects 1 argument",
                ));
            };
            let value = self.eval_expr_value(arg, env)?;
            let RuntimeValue::Text(text) = value else {
                return Err(ConstEvalError::new(expr.span, "parse_i32 expects Text"));
            };
            let parsed = text.trim().parse::<i64>().map_err(|_| {
                ConstEvalError::new(expr.span, "parse_i32 expects a base-10 integer")
            })?;
            return Ok(ConstFlow::Value(RuntimeValue::Int(parsed)));
        }
        if let Some((enum_name, variant_name)) = split_enum_variant_path(&expr.callee) {
            let variant = self
                .enum_variants
                .get(enum_name)
                .and_then(|variants| variants.iter().find(|variant| variant.name == variant_name))
                .ok_or_else(|| {
                    ConstEvalError::new(
                        expr.span,
                        format!("unknown enum constructor `{}`", expr.callee),
                    )
                })?;
            let payload = match (&variant.payload_type, expr.args.as_slice()) {
                (Some(_), [arg]) => Some(Box::new(self.eval_expr_value(arg, env)?)),
                (None, []) => None,
                (Some(_), _) => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        format!(
                            "enum constructor `{}` requires one payload argument",
                            expr.callee
                        ),
                    ));
                }
                (None, _) => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        format!(
                            "enum constructor `{}` does not accept payload arguments",
                            expr.callee
                        ),
                    ));
                }
            };
            return Ok(ConstFlow::Value(RuntimeValue::Enum(RuntimeEnum {
                name: enum_name.to_owned(),
                variant: variant_name.to_owned(),
                payload,
            })));
        }
        let args = expr
            .args
            .iter()
            .map(|arg| self.eval_expr_value(arg, env))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ConstFlow::Value(self.eval_function(
            &expr.callee,
            &args,
            expr.span,
        )?))
    }

    fn eval_len_expr(
        &mut self,
        expr: &sarif_frontend::hir::CallExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let [arg] = expr.args.as_slice() else {
            return Err(ConstEvalError::new(
                expr.span,
                "builtin `len` expects exactly one array argument",
            ));
        };
        let value = self.eval_expr_value(arg, env)?;
        let RuntimeValue::Record(record) = value else {
            return Err(ConstEvalError::new(
                expr.span,
                "builtin `len` expects an internal array value",
            ));
        };
        let Some((_, len)) = synthetic_array_record_info(&record) else {
            return Err(ConstEvalError::new(
                expr.span,
                "builtin `len` expects an internal array value",
            ));
        };
        let Ok(len) = i64::try_from(len) else {
            return Err(ConstEvalError::new(
                expr.span,
                "array length exceeds stage-0 integer limits",
            ));
        };
        Ok(ConstFlow::Value(RuntimeValue::Int(len)))
    }

    fn eval_array_expr(
        &mut self,
        expr: &sarif_frontend::hir::ArrayExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let mut elements = Vec::with_capacity(expr.elements.len());
        for element in &expr.elements {
            elements.push(self.eval_expr_value(element, env)?);
        }
        let Some(first) = elements.first() else {
            return Err(ConstEvalError::new(
                expr.span,
                "empty arrays are not admitted in stage-0 compile-time evaluation",
            ));
        };
        let element_ty = runtime_value_lower_type(first);
        if elements
            .iter()
            .skip(1)
            .any(|element| runtime_value_lower_type(element) != element_ty)
        {
            return Err(ConstEvalError::new(
                expr.span,
                "compile-time array literals must use one element type",
            ));
        }
        let Some(element_name) = lower_type_name(&element_ty) else {
            return Err(ConstEvalError::new(
                expr.span,
                "compile-time array literals use an unsupported element type",
            ));
        };
        Ok(ConstFlow::Value(RuntimeValue::Record(RuntimeRecord {
            name: array_struct_name(&element_name, elements.len()),
            fields: elements
                .into_iter()
                .enumerate()
                .map(|(index, value)| (array_field_name(index), value))
                .collect(),
        })))
    }

    fn eval_index_expr(
        &mut self,
        expr: &sarif_frontend::hir::IndexExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let base = self.eval_expr_value(&expr.base, env)?;
        let RuntimeValue::Record(record) = base else {
            return Err(ConstEvalError::new(
                expr.base.span(),
                "compile-time indexing requires an internal array value",
            ));
        };
        let Some((_, len)) = synthetic_array_record_info(&record) else {
            return Err(ConstEvalError::new(
                expr.base.span(),
                "compile-time indexing requires an internal array value",
            ));
        };
        let index = expect_const_int(&self.eval_expr_value(&expr.index, env)?, expr.index.span())?;
        if index < 0 {
            return Err(ConstEvalError::new(
                expr.index.span(),
                "compile-time array indices must be non-negative",
            ));
        }
        let Ok(index) = usize::try_from(index) else {
            return Err(ConstEvalError::new(
                expr.index.span(),
                "compile-time array index exceeds platform limits",
            ));
        };
        let Some((_, value)) = record.fields.get(index) else {
            return Err(ConstEvalError::new(
                expr.span,
                format!("compile-time array index {index} is out of bounds for length {len}"),
            ));
        };
        Ok(ConstFlow::Value(value.clone()))
    }

    fn eval_field_expr(
        &mut self,
        expr: &sarif_frontend::hir::FieldExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        if let Expr::Name(base_name) = &*expr.base
            && let Some(value) = self.payload_free_enum_variant_value(&base_name.name, &expr.field)
        {
            return Ok(ConstFlow::Value(value));
        }
        let base = self.eval_expr_value(&expr.base, env)?;
        let RuntimeValue::Record(record) = base else {
            return Err(ConstEvalError::new(
                expr.span,
                format!("expected record value for field access `{}`", expr.field),
            ));
        };
        let value = record
            .fields
            .iter()
            .find_map(|(name, value)| (name == &expr.field).then(|| value.clone()))
            .ok_or_else(|| {
                ConstEvalError::new(
                    expr.span,
                    format!("record `{}` has no field `{}`", record.name, expr.field),
                )
            })?;
        Ok(ConstFlow::Value(value))
    }

    fn eval_match_expr(
        &mut self,
        expr: &sarif_frontend::hir::MatchExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let scrutinee = self.eval_expr_value(&expr.scrutinee, env)?;
        self.eval_match_arms(&scrutinee, &expr.arms, env, expr.span)
    }

    fn eval_repeat_expr(
        &mut self,
        expr: &sarif_frontend::hir::RepeatExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let count = self.eval_expr_value(&expr.count, env)?;
        let RuntimeValue::Int(count) = count else {
            return Err(ConstEvalError::new(
                expr.count.span(),
                "compile-time `repeat` counts must be `I32`",
            ));
        };
        if count > 0 {
            for index in 0..count {
                let binding = expr
                    .binding
                    .as_ref()
                    .map(|binding| (binding.clone(), RuntimeValue::Int(index)));
                match self.eval_nested_body_with_binding(&expr.body, env, binding)? {
                    ConstFlow::Value(_) => {}
                }
            }
        }
        Ok(ConstFlow::Value(RuntimeValue::Unit))
    }

    fn eval_while_expr(
        &mut self,
        expr: &sarif_frontend::hir::WhileExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        loop {
            let condition = self.eval_expr_value(&expr.condition, env)?;
            let RuntimeValue::Bool(condition) = condition else {
                return Err(ConstEvalError::new(
                    expr.condition.span(),
                    "compile-time `while` conditions must be `Bool`",
                ));
            };
            if !condition {
                break;
            }
            match self.eval_nested_body(&expr.body, env)? {
                ConstFlow::Value(_) => {}
            }
        }
        Ok(ConstFlow::Value(RuntimeValue::Unit))
    }

    fn eval_record_expr(
        &mut self,
        expr: &sarif_frontend::hir::RecordExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let fields = if let Some(declared_fields) = self.struct_fields.get(&expr.name) {
            declared_fields
                .iter()
                .filter_map(|declared| {
                    expr.fields
                        .iter()
                        .find(|field| &field.name == declared)
                        .map(|field| {
                            self.eval_expr_value(&field.value, env)
                                .map(|value| (declared.clone(), value))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            expr.fields
                .iter()
                .map(|field| {
                    self.eval_expr_value(&field.value, env)
                        .map(|value| (field.name.clone(), value))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(ConstFlow::Value(RuntimeValue::Record(RuntimeRecord {
            name: expr.name.clone(),
            fields,
        })))
    }

    fn eval_binary_expr(
        &mut self,
        expr: &sarif_frontend::hir::BinaryExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let left = self.eval_expr_value(&expr.left, env)?;
        let right = self.eval_expr_value(&expr.right, env)?;
        let value = match expr.op {
            BinaryOp::Add => match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                    RuntimeValue::Int(left + right)
                }
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                    RuntimeValue::F64(left + right)
                }
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time arithmetic operands must both be `I32` or both be `F64`",
                    ));
                }
            },
            BinaryOp::Sub => match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                    RuntimeValue::Int(left - right)
                }
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                    RuntimeValue::F64(left - right)
                }
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time arithmetic operands must both be `I32` or both be `F64`",
                    ));
                }
            },
            BinaryOp::Mul => match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                    RuntimeValue::Int(left * right)
                }
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                    RuntimeValue::F64(left * right)
                }
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time arithmetic operands must both be `I32` or both be `F64`",
                    ));
                }
            },
            BinaryOp::Div => match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                    if *right == 0 {
                        return Err(ConstEvalError::new(expr.span, "division by zero"));
                    }
                    RuntimeValue::Int(left / right)
                }
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                    if *right == 0.0 {
                        return Err(ConstEvalError::new(expr.span, "division by zero"));
                    }
                    RuntimeValue::F64(left / right)
                }
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time arithmetic operands must both be `I32` or both be `F64`",
                    ));
                }
            },
            BinaryOp::And => RuntimeValue::Bool(
                expect_const_bool(&left, expr.left.span())?
                    && expect_const_bool(&right, expr.right.span())?,
            ),
            BinaryOp::Or => RuntimeValue::Bool(
                expect_const_bool(&left, expr.left.span())?
                    || expect_const_bool(&right, expr.right.span())?,
            ),
            BinaryOp::Eq => RuntimeValue::Bool(left == right),
            BinaryOp::Ne => RuntimeValue::Bool(left != right),
            BinaryOp::Lt => RuntimeValue::Bool(match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left < right,
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left < right,
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time comparison operands must both be `I32` or both be `F64`",
                    ));
                }
            }),
            BinaryOp::Le => RuntimeValue::Bool(match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left <= right,
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left <= right,
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time comparison operands must both be `I32` or both be `F64`",
                    ));
                }
            }),
            BinaryOp::Gt => RuntimeValue::Bool(match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left > right,
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left > right,
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time comparison operands must both be `I32` or both be `F64`",
                    ));
                }
            }),
            BinaryOp::Ge => RuntimeValue::Bool(match (&left, &right) {
                (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left >= right,
                (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left >= right,
                _ => {
                    return Err(ConstEvalError::new(
                        expr.span,
                        "compile-time comparison operands must both be `I32` or both be `F64`",
                    ));
                }
            }),
        };
        Ok(ConstFlow::Value(value))
    }

    fn eval_unary_expr(
        &mut self,
        expr: &sarif_frontend::hir::UnaryExpr,
        env: &mut ConstEnv,
    ) -> Result<ConstFlow, ConstEvalError> {
        let inner = self.eval_expr_value(&expr.inner, env)?;
        let value = match expr.op {
            sarif_frontend::hir::UnaryOp::Not => {
                RuntimeValue::Bool(!expect_const_bool(&inner, expr.inner.span())?)
            }
        };
        Ok(ConstFlow::Value(value))
    }

    fn eval_match_arms(
        &mut self,
        scrutinee: &RuntimeValue,
        arms: &[sarif_frontend::hir::MatchArm],
        env: &mut ConstEnv,
        span: Span,
    ) -> Result<ConstFlow, ConstEvalError> {
        for arm in arms {
            match Self::match_pattern(scrutinee, &arm.pattern, arm.span)? {
                PatternMatch::NoMatch => {}
                PatternMatch::Match(bound) => {
                    return self.eval_nested_body_with_binding(&arm.body, env, bound);
                }
            }
        }
        Err(ConstEvalError::new(
            span,
            "no `match` arm matched during compile-time evaluation",
        ))
    }

    fn match_pattern(
        scrutinee: &RuntimeValue,
        pattern: &sarif_frontend::hir::MatchPattern,
        span: Span,
    ) -> Result<PatternMatch, ConstEvalError> {
        match pattern {
            sarif_frontend::hir::MatchPattern::Variant { path, binding, .. } => {
                let Some((enum_name, variant_name)) = split_enum_variant_path(&path.path) else {
                    return Err(ConstEvalError::new(
                        span,
                        format!("invalid enum pattern `{}`", path.path),
                    ));
                };
                let RuntimeValue::Enum(enum_value) = scrutinee else {
                    return Ok(PatternMatch::NoMatch);
                };
                if enum_value.name != enum_name || enum_value.variant != variant_name {
                    return Ok(PatternMatch::NoMatch);
                }
                match (binding, &enum_value.payload) {
                    (Some(binding), Some(payload)) => Ok(PatternMatch::Match(Some((
                        binding.clone(),
                        (**payload).clone(),
                    )))),
                    (Some(_), None) => Err(ConstEvalError::new(
                        span,
                        format!("pattern `{}` does not carry a payload", path.path),
                    )),
                    (None, _) => Ok(PatternMatch::Match(None)),
                }
            }
            sarif_frontend::hir::MatchPattern::Integer { value, .. } => {
                Ok(if scrutinee == &RuntimeValue::Int(*value) {
                    PatternMatch::Match(None)
                } else {
                    PatternMatch::NoMatch
                })
            }
            sarif_frontend::hir::MatchPattern::String { value, .. } => {
                Ok(if scrutinee == &RuntimeValue::Text(value.clone()) {
                    PatternMatch::Match(None)
                } else {
                    PatternMatch::NoMatch
                })
            }
            sarif_frontend::hir::MatchPattern::Bool { value, .. } => {
                Ok(if scrutinee == &RuntimeValue::Bool(*value) {
                    PatternMatch::Match(None)
                } else {
                    PatternMatch::NoMatch
                })
            }
            sarif_frontend::hir::MatchPattern::Wildcard { .. } => Ok(PatternMatch::Match(None)),
        }
    }

    fn payload_free_enum_variant_value(
        &self,
        enum_name: &str,
        variant_name: &str,
    ) -> Option<RuntimeValue> {
        self.enum_variants
            .get(enum_name)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|variant| variant.name == variant_name && variant.payload_type.is_none())
            })
            .map(|_| {
                RuntimeValue::Enum(RuntimeEnum {
                    name: enum_name.to_owned(),
                    variant: variant_name.to_owned(),
                    payload: None,
                })
            })
    }

    fn eval_contract_clause(
        &mut self,
        expr: &Expr,
        env: &mut ConstEnv,
        failure_message: String,
    ) -> Result<(), ConstEvalError> {
        let value = self.eval_expr_value(expr, env)?;
        let RuntimeValue::Bool(value) = value else {
            return Err(ConstEvalError::new(
                expr.span(),
                "compile-time contract clauses must evaluate to `Bool`",
            ));
        };
        if value {
            Ok(())
        } else {
            Err(ConstEvalError::new(expr.span(), failure_message))
        }
    }
}

fn expect_const_int(value: &RuntimeValue, span: Span) -> Result<i64, ConstEvalError> {
    match value {
        RuntimeValue::Int(value) => Ok(*value),
        other => Err(ConstEvalError::new(
            span,
            format!(
                "expected `I32` during compile-time evaluation, found {}",
                other.render()
            ),
        )),
    }
}

fn expect_const_bool(value: &RuntimeValue, span: Span) -> Result<bool, ConstEvalError> {
    match value {
        RuntimeValue::Bool(value) => Ok(*value),
        other => Err(ConstEvalError::new(
            span,
            format!(
                "expected `Bool` during compile-time evaluation, found {}",
                other.render()
            ),
        )),
    }
}

/// # Errors
///
/// Returns an error if the program has no `main` entrypoint, if `main`
/// requires parameters, if a callee is missing at runtime, or if evaluation
/// observes an invalid runtime state such as division by zero.
pub fn run_main(program: &Program) -> Result<RuntimeValue, RuntimeError> {
    let mut interpreter = Interpreter::new(program, &[], String::new());
    interpreter.run_main()
}

/// # Errors
///
/// Returns an error if the program has no `main` entrypoint, if `main`
/// requires parameters, if a callee is missing at runtime, or if evaluation
/// observes an invalid runtime state such as division by zero.
pub fn run_main_with_args(
    program: &Program,
    args: &[String],
) -> Result<RuntimeValue, RuntimeError> {
    let mut interpreter = Interpreter::new(program, args, String::new());
    let result = interpreter.run_main();
    print!("{}", interpreter.take_stdout());
    use std::io::Write;
    let _ = std::io::stdout().flush();
    result
}

/// # Errors
///
/// Returns an error if the program has no `main` entrypoint, if `main`
/// requires parameters, if a callee is missing at runtime, or if evaluation
/// observes an invalid runtime state such as division by zero.
pub fn run_main_with_io(
    program: &Program,
    program_args: &[String],
    stdin_text: String,
) -> Result<RuntimeValue, RuntimeError> {
    let mut interpreter = Interpreter::new(program, program_args, stdin_text);
    interpreter.run_main()
}

/// # Errors
///
/// Returns an error if the program has no `main` entrypoint, if `main`
/// requires parameters, if a callee is missing at runtime, or if evaluation
/// observes an invalid runtime state such as division by zero.
pub fn run_main_with_io_capture(
    program: &Program,
    program_args: &[String],
    stdin_text: String,
) -> Result<(RuntimeValue, String), RuntimeError> {
    let mut interpreter = Interpreter::new(program, program_args, stdin_text);
    let value = interpreter.run_main()?;
    Ok((value, interpreter.take_stdout()))
}

/// # Errors
///
/// Returns an error if the named function does not exist, if its argument
/// count or types do not match, if a callee is missing at runtime, or if
/// evaluation observes an invalid runtime state.
pub fn run_function(
    program: &Program,
    name: &str,
    args: &[RuntimeValue],
) -> Result<RuntimeValue, RuntimeError> {
    let mut interpreter = Interpreter::new(program, &[], String::new());
    interpreter.run_function(name, args)
}

#[cfg(any(feature = "backend-native", feature = "backend-wasm"))]
pub(crate) fn insts_fall_through(instructions: &[Inst]) -> bool {
    for inst in instructions {
        match inst {
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                if !(insts_fall_through(then_insts) || insts_fall_through(else_insts)) {
                    return false;
                }
            }
            Inst::While { .. }
            | Inst::Repeat { .. }
            | Inst::LoadParam { .. }
            | Inst::LoadLocal { .. }
            | Inst::StoreLocal { .. }
            | Inst::ConstInt { .. }
            | Inst::ConstF64 { .. }
            | Inst::ConstBool { .. }
            | Inst::ConstText { .. }
            | Inst::TextBuilderNew { .. }
            | Inst::TextBuilderAppend { .. }
            | Inst::TextBuilderAppendCodepoint { .. }
            | Inst::TextBuilderFinish { .. }
            | Inst::ListNew { .. }
            | Inst::ListLen { .. }
            | Inst::ListGet { .. }
            | Inst::ListSet { .. }
            | Inst::F64FromI32 { .. }
            | Inst::TextLen { .. }
            | Inst::TextConcat { .. }
            | Inst::TextSlice { .. }
            | Inst::TextByte { .. }
            | Inst::TextCmp { .. }
            | Inst::TextEqRange { .. }
            | Inst::TextFindByteRange { .. }
            | Inst::TextFromF64Fixed { .. }
            | Inst::ArgCount { .. }
            | Inst::AllocPush
            | Inst::AllocPop
            | Inst::ArgText { .. }
            | Inst::StdinText { .. }
            | Inst::StdoutWrite { .. }
            | Inst::ParseI32 { .. }
            | Inst::ParseI32Range { .. }
            | Inst::ParseF64 { .. }
            | Inst::MakeEnum { .. }
            | Inst::MakeRecord { .. }
            | Inst::Field { .. }
            | Inst::EnumTagEq { .. }
            | Inst::EnumPayload { .. }
            | Inst::Add { .. }
            | Inst::Sub { .. }
            | Inst::Mul { .. }
            | Inst::Div { .. }
            | Inst::Sqrt { .. }
            | Inst::And { .. }
            | Inst::Or { .. }
            | Inst::Eq { .. }
            | Inst::Ne { .. }
            | Inst::Lt { .. }
            | Inst::Le { .. }
            | Inst::Gt { .. }
            | Inst::Ge { .. }
            | Inst::Call { .. }
            | Inst::Assert { .. }
            | Inst::Perform { .. }
            | Inst::Handle { .. } => {}
        }
    }
    true
}

fn lower_function<'a>(
    function: &'a sarif_frontend::hir::Function,
    shared: &mut LowerShared<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Function {
    lower_function_monomorphized(function, shared, diagnostics, HashMap::new(), None)
}

fn lower_function_monomorphized<'a>(
    function: &'a sarif_frontend::hir::Function,
    shared: &mut LowerShared<'a>,
    diagnostics: &mut Vec<Diagnostic>,
    substitutions: HashMap<String, usize>,
    new_name: Option<String>,
) -> Function {
    let mut lowerer = FunctionLowerer::new(function, shared, substitutions.clone());
    if let Some(requires) = &function.requires {
        let condition = lowerer.lower_expr(requires);
        lowerer.instructions.push(Inst::Assert {
            condition,
            kind: ContractKind::Requires,
        });
    }
    let result = function.body.as_ref().and_then(|body| {
        let body = lowerer.lower_body(body, true);
        body.result
    });
    lowerer.contract_result = result;
    if let Some(ensures) = &function.ensures {
        let condition = lowerer.lower_expr(ensures);
        lowerer.instructions.push(Inst::Assert {
            condition,
            kind: ContractKind::Ensures,
        });
    }
    diagnostics.extend(lowerer.diagnostics);

    Function {
        name: new_name.unwrap_or_else(|| function.name.clone()),
        params: function
            .params
            .iter()
            .map(|param| Param {
                name: param.name.clone(),
                ty: LowerType::from_type_name(&param.ty.path, &substitutions)
                    .type_name()
                    .unwrap_or_else(|| param.ty.path.clone()),
            })
            .collect(),
        return_type: function.return_type.as_ref().map(|ty| {
            LowerType::from_type_name(&ty.path, &substitutions)
                .type_name()
                .unwrap_or_else(|| ty.path.clone())
        }),
        effects: function
            .effects
            .iter()
            .map(|effect| effect.name().to_owned())
            .collect(),
        mutable_locals: lowerer.mutable_locals,
        instructions: lowerer.instructions,
        result,
        span: function.span,
    }
}

struct FunctionLowerer<'a, 'shared> {
    function: &'a sarif_frontend::hir::Function,
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantType>>,
    struct_fields: &'a BTreeMap<String, Vec<String>>,
    struct_layouts: &'a BTreeMap<String, Vec<(String, String)>>,
    function_returns: &'a BTreeMap<String, String>,
    evaluated_consts: &'a BTreeMap<String, RuntimeValue>,
    generated_arrays: &'shared mut BTreeMap<String, GeneratedArrayType>,
    generic_functions: &'a BTreeMap<String, &'a sarif_frontend::hir::Function>,
    all_functions: &'a BTreeMap<String, &'a sarif_frontend::hir::Function>,
    monomorphized_functions: &'shared mut Vec<Function>,
    substitutions: HashMap<String, usize>,
    next_value: u32,
    next_slot: u32,
    instructions: Vec<Inst>,
    locals: HashMap<String, LocalBinding>,
    local_types: HashMap<String, LowerType>,
    mutable_locals: Vec<MutableLocal>,
    contract_result: Option<ValueId>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug)]
enum LocalBinding {
    Value(ValueId),
    Slot(LocalSlotId),
}

struct BodyLowering {
    result: Option<ValueId>,
    falls_through: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LowerType {
    I32,
    F64,
    Bool,
    Text,
    TextBuilder,
    List(Box<Self>),
    Unit,
    Named(String),
    Array(Box<Self>, usize),
    Error,
}

impl LowerType {
    fn from_type_name(name: &str, substitutions: &HashMap<String, usize>) -> Self {
        match name {
            "I32" => Self::I32,
            "F64" => Self::F64,
            "Bool" => Self::Bool,
            "Text" => Self::Text,
            "TextBuilder" => Self::TextBuilder,
            "Unit" => Self::Unit,
            other if other.starts_with("List[") && other.ends_with(']') => {
                let inner = &other[5..other.len() - 1];
                let element = Self::from_type_name(inner, substitutions);
                Self::List(Box::new(element))
            }
            other => parse_array_lower_type(other, substitutions)
                .unwrap_or_else(|| Self::Named(other.to_owned())),
        }
    }

    fn type_name(&self) -> Option<String> {
        match self {
            Self::I32 => Some("I32".to_owned()),
            Self::F64 => Some("F64".to_owned()),
            Self::Bool => Some("Bool".to_owned()),
            Self::Text => Some("Text".to_owned()),
            Self::TextBuilder => Some("TextBuilder".to_owned()),
            Self::List(element) => Some(format!("List[{}]", lower_type_name(element)?)),
            Self::Unit => Some("Unit".to_owned()),
            Self::Named(name) => Some(name.clone()),
            Self::Array(_, _) | Self::Error => None,
        }
    }
}

fn parse_array_lower_type(name: &str, substitutions: &HashMap<String, usize>) -> Option<LowerType> {
    let inner = name.strip_prefix('[')?.strip_suffix(']')?;
    let mut depth = 0usize;
    let mut split = None::<usize>;
    for (index, ch) in inner.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                split = Some(index);
                break;
            }
            _ => {}
        }
    }
    let split = split?;
    let element_str = inner[..split].trim();
    let len_str = inner[split + 1..].trim();
    let len = if let Ok(l) = len_str.parse::<usize>() {
        l
    } else {
        *substitutions.get(len_str)?
    };
    Some(LowerType::Array(
        Box::new(LowerType::from_type_name(element_str, substitutions)),
        len,
    ))
}

fn lower_type_name(ty: &LowerType) -> Option<String> {
    match ty {
        LowerType::Array(element, len) => Some(array_struct_name(&lower_type_name(element)?, *len)),
        _ => ty.type_name(),
    }
}

#[derive(Clone, Debug)]
struct GeneratedArrayType {
    name: String,
    element_ty: String,
    len: usize,
}

struct LowerShared<'a> {
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantType>>,
    struct_fields: &'a BTreeMap<String, Vec<String>>,
    struct_layouts: &'a BTreeMap<String, Vec<(String, String)>>,
    function_returns: &'a BTreeMap<String, String>,
    evaluated_consts: &'a BTreeMap<String, RuntimeValue>,
    generated_arrays: &'a mut BTreeMap<String, GeneratedArrayType>,
    generic_functions: &'a BTreeMap<String, &'a sarif_frontend::hir::Function>,
    all_functions: &'a BTreeMap<String, &'a sarif_frontend::hir::Function>,
    monomorphized_functions: Vec<Function>,
}

impl<'a, 'shared> FunctionLowerer<'a, 'shared> {
    fn new(
        function: &'a sarif_frontend::hir::Function,
        shared: &'shared mut LowerShared<'a>,
        substitutions: HashMap<String, usize>,
    ) -> Self {
        let mut lowerer = Self {
            function,
            enum_variants: shared.enum_variants,
            struct_fields: shared.struct_fields,
            struct_layouts: shared.struct_layouts,
            function_returns: shared.function_returns,
            evaluated_consts: shared.evaluated_consts,
            generated_arrays: shared.generated_arrays,
            generic_functions: shared.generic_functions,
            all_functions: shared.all_functions,
            monomorphized_functions: &mut shared.monomorphized_functions,
            substitutions: substitutions.clone(),
            next_value: 0,
            next_slot: 0,
            instructions: Vec::new(),
            locals: HashMap::new(),
            local_types: HashMap::new(),
            mutable_locals: Vec::new(),
            contract_result: None,
            diagnostics: Vec::new(),
        };
        for (index, param) in function.params.iter().enumerate() {
            let dest = lowerer.fresh_value();
            lowerer
                .locals
                .insert(param.name.clone(), LocalBinding::Value(dest));
            lowerer.local_types.insert(
                param.name.clone(),
                LowerType::from_type_name(&param.ty.path, &substitutions),
            );
            lowerer.instructions.push(Inst::LoadParam { dest, index });
        }
        lowerer
    }

    fn lower_body(&mut self, body: &sarif_frontend::hir::Body, _top_level: bool) -> BodyLowering {
        for statement in &body.statements {
            match statement {
                Stmt::Let(binding) => {
                    let value = self.lower_expr(&binding.value);
                    let ty = self.infer_expr_type(&binding.value);
                    if binding.mutable {
                        let Some(slot_ty) = self.register_type_name(&ty) else {
                            self.diagnostics.push(Diagnostic::new(
                                "mir.unsupported-mutable-local",
                                format!(
                                    "mutable local `{}` in `{}` uses a stage-0 unsupported type",
                                    binding.name, self.function.name
                                ),
                                binding.span,
                                Some(
                                    "Keep mutable locals to stage-0 runtime-supported value types."
                                        .to_owned(),
                                ),
                            ));
                            continue;
                        };
                        let slot = self.fresh_slot();
                        self.mutable_locals.push(MutableLocal {
                            slot,
                            name: binding.name.clone(),
                            ty: slot_ty,
                            mutable: true,
                        });
                        self.instructions
                            .push(Inst::StoreLocal { slot, src: value });
                        self.locals
                            .insert(binding.name.clone(), LocalBinding::Slot(slot));
                    } else {
                        self.locals
                            .insert(binding.name.clone(), LocalBinding::Value(value));
                    }
                    self.local_types.insert(binding.name.clone(), ty);
                }
                Stmt::Assign(statement) => {
                    let value = self.lower_expr(&statement.value);
                    match &statement.target {
                        Expr::Name(target) => match self.locals.get(&target.name).copied() {
                            Some(LocalBinding::Slot(slot)) => {
                                self.instructions
                                    .push(Inst::StoreLocal { slot, src: value });
                                self.local_types.insert(
                                    target.name.clone(),
                                    self.infer_expr_type(&statement.value),
                                );
                            }
                            Some(LocalBinding::Value(_)) | None => {
                                self.diagnostics.push(Diagnostic::new(
                                    "mir.assign-without-slot",
                                    format!(
                                        "mutable assignment to `{}` in `{}` is missing a lowered slot",
                                        target.name, self.function.name
                                    ),
                                    statement.span,
                                    Some(
                                        "Only `let mut` bindings may be assigned to in stage-0 MIR."
                                            .to_owned(),
                                    ),
                                ));
                            }
                        },
                        Expr::Index(target) => {
                            self.lower_array_index_assign_statement(target, value, statement.span);
                        }
                        _ => {
                            self.diagnostics.push(Diagnostic::new(
                                "mir.assign-target",
                                format!(
                                    "assignment target in `{}` must be a mutable local or mutable local array element",
                                    self.function.name
                                ),
                                statement.span,
                                Some("Use `name = value;` or `name[index] = value;`.".to_owned()),
                            ));
                        }
                    }
                }
                Stmt::Expr(stmt) => {
                    self.lower_expr(&stmt.expr);
                }
            }
        }
        body.tail.as_ref().map_or(
            BodyLowering {
                result: None,
                falls_through: true,
            },
            |tail| self.lower_tail_expr(tail),
        )
    }

    #[allow(clippy::too_many_lines)]
    fn lower_expr(&mut self, expr: &Expr) -> ValueId {
        match expr {
            Expr::Integer(expr) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstInt {
                    dest,
                    value: expr.value,
                });
                dest
            }
            Expr::Float(expr) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstF64 {
                    dest,
                    bits: expr.value.to_bits(),
                });
                dest
            }
            Expr::String(expr) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstText {
                    dest,
                    value: expr.value.clone(),
                });
                dest
            }
            Expr::Bool(expr) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstBool {
                    dest,
                    value: expr.value,
                });
                dest
            }
            Expr::Name(expr) => {
                if let Some(binding) = self.locals.get(&expr.name).copied() {
                    match binding {
                        LocalBinding::Value(value) => value,
                        LocalBinding::Slot(slot) => {
                            let dest = self.fresh_value();
                            self.instructions.push(Inst::LoadLocal { dest, slot });
                            dest
                        }
                    }
                } else if let Some(&value) = self.substitutions.get(&expr.name) {
                    let dest = self.fresh_value();
                    self.instructions.push(Inst::ConstInt {
                        dest,
                        value: value as i64,
                    });
                    dest
                } else if let Some(value) = self.evaluated_consts.get(&expr.name) {
                    self.emit_runtime_value(value)
                } else {
                    self.diagnostics.push(Diagnostic::new(
                        "mir.unknown-local",
                        format!(
                            "failed to lower `{}` because it is not bound in `{}`",
                            expr.name, self.function.name,
                        ),
                        expr.span,
                        Some("Stage-0 MIR lowering only admits previously bound parameter, `let`, and `const` names.".to_owned()),
                    ));
                    self.fresh_value()
                }
            }
            Expr::ContractResult(expr) => {
                self.contract_result.unwrap_or_else(|| {
                    self.diagnostics.push(Diagnostic::new(
                        "mir.contract-result",
                        format!(
                            "failed to lower `result` because no executable result is available in `{}`",
                            self.function.name,
                        ),
                        expr.span,
                        Some("Keep `result` inside executable `ensures` clauses only.".to_owned()),
                    ));
                    self.fresh_value()
                })
            }
            Expr::Group(expr) => self.lower_expr(&expr.inner),
            Expr::Unary(expr) => {
                let inner = self.lower_expr(&expr.inner);
                match expr.op {
                    sarif_frontend::hir::UnaryOp::Not => {
                        let false_value = self.emit_runtime_value(&RuntimeValue::Bool(false));
                        let dest = self.fresh_value();
                        self.instructions.push(Inst::Eq {
                            dest,
                            left: inner,
                            right: false_value,
                        });
                        dest
                    }
                }
            }
            Expr::Comptime(body) => {
                let mut evaluator = ConstEvaluator {
                    consts: &BTreeMap::new(),
                    functions: self.all_functions,
                    enum_variants: self.enum_variants,
                    struct_fields: self.struct_fields,
                    values: self.evaluated_consts.clone(),
                    active_consts: Vec::new(),
                    active_functions: Vec::new(),
                    next_slot: 0,
                    contract_result: None,
                    diagnostics: &mut Vec::new(),
                };
                let mut env = ConstEnv::default();
                // Comptime should probably not capture locals unless they are const.
                let result = evaluator
                    .eval_body(body, &mut env)
                    .map_or(RuntimeValue::Unit, |flow| match flow {
                        ConstFlow::Value(v) => v,
                    });
                self.emit_runtime_value(&result)
            }
            Expr::Handle(expr) => {
                let mut arms = Vec::new();
                for arm in &expr.arms {
                    let saved_locals = self.locals.clone();
                    let saved_local_types = self.local_types.clone();
                    let saved_instructions = std::mem::take(&mut self.instructions);

                    // Bind arm params
                    for param in &arm.params {
                        let id = self.fresh_value();
                        self.instructions.push(Inst::LoadParam {
                            dest: id,
                            index: 0, // This is a hack, we don't have a good way to represent arm params yet
                        });
                        self.locals.insert(param.clone(), LocalBinding::Value(id));
                    }

                    let arm_lower = self.lower_body(&arm.body, false);
                    let nested_instructions = std::mem::take(&mut self.instructions);

                    self.instructions = saved_instructions;
                    self.locals = saved_locals;
                    self.local_types = saved_local_types;

                    arms.push(HandleArm {
                        effect: String::new(), // HIR HandleArm has 'name' which we'll use as 'operation'
                        operation: arm.name.clone(),
                        params: arm.params.clone(),
                        body_insts: nested_instructions,
                        body_result: arm_lower.result,
                    });
                }
                let body_lower = self.lower_nested_body(&expr.body);
                let dest = self.fresh_value();
                self.instructions.push(Inst::Handle {
                    dest,
                    body_insts: body_lower.instructions,
                    body_result: body_lower.result,
                    arms,
                });
                dest
            }
            Expr::Perform(expr) => {
                let args = expr
                    .args
                    .iter()
                    .map(|arg| self.lower_expr(arg))
                    .collect::<Vec<_>>();
                let dest = self.fresh_value();
                self.instructions.push(Inst::Perform {
                    dest,
                    effect: expr.effect.clone(),
                    operation: expr.operation.clone(),
                    args,
                });
                dest
            }
            Expr::Call(expr) => {
                if expr.callee == "len" && !self.function_returns.contains_key("len") {
                    return self.lower_array_len_expr(expr);
                }
                if expr.callee == "text_len" && !self.function_returns.contains_key("text_len") {
                    return self.lower_text_len_expr(expr);
                }
                if expr.callee == "text_builder_new"
                    && !self.function_returns.contains_key("text_builder_new")
                {
                    return self.lower_text_builder_new_expr(expr);
                }
                if expr.callee == "text_builder_append"
                    && !self.function_returns.contains_key("text_builder_append")
                {
                    return self.lower_text_builder_append_expr(expr);
                }
                if expr.callee == "text_builder_append_codepoint"
                    && !self
                        .function_returns
                        .contains_key("text_builder_append_codepoint")
                {
                    return self.lower_text_builder_append_codepoint_expr(expr);
                }
                if expr.callee == "text_builder_finish"
                    && !self.function_returns.contains_key("text_builder_finish")
                {
                    return self.lower_text_builder_finish_expr(expr);
                }
                if expr.callee == "list_new"
                    && !self.function_returns.contains_key("list_new")
                {
                    return self.lower_list_new_expr(expr);
                }
                if expr.callee == "list_len"
                    && !self.function_returns.contains_key("list_len")
                {
                    return self.lower_list_len_expr(expr);
                }
                if expr.callee == "list_get"
                    && !self.function_returns.contains_key("list_get")
                {
                    return self.lower_list_get_expr(expr);
                }
                if expr.callee == "list_set"
                    && !self.function_returns.contains_key("list_set")
                {
                    return self.lower_list_set_expr(expr);
                }
                if expr.callee == "f64_from_i32"
                    && !self.function_returns.contains_key("f64_from_i32")
                {
                    return self.lower_f64_from_i32_expr(expr);
                }
                if expr.callee == "text_concat"
                    && !self.function_returns.contains_key("text_concat")
                {
                    return self.lower_text_concat_expr(expr);
                }
                if expr.callee == "text_slice" && !self.function_returns.contains_key("text_slice")
                {
                    return self.lower_text_slice_expr(expr);
                }
                if expr.callee == "text_byte" && !self.function_returns.contains_key("text_byte") {
                    return self.lower_text_byte_expr(expr);
                }
                if expr.callee == "text_cmp" && !self.function_returns.contains_key("text_cmp") {
                    return self.lower_text_cmp_expr(expr);
                }
                if expr.callee == "text_eq_range"
                    && !self.function_returns.contains_key("text_eq_range")
                {
                    return self.lower_text_eq_range_expr(expr);
                }
                if expr.callee == "text_find_byte_range"
                    && !self.function_returns.contains_key("text_find_byte_range")
                {
                    return self.lower_text_find_byte_range_expr(expr);
                }
                if expr.callee == "text_from_f64_fixed"
                    && !self.function_returns.contains_key("text_from_f64_fixed")
                {
                    return self.lower_text_from_f64_fixed_expr(expr);
                }
                if expr.callee == "sqrt" && !self.function_returns.contains_key("sqrt") {
                    return self.lower_sqrt_expr(expr);
                }
                if expr.callee == "parse_i32" && !self.function_returns.contains_key("parse_i32") {
                    return self.lower_parse_i32_expr(expr);
                }
                if expr.callee == "parse_i32_range"
                    && !self.function_returns.contains_key("parse_i32_range")
                {
                    return self.lower_parse_i32_range_expr(expr);
                }
                if expr.callee == "parse_f64" && !self.function_returns.contains_key("parse_f64") {
                    return self.lower_parse_f64_expr(expr);
                }
                if expr.callee == "arg_count" && !self.function_returns.contains_key("arg_count")
                {
                    return self.lower_arg_count_expr(expr);
                }
                if expr.callee == "alloc_push"
                    && !self.function_returns.contains_key("alloc_push")
                {
                    return self.lower_alloc_push_expr(expr);
                }
                if expr.callee == "alloc_pop"
                    && !self.function_returns.contains_key("alloc_pop")
                {
                    return self.lower_alloc_pop_expr(expr);
                }
                if expr.callee == "arg_text" && !self.function_returns.contains_key("arg_text") {
                    return self.lower_arg_text_expr(expr);
                }
                if expr.callee == "stdin_text"
                    && !self.function_returns.contains_key("stdin_text")
                {
                    return self.lower_stdin_text_expr(expr);
                }
                if expr.callee == "stdout_write"
                    && !self.function_returns.contains_key("stdout_write")
                {
                    return self.lower_stdout_write_expr(expr);
                }
                if let Some((enum_name, variant_name, payload_type)) =
                    self.enum_constructor_for_call(&expr.callee)
                {
                    let dest = self.fresh_value();
                    let payload = payload_type.as_ref().map(|_| self.lower_expr(&expr.args[0]));
                    self.instructions.push(Inst::MakeEnum {
                        dest,
                        name: enum_name,
                        variant: variant_name,
                        payload,
                    });
                    return dest;
                }

                let callee_name = if self.generic_functions.contains_key(&expr.callee) {
                    self.monomorphize(&expr.callee, &expr.args)
                } else {
                    expr.callee.clone()
                };

                let dest = self.fresh_value();
                let args = expr.args.iter().map(|arg| self.lower_expr(arg)).collect();
                self.instructions.push(Inst::Call {
                    dest,
                    callee: callee_name,
                    args,
                });
                dest
            }
            Expr::Array(expr) => self.lower_array_expr(expr),
            Expr::Field(expr) => {
                if let Some((enum_name, variant_name)) =
                    self.payload_free_enum_variant_for_field(&expr.base, &expr.field)
                {
                    let dest = self.fresh_value();
                    self.instructions.push(Inst::MakeEnum {
                        dest,
                        name: enum_name,
                        variant: variant_name,
                        payload: None,
                    });
                    return dest;
                }
                let base = self.lower_expr(&expr.base);
                let dest = self.fresh_value();
                self.instructions.push(Inst::Field {
                    dest,
                    base,
                    name: expr.field.clone(),
                });
                dest
            }
            Expr::Index(expr) => self.lower_index_expr(expr),
            Expr::If(expr) => {
                let condition = self.lower_expr(&expr.condition);
                let then_body = self.lower_nested_body(&expr.then_body);
                let else_body = self.lower_nested_body(&expr.else_body);
                let dest = self.fresh_value();
                self.instructions.push(Inst::If {
                    dest,
                    condition,
                    then_insts: then_body.instructions,
                    then_result: then_body.result,
                    else_insts: else_body.instructions,
                    else_result: else_body.result,
                });
                dest
            }
            Expr::Match(expr) => {
                let scrutinee = self.lower_expr(&expr.scrutinee);
                self.lower_match_value(scrutinee, &expr.arms)
            }
            Expr::While(expr) => {
                let condition = self.lower_nested_expr(&expr.condition);
                let body = self.lower_nested_body(&expr.body);
                let dest = self.fresh_value();
                self.instructions.push(Inst::While {
                    dest,
                    condition_insts: condition.instructions,
                    condition: condition.value,
                    body_insts: body.instructions,
                });
                dest
            }
            Expr::Repeat(expr) => {
                let count = self.lower_expr(&expr.count);
                let (index_slot, body) = self.lower_repeat_body(expr);
                let dest = self.fresh_value();
                self.instructions.push(Inst::Repeat {
                    dest,
                    count,
                    index_slot,
                    body_insts: body.instructions,
                });
                dest
            }
            Expr::Record(expr) => {
                let dest = self.fresh_value();
                let fields = if let Some(declared_fields) = self.struct_fields.get(&expr.name) {
                    declared_fields
                        .iter()
                        .filter_map(|declared| {
                            expr.fields
                                .iter()
                                .find(|field| &field.name == declared)
                                .map(|field| (declared.clone(), self.lower_expr(&field.value)))
                        })
                        .collect::<Vec<_>>()
                } else {
                    expr.fields
                        .iter()
                        .map(|field| (field.name.clone(), self.lower_expr(&field.value)))
                        .collect::<Vec<_>>()
                };
                self.instructions.push(Inst::MakeRecord {
                    dest,
                    name: expr.name.clone(),
                    fields,
                });
                dest
            }
            Expr::Binary(expr) => {
                let left = self.lower_expr(&expr.left);
                let right = self.lower_expr(&expr.right);
                let dest = self.fresh_value();
                let inst = match expr.op {
                    BinaryOp::Add => Inst::Add { dest, left, right },
                    BinaryOp::Sub => Inst::Sub { dest, left, right },
                    BinaryOp::Mul => Inst::Mul { dest, left, right },
                    BinaryOp::Div => Inst::Div { dest, left, right },
                    BinaryOp::And => Inst::And { dest, left, right },
                    BinaryOp::Or => Inst::Or { dest, left, right },
                    BinaryOp::Eq => Inst::Eq { dest, left, right },
                    BinaryOp::Ne => Inst::Ne { dest, left, right },
                    BinaryOp::Lt => Inst::Lt { dest, left, right },
                    BinaryOp::Le => Inst::Le { dest, left, right },
                    BinaryOp::Gt => Inst::Gt { dest, left, right },
                    BinaryOp::Ge => Inst::Ge { dest, left, right },
                };
                self.instructions.push(inst);
                dest
            }
        }
    }

    fn lower_tail_expr(&mut self, expr: &Expr) -> BodyLowering {
        match expr {
            Expr::Group(expr) => self.lower_tail_expr(&expr.inner),
            Expr::Unary(_) => BodyLowering {
                result: Some(self.lower_expr(expr)),
                falls_through: true,
            },
            Expr::Handle(expr) => {
                let dest = self.lower_expr(&Expr::Handle(expr.clone()));
                BodyLowering {
                    result: Some(dest),
                    falls_through: true,
                }
            }
            Expr::Perform(expr) => {
                let dest = self.lower_expr(&Expr::Perform(expr.clone()));
                BodyLowering {
                    result: Some(dest),
                    falls_through: true,
                }
            }
            Expr::If(expr) => {
                let condition = self.lower_expr(&expr.condition);
                let then_body = self.lower_nested_body(&expr.then_body);
                let else_body = self.lower_nested_body(&expr.else_body);
                let dest = self.fresh_value();
                self.instructions.push(Inst::If {
                    dest,
                    condition,
                    then_insts: then_body.instructions,
                    then_result: then_body.result,
                    else_insts: else_body.instructions,
                    else_result: else_body.result,
                });
                BodyLowering {
                    result: then_body.result.or(else_body.result).map(|_| dest),
                    falls_through: then_body.falls_through || else_body.falls_through,
                }
            }
            Expr::Match(expr) => {
                let scrutinee = self.lower_expr(&expr.scrutinee);
                self.lower_match_body(scrutinee, &expr.arms)
            }
            Expr::While(expr) => {
                let condition = self.lower_nested_expr(&expr.condition);
                let body = self.lower_nested_body(&expr.body);
                let dest = self.fresh_value();
                self.instructions.push(Inst::While {
                    dest,
                    condition_insts: condition.instructions,
                    condition: condition.value,
                    body_insts: body.instructions,
                });
                BodyLowering {
                    result: None,
                    falls_through: true,
                }
            }
            Expr::Repeat(expr) => {
                let count = self.lower_expr(&expr.count);
                let (index_slot, body) = self.lower_repeat_body(expr);
                let dest = self.fresh_value();
                self.instructions.push(Inst::Repeat {
                    dest,
                    count,
                    index_slot,
                    body_insts: body.instructions,
                });
                BodyLowering {
                    result: None,
                    falls_through: true,
                }
            }
            _ => {
                let value = self.lower_expr(expr);
                BodyLowering {
                    result: (!expr_returns_unit(expr)).then_some(value),
                    falls_through: true,
                }
            }
        }
    }

    fn lower_match_value(
        &mut self,
        scrutinee: ValueId,
        arms: &[sarif_frontend::hir::MatchArm],
    ) -> ValueId {
        let body = self.lower_match_body(scrutinee, arms);
        body.result.unwrap_or_else(|| {
            let dest = self.fresh_value();
            let condition = self.const_true();
            self.instructions.push(Inst::If {
                dest,
                condition,
                then_insts: Vec::new(),
                then_result: None,
                else_insts: Vec::new(),
                else_result: None,
            });
            dest
        })
    }

    fn lower_match_body(
        &mut self,
        scrutinee: ValueId,
        arms: &[sarif_frontend::hir::MatchArm],
    ) -> BodyLowering {
        if arms.is_empty() {
            return BodyLowering {
                result: None,
                falls_through: true,
            };
        }

        let arm = &arms[0];
        let then_body = self.lower_match_arm_body(scrutinee, arm);
        let Some(condition) = self.lower_match_pattern_condition(scrutinee, &arm.pattern) else {
            self.instructions.extend(then_body.instructions);
            return BodyLowering {
                result: then_body.result,
                falls_through: then_body.falls_through,
            };
        };
        let else_body = if arms.len() == 1 {
            self.lower_match_arm_body(scrutinee, arm)
        } else {
            self.lower_match_nested(scrutinee, &arms[1..])
        };
        let dest = self.fresh_value();
        self.instructions.push(Inst::If {
            dest,
            condition,
            then_insts: then_body.instructions,
            then_result: then_body.result,
            else_insts: else_body.instructions,
            else_result: else_body.result,
        });
        BodyLowering {
            result: then_body.result.or(else_body.result).map(|_| dest),
            falls_through: then_body.falls_through || else_body.falls_through,
        }
    }

    fn lower_match_nested(
        &mut self,
        scrutinee: ValueId,
        arms: &[sarif_frontend::hir::MatchArm],
    ) -> NestedBodyLowering {
        let saved_instructions = std::mem::take(&mut self.instructions);
        let body = self.lower_match_body(scrutinee, arms);
        let nested_instructions = std::mem::take(&mut self.instructions);
        self.instructions = saved_instructions;
        NestedBodyLowering {
            instructions: nested_instructions,
            result: body.result,
            falls_through: body.falls_through,
        }
    }

    fn lower_enum_tag_check(&mut self, scrutinee: ValueId, tag: i64) -> ValueId {
        let condition = self.fresh_value();
        self.instructions.push(Inst::EnumTagEq {
            dest: condition,
            value: scrutinee,
            tag,
        });
        condition
    }

    fn lower_match_arm_body(
        &mut self,
        scrutinee: ValueId,
        arm: &sarif_frontend::hir::MatchArm,
    ) -> NestedBodyLowering {
        let saved_instructions = std::mem::take(&mut self.instructions);
        let saved_locals = self.locals.clone();
        if let sarif_frontend::hir::MatchPattern::Variant {
            binding: Some(binding),
            ..
        } = &arm.pattern
        {
            let payload = self.fresh_value();
            let payload_type = self.payload_type_for_pattern(&arm.pattern);
            self.instructions.push(Inst::EnumPayload {
                dest: payload,
                value: scrutinee,
                payload_type,
            });
            self.locals
                .insert(binding.clone(), LocalBinding::Value(payload));
        }
        let body = self.lower_nested_body(&arm.body);
        self.locals = saved_locals;
        let mut nested_instructions = std::mem::take(&mut self.instructions);
        nested_instructions.extend(body.instructions);
        self.instructions = saved_instructions;
        NestedBodyLowering {
            instructions: nested_instructions,
            result: body.result,
            falls_through: body.falls_through,
        }
    }

    fn lower_match_pattern_condition(
        &mut self,
        scrutinee: ValueId,
        pattern: &sarif_frontend::hir::MatchPattern,
    ) -> Option<ValueId> {
        match pattern {
            sarif_frontend::hir::MatchPattern::Variant { path, span, .. } => {
                let tag = self.enum_variant_tag_from_pattern(path.path.as_str(), *span);
                Some(self.lower_enum_tag_check(scrutinee, tag))
            }
            sarif_frontend::hir::MatchPattern::Integer { value, .. } => {
                let right = self.fresh_value();
                self.instructions.push(Inst::ConstInt {
                    dest: right,
                    value: *value,
                });
                Some(self.lower_value_eq(scrutinee, right))
            }
            sarif_frontend::hir::MatchPattern::String { value, .. } => {
                let right = self.fresh_value();
                self.instructions.push(Inst::ConstText {
                    dest: right,
                    value: value.clone(),
                });
                Some(self.lower_value_eq(scrutinee, right))
            }
            sarif_frontend::hir::MatchPattern::Bool { value, .. } => {
                let right = self.fresh_value();
                self.instructions.push(Inst::ConstBool {
                    dest: right,
                    value: *value,
                });
                Some(self.lower_value_eq(scrutinee, right))
            }
            sarif_frontend::hir::MatchPattern::Wildcard { .. } => None,
        }
    }

    fn lower_value_eq(&mut self, left: ValueId, right: ValueId) -> ValueId {
        let dest = self.fresh_value();
        self.instructions.push(Inst::Eq { dest, left, right });
        dest
    }

    fn payload_free_enum_variant_for_field(
        &self,
        base: &Expr,
        field: &str,
    ) -> Option<(String, String)> {
        let Expr::Name(base_name) = base else {
            return None;
        };
        self.enum_variants
            .get(&base_name.name)?
            .iter()
            .find(|variant| variant.name == field && variant.payload_type.is_none())
            .map(|_| (base_name.name.clone(), field.to_owned()))
    }

    fn payload_type_for_pattern(&mut self, pattern: &sarif_frontend::hir::MatchPattern) -> String {
        let sarif_frontend::hir::MatchPattern::Variant { path, span, .. } = pattern else {
            self.diagnostics.push(Diagnostic::new(
                "mir.enum-payload",
                "failed to lower payload binding for a non-enum pattern",
                self.function.span,
                Some("Use a declared payload-carrying enum variant.".to_owned()),
            ));
            return "Unit".to_owned();
        };
        let Some((enum_name, variant_name)) = split_enum_variant_path(path.path.as_str()) else {
            self.diagnostics.push(Diagnostic::new(
                "mir.enum-payload",
                "failed to lower payload binding for an invalid enum pattern",
                *span,
                Some("Use a declared enum variant with a payload binding.".to_owned()),
            ));
            return "Unit".to_owned();
        };

        self.enum_variants
            .get(enum_name)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|variant| variant.name == variant_name)
                    .and_then(|variant| variant.payload_type.clone())
            })
            .unwrap_or_else(|| {
                self.diagnostics.push(Diagnostic::new(
                    "mir.enum-payload",
                    format!(
                        "failed to lower payload binding for unknown or payload-free enum variant `{}`",
                        path.path
                    ),
                    *span,
                    Some("Use a declared payload-carrying enum variant.".to_owned()),
                ));
                "Unit".to_owned()
            })
    }

    fn enum_variant_tag_from_pattern(&mut self, path: &str, span: Span) -> i64 {
        let Some((enum_name, variant_name)) = split_enum_variant_path(path) else {
            self.diagnostics.push(Diagnostic::new(
                "mir.match-pattern",
                format!("failed to lower match arm `{path}`"),
                span,
                Some("Use `Enum.variant` arm patterns during MIR lowering.".to_owned()),
            ));
            return 0;
        };
        self.enum_variants
            .get(enum_name)
            .and_then(|variants| {
                variants
                    .iter()
                    .position(|variant| variant.name == variant_name)
                    .map(|index| i64::try_from(index).expect("enum variant index fits in i64"))
            })
            .unwrap_or_else(|| {
                self.diagnostics.push(Diagnostic::new(
                    "mir.match-pattern",
                    format!("failed to lower unknown match arm `{path}`"),
                    span,
                    Some("Use one of the declared enum variants.".to_owned()),
                ));
                0
            })
    }

    fn enum_constructor_for_call(&self, callee: &str) -> Option<(String, String, Option<String>)> {
        let (enum_name, variant_name) = split_enum_variant_path(callee)?;
        let variant = self
            .enum_variants
            .get(enum_name)?
            .iter()
            .find(|variant| variant.name == variant_name)?;
        Some((
            enum_name.to_owned(),
            variant_name.to_owned(),
            variant.payload_type.clone(),
        ))
    }

    fn infer_expr_type(&self, expr: &Expr) -> LowerType {
        match expr {
            Expr::Integer(_) => LowerType::I32,
            Expr::Float(_) => LowerType::F64,
            Expr::String(_) => LowerType::Text,
            Expr::Bool(_) => LowerType::Bool,
            Expr::Name(expr) => self
                .local_types
                .get(&expr.name)
                .cloned()
                .or_else(|| {
                    self.evaluated_consts
                        .get(&expr.name)
                        .map(runtime_value_lower_type)
                })
                .unwrap_or(LowerType::Error),
            Expr::ContractResult(_) => self
                .function
                .return_type
                .as_ref()
                .map_or(LowerType::Unit, |ty| {
                    LowerType::from_type_name(&ty.path, &self.substitutions)
                }),
            Expr::Call(expr) => match expr.callee.as_str() {
                "len" if !self.function_returns.contains_key("len") => {
                    match expr.args.first().map(|arg| self.infer_expr_type(arg)) {
                        Some(LowerType::Array(_, _)) => LowerType::I32,
                        _ => LowerType::Error,
                    }
                }
                "text_len" if !self.function_returns.contains_key("text_len") => LowerType::I32,
                "text_byte" if !self.function_returns.contains_key("text_byte") => LowerType::I32,
                "text_cmp" if !self.function_returns.contains_key("text_cmp") => LowerType::I32,
                "text_eq_range" if !self.function_returns.contains_key("text_eq_range") => {
                    LowerType::Bool
                }
                "text_find_byte_range"
                    if !self.function_returns.contains_key("text_find_byte_range") =>
                {
                    LowerType::I32
                }
                "text_builder_new" if !self.function_returns.contains_key("text_builder_new") => {
                    LowerType::TextBuilder
                }
                "text_builder_append"
                    if !self.function_returns.contains_key("text_builder_append") =>
                {
                    LowerType::TextBuilder
                }
                "text_builder_append_codepoint"
                    if !self
                        .function_returns
                        .contains_key("text_builder_append_codepoint") =>
                {
                    LowerType::TextBuilder
                }
                "text_builder_finish"
                    if !self.function_returns.contains_key("text_builder_finish") =>
                {
                    LowerType::Text
                }
                "list_new" if !self.function_returns.contains_key("list_new") => {
                    match expr.args.get(1).map(|arg| self.infer_expr_type(arg)) {
                        Some(element) => LowerType::List(Box::new(element)),
                        None => LowerType::Error,
                    }
                }
                "list_len" if !self.function_returns.contains_key("list_len") => LowerType::I32,
                "list_get" if !self.function_returns.contains_key("list_get") => {
                    match expr.args.first().map(|arg| self.infer_expr_type(arg)) {
                        Some(LowerType::List(element)) => *element,
                        _ => LowerType::Error,
                    }
                }
                "list_set" if !self.function_returns.contains_key("list_set") => {
                    match expr.args.first().map(|arg| self.infer_expr_type(arg)) {
                        Some(LowerType::List(element)) => LowerType::List(element),
                        _ => LowerType::Error,
                    }
                }
                "f64_from_i32" if !self.function_returns.contains_key("f64_from_i32") => {
                    LowerType::F64
                }
                "text_concat" if !self.function_returns.contains_key("text_concat") => {
                    LowerType::Text
                }
                "text_slice" if !self.function_returns.contains_key("text_slice") => {
                    LowerType::Text
                }
                "text_from_f64_fixed"
                    if !self.function_returns.contains_key("text_from_f64_fixed") =>
                {
                    LowerType::Text
                }
                "alloc_push" if !self.function_returns.contains_key("alloc_push") => {
                    LowerType::Unit
                }
                "alloc_pop" if !self.function_returns.contains_key("alloc_pop") => {
                    LowerType::Unit
                }
                "stdin_text" if !self.function_returns.contains_key("stdin_text") => {
                    LowerType::Text
                }
                "sqrt" if !self.function_returns.contains_key("sqrt") => LowerType::F64,
                "parse_i32" if !self.function_returns.contains_key("parse_i32") => LowerType::I32,
                "parse_i32_range" if !self.function_returns.contains_key("parse_i32_range") => {
                    LowerType::I32
                }
                "parse_f64" if !self.function_returns.contains_key("parse_f64") => LowerType::F64,
                _ => {
                    if let Some((enum_name, _, _)) = self.enum_constructor_for_call(&expr.callee) {
                        LowerType::Named(enum_name)
                    } else {
                        self.function_returns
                            .get(&expr.callee)
                            .map_or(LowerType::Error, |ty| {
                                LowerType::from_type_name(ty, &self.substitutions)
                            })
                    }
                }
            },
            Expr::Array(expr) => expr.elements.first().map_or(LowerType::Error, |first| {
                LowerType::Array(Box::new(self.infer_expr_type(first)), expr.elements.len())
            }),
            Expr::Field(expr) => {
                if let Some((enum_name, _)) =
                    self.payload_free_enum_variant_for_field(&expr.base, &expr.field)
                {
                    LowerType::Named(enum_name)
                } else {
                    match self.infer_expr_type(&expr.base) {
                        LowerType::Named(name) => self
                            .struct_layouts
                            .get(&name)
                            .and_then(|fields| {
                                fields
                                    .iter()
                                    .find(|(field_name, _)| field_name == &expr.field)
                                    .map(|(_, field_ty)| {
                                        LowerType::from_type_name(field_ty, &self.substitutions)
                                    })
                            })
                            .unwrap_or(LowerType::Error),
                        _ => LowerType::Error,
                    }
                }
            }
            Expr::Index(expr) => match self.infer_expr_type(&expr.base) {
                LowerType::Array(element, _) => *element,
                _ => LowerType::Error,
            },
            Expr::If(expr) => self.infer_body_type(&expr.then_body),
            Expr::Match(expr) => expr
                .arms
                .first()
                .map_or(LowerType::Unit, |arm| self.infer_body_type(&arm.body)),
            Expr::Repeat(_) | Expr::While(_) => LowerType::Unit,
            Expr::Record(expr) => LowerType::Named(expr.name.clone()),
            Expr::Unary(expr) => match expr.op {
                sarif_frontend::hir::UnaryOp::Not => LowerType::Bool,
            },
            Expr::Binary(expr) => match expr.op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => match (
                    self.infer_expr_type(&expr.left),
                    self.infer_expr_type(&expr.right),
                ) {
                    (LowerType::F64, LowerType::F64) => LowerType::F64,
                    (LowerType::I32, LowerType::I32) => LowerType::I32,
                    _ => LowerType::Error,
                },
                BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge => LowerType::Bool,
            },
            Expr::Group(expr) => self.infer_expr_type(&expr.inner),
            Expr::Comptime(body) => self.infer_body_type(body),
            Expr::Handle(_) | Expr::Perform(_) => LowerType::Error,
        }
    }

    fn infer_body_type(&self, body: &sarif_frontend::hir::Body) -> LowerType {
        body.tail
            .as_ref()
            .map_or(LowerType::Unit, |tail| self.infer_expr_type(tail))
    }

    fn lower_array_expr(&mut self, expr: &sarif_frontend::hir::ArrayExpr) -> ValueId {
        let Some(first) = expr.elements.first() else {
            self.diagnostics.push(Diagnostic::new(
                "mir.array-empty",
                format!(
                    "failed to lower empty array literal in `{}`",
                    self.function.name
                ),
                expr.span,
                Some("Keep stage-0 array literals non-empty.".to_owned()),
            ));
            return self.emit_unit_value();
        };
        let element_ty = self.infer_expr_type(first);
        let array_ty = LowerType::Array(Box::new(element_ty), expr.elements.len());
        let Some(struct_name) = self.register_type_name(&array_ty) else {
            self.diagnostics.push(Diagnostic::new(
                "mir.array-type",
                format!(
                    "failed to lower array literal in `{}` because its element type is unsupported",
                    self.function.name
                ),
                expr.span,
                Some("Keep stage-0 arrays on scalar or named plain-value elements.".to_owned()),
            ));
            return self.emit_unit_value();
        };
        let fields = expr
            .elements
            .iter()
            .enumerate()
            .map(|(index, element)| (array_field_name(index), self.lower_expr(element)))
            .collect::<Vec<_>>();
        let dest = self.fresh_value();
        self.instructions.push(Inst::MakeRecord {
            dest,
            name: struct_name,
            fields,
        });
        dest
    }

    fn lower_array_len_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        if expr.args.len() != 1 {
            self.diagnostics.push(Diagnostic::new(
                "mir.array-len-arity",
                format!(
                    "failed to lower builtin `len` in `{}` because it expects exactly one argument",
                    self.function.name,
                ),
                expr.span,
                Some("Call `len(xs)` with exactly one array argument.".to_owned()),
            ));
            return self.emit_unit_value();
        }
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        if let LowerType::Array(_, len) = self.infer_expr_type(arg) {
            let Ok(value) = i64::try_from(len) else {
                self.diagnostics.push(Diagnostic::new(
                    "mir.array-len-range",
                    format!(
                        "failed to lower builtin `len` in `{}` because the array length exceeds stage-0 integer limits",
                        self.function.name,
                    ),
                    expr.span,
                    Some("Keep stage-0 array lengths within `I32`/`I64`-sized limits.".to_owned()),
                ));
                return self.emit_unit_value();
            };
            let dest = self.fresh_value();
            self.instructions.push(Inst::ConstInt { dest, value });
            dest
        } else {
            self.diagnostics.push(Diagnostic::new(
                "mir.array-len-type",
                format!(
                    "failed to lower builtin `len` in `{}` because its argument is not an array",
                    self.function.name,
                ),
                expr.span,
                Some("Pass an internal stage-0 array to `len`.".to_owned()),
            ));
            self.emit_unit_value()
        }
    }

    fn lower_index_expr(&mut self, expr: &sarif_frontend::hir::IndexExpr) -> ValueId {
        let array_ty = self.infer_expr_type(&expr.base);
        let LowerType::Array(_, len) = array_ty else {
            self.diagnostics.push(Diagnostic::new(
                "mir.array-index-base",
                format!(
                    "failed to lower array index in `{}` because the base is not an array",
                    self.function.name
                ),
                expr.base.span(),
                Some("Index into a stage-0 array-valued local or literal.".to_owned()),
            ));
            return self.emit_unit_value();
        };
        let base = self.lower_expr(&expr.base);
        let index = self.lower_expr(&expr.index);
        self.emit_bounds_assert(index, len);
        self.lower_index_choice(base, index, 0, len)
    }

    fn lower_array_index_assign_statement(
        &mut self,
        target: &sarif_frontend::hir::IndexExpr,
        value: ValueId,
        span: Span,
    ) {
        let Expr::Name(base) = target.base.as_ref() else {
            self.diagnostics.push(Diagnostic::new(
                "mir.assign-target",
                format!(
                    "indexed assignment in `{}` must target a mutable local array",
                    self.function.name
                ),
                span,
                Some("Use `name[index] = value;` on a mutable local array.".to_owned()),
            ));
            return;
        };
        let Some(LocalBinding::Slot(slot)) = self.locals.get(&base.name).copied() else {
            self.diagnostics.push(Diagnostic::new(
                "mir.assign-without-slot",
                format!(
                    "mutable assignment to `{}` in `{}` is missing a lowered slot",
                    base.name, self.function.name
                ),
                span,
                Some("Only `let mut` bindings may be assigned to in stage-0 MIR.".to_owned()),
            ));
            return;
        };
        let Some(array_ty) = self.local_types.get(&base.name).cloned() else {
            self.diagnostics.push(Diagnostic::new(
                "mir.assign-target",
                format!(
                    "mutable assignment target `{}` in `{}` has no lowered type",
                    base.name, self.function.name
                ),
                span,
                Some("Keep indexed assignment on mutable local arrays.".to_owned()),
            ));
            return;
        };
        let LowerType::Array(_, len) = array_ty.clone() else {
            self.diagnostics.push(Diagnostic::new(
                "mir.assign-index-base",
                format!(
                    "indexed assignment target `{}` in `{}` is not an array",
                    base.name, self.function.name
                ),
                target.base.span(),
                Some("Use `name[index] = value;` only on mutable local arrays.".to_owned()),
            ));
            return;
        };
        let Some(struct_name) = self.register_type_name(&array_ty) else {
            self.diagnostics.push(Diagnostic::new(
                "mir.assign-index-base",
                format!(
                    "indexed assignment target `{}` in `{}` uses a stage-0 unsupported array type",
                    base.name, self.function.name
                ),
                target.base.span(),
                Some(
                    "Keep indexed assignment on stage-0 supported array element types.".to_owned(),
                ),
            ));
            return;
        };

        let current_array = self.fresh_value();
        self.instructions.push(Inst::LoadLocal {
            dest: current_array,
            slot,
        });
        let index = self.lower_expr(&target.index);
        self.emit_bounds_assert(index, len);

        let mut fields = Vec::with_capacity(len);
        for offset in 0..len {
            let current_field = self.fresh_value();
            self.instructions.push(Inst::Field {
                dest: current_field,
                base: current_array,
                name: array_field_name(offset),
            });

            let field_index = self.fresh_value();
            self.instructions.push(Inst::ConstInt {
                dest: field_index,
                value: i64::try_from(offset).expect("array field offset fits in i64"),
            });
            let matches = self.fresh_value();
            self.instructions.push(Inst::Eq {
                dest: matches,
                left: index,
                right: field_index,
            });
            let selected = self.fresh_value();
            self.instructions.push(Inst::If {
                dest: selected,
                condition: matches,
                then_insts: Vec::new(),
                then_result: Some(value),
                else_insts: Vec::new(),
                else_result: Some(current_field),
            });
            fields.push((array_field_name(offset), selected));
        }

        let updated = self.fresh_value();
        self.instructions.push(Inst::MakeRecord {
            dest: updated,
            name: struct_name,
            fields,
        });
        self.instructions
            .push(Inst::StoreLocal { slot, src: updated });
    }

    fn emit_bounds_assert(&mut self, index: ValueId, len: usize) {
        let zero = self.fresh_value();
        self.instructions.push(Inst::ConstInt {
            dest: zero,
            value: 0,
        });
        let non_negative = self.fresh_value();
        self.instructions.push(Inst::Ge {
            dest: non_negative,
            left: index,
            right: zero,
        });
        let upper = self.fresh_value();
        self.instructions.push(Inst::ConstInt {
            dest: upper,
            value: i64::try_from(len).expect("array length fits in i64"),
        });
        let below_len = self.fresh_value();
        self.instructions.push(Inst::Lt {
            dest: below_len,
            left: index,
            right: upper,
        });
        let in_bounds = self.fresh_value();
        self.instructions.push(Inst::And {
            dest: in_bounds,
            left: non_negative,
            right: below_len,
        });
        self.instructions.push(Inst::Assert {
            condition: in_bounds,
            kind: ContractKind::Bounds,
        });
    }

    fn lower_index_choice(
        &mut self,
        base: ValueId,
        index: ValueId,
        offset: usize,
        len: usize,
    ) -> ValueId {
        if offset + 1 == len {
            let dest = self.fresh_value();
            self.instructions.push(Inst::Field {
                dest,
                base,
                name: array_field_name(offset),
            });
            return dest;
        }

        let expected = self.fresh_value();
        self.instructions.push(Inst::ConstInt {
            dest: expected,
            value: i64::try_from(offset).expect("array index fits in i64"),
        });
        let condition = self.fresh_value();
        self.instructions.push(Inst::Eq {
            dest: condition,
            left: index,
            right: expected,
        });
        let (then_insts, then_result) = self.capture_nested_value(|this| {
            let dest = this.fresh_value();
            this.instructions.push(Inst::Field {
                dest,
                base,
                name: array_field_name(offset),
            });
            dest
        });
        let (else_insts, else_result) =
            self.capture_nested_value(|this| this.lower_index_choice(base, index, offset + 1, len));
        let dest = self.fresh_value();
        self.instructions.push(Inst::If {
            dest,
            condition,
            then_insts,
            then_result: Some(then_result),
            else_insts,
            else_result: Some(else_result),
        });
        dest
    }

    fn capture_nested_value(
        &mut self,
        lower: impl FnOnce(&mut Self) -> ValueId,
    ) -> (Vec<Inst>, ValueId) {
        let saved_instructions = std::mem::take(&mut self.instructions);
        let result = lower(self);
        let nested_instructions = std::mem::take(&mut self.instructions);
        self.instructions = saved_instructions;
        (nested_instructions, result)
    }

    fn register_type_name(&mut self, ty: &LowerType) -> Option<String> {
        match ty {
            LowerType::Array(element, len) => {
                let element_ty = self.register_type_name(element)?;
                let name = array_struct_name(&element_ty, *len);
                self.generated_arrays
                    .entry(name.clone())
                    .or_insert_with(|| GeneratedArrayType {
                        name: name.clone(),
                        element_ty,
                        len: *len,
                    });
                Some(name)
            }
            _ => lower_type_name(ty),
        }
    }

    fn const_true(&mut self) -> ValueId {
        let left = self.fresh_value();
        self.instructions.push(Inst::ConstBool {
            dest: left,
            value: true,
        });
        left
    }

    fn monomorphize(&mut self, callee: &str, args: &[sarif_frontend::hir::Expr]) -> String {
        let template = self.generic_functions.get(callee).cloned().unwrap();
        let mut substitutions = self.substitutions.clone();
        for (param, arg) in template.params.iter().zip(args) {
            self.unify_into(
                &param.ty.path,
                &self.infer_expr_type(arg),
                &mut substitutions,
            );
        }

        let mut sorted_subs: Vec<_> = substitutions
            .iter()
            .filter(|(k, _)| template.type_params.iter().any(|p| &p.name == *k))
            .collect();
        sorted_subs.sort_by_key(|(k, _)| *k);
        let suffix = sorted_subs
            .iter()
            .map(|(k, v)| format!("{k}_{v}"))
            .collect::<Vec<_>>()
            .join("_");
        let new_name = if suffix.is_empty() {
            callee.to_owned()
        } else {
            format!("{callee}_{suffix}")
        };

        let already_lowered = self.function_returns.contains_key(&new_name)
            || self
                .monomorphized_functions
                .iter()
                .any(|f| f.name == new_name);

        if !already_lowered {
            let mut shared = LowerShared {
                enum_variants: self.enum_variants,
                struct_fields: self.struct_fields,
                struct_layouts: self.struct_layouts,
                function_returns: self.function_returns,
                evaluated_consts: self.evaluated_consts,
                generated_arrays: self.generated_arrays,
                generic_functions: self.generic_functions,
                all_functions: self.all_functions,
                monomorphized_functions: Vec::new(),
            };
            let mut diagnostics = Vec::new();
            let monomorphized = lower_function_monomorphized(
                template,
                &mut shared,
                &mut diagnostics,
                substitutions,
                Some(new_name.clone()),
            );
            self.monomorphized_functions.push(monomorphized);
            self.monomorphized_functions
                .extend(shared.monomorphized_functions);
        }

        new_name
    }

    fn unify_into(
        &self,
        expected_path: &str,
        actual_ty: &LowerType,
        substitutions: &mut HashMap<String, usize>,
    ) {
        if expected_path.starts_with('[') && expected_path.ends_with(']') {
            let inner = &expected_path[1..expected_path.len() - 1];
            if let Some(split) = inner.rfind(';') {
                let element_path = inner[..split].trim();
                let len_str = inner[split + 1..].trim();

                if let LowerType::Array(actual_element, actual_len) = actual_ty {
                    self.unify_into(element_path, actual_element, substitutions);

                    if !len_str.chars().all(|c| c.is_ascii_digit()) {
                        substitutions.insert(len_str.to_owned(), *actual_len);
                    }
                }
            }
        }
    }

    const fn fresh_value(&mut self) -> ValueId {
        let value = ValueId(self.next_value);
        self.next_value += 1;
        value
    }

    const fn fresh_slot(&mut self) -> LocalSlotId {
        let slot = LocalSlotId(self.next_slot);
        self.next_slot += 1;
        slot
    }

    fn lower_nested_body(&mut self, body: &sarif_frontend::hir::Body) -> NestedBodyLowering {
        let saved_instructions = std::mem::take(&mut self.instructions);
        let saved_locals = self.locals.clone();
        let saved_local_types = self.local_types.clone();
        let body = self.lower_body(body, false);
        let nested_instructions = std::mem::take(&mut self.instructions);
        self.instructions = saved_instructions;
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        NestedBodyLowering {
            instructions: nested_instructions,
            result: body.result,
            falls_through: body.falls_through,
        }
    }

    fn lower_nested_expr(&mut self, expr: &sarif_frontend::hir::Expr) -> NestedExprLowering {
        let saved_instructions = std::mem::take(&mut self.instructions);
        let saved_locals = self.locals.clone();
        let saved_local_types = self.local_types.clone();
        let value = self.lower_expr(expr);
        let nested_instructions = std::mem::take(&mut self.instructions);
        self.instructions = saved_instructions;
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        NestedExprLowering {
            instructions: nested_instructions,
            value,
        }
    }

    fn lower_repeat_body(
        &mut self,
        expr: &sarif_frontend::hir::RepeatExpr,
    ) -> (Option<LocalSlotId>, NestedBodyLowering) {
        let saved_instructions = std::mem::take(&mut self.instructions);
        let saved_locals = self.locals.clone();
        let saved_local_types = self.local_types.clone();
        let index_slot = expr.binding.as_ref().map(|binding| {
            let slot = self.fresh_slot();
            self.mutable_locals.push(MutableLocal {
                slot,
                name: binding.clone(),
                ty: "I32".to_owned(),
                mutable: false,
            });
            self.locals
                .insert(binding.clone(), LocalBinding::Slot(slot));
            self.local_types.insert(binding.clone(), LowerType::I32);
            slot
        });
        let body = self.lower_body(&expr.body, false);
        let nested_instructions = std::mem::take(&mut self.instructions);
        self.instructions = saved_instructions;
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        (
            index_slot,
            NestedBodyLowering {
                instructions: nested_instructions,
                result: body.result,
                falls_through: body.falls_through,
            },
        )
    }

    fn emit_runtime_value(&mut self, value: &RuntimeValue) -> ValueId {
        match value {
            RuntimeValue::Int(value) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstInt {
                    dest,
                    value: *value,
                });
                dest
            }
            RuntimeValue::F64(value) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstF64 {
                    dest,
                    bits: value.to_bits(),
                });
                dest
            }
            RuntimeValue::Bool(value) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstBool {
                    dest,
                    value: *value,
                });
                dest
            }
            RuntimeValue::Text(value) => {
                let dest = self.fresh_value();
                self.instructions.push(Inst::ConstText {
                    dest,
                    value: value.clone(),
                });
                dest
            }
            RuntimeValue::TextBuilder(_) => {
                self.diagnostics.push(Diagnostic::new(
                    "mir.text-builder-const",
                    format!(
                        "failed to lower compile-time text builder value in `{}` because text builders are runtime-only",
                        self.function.name
                    ),
                    self.function.span,
                    Some("Finish text builders at runtime and keep const values on stage-0 text literals.".to_owned()),
                ));
                self.emit_unit_value()
            }
            RuntimeValue::List(_) => {
                self.diagnostics.push(Diagnostic::new(
                    "mir.list-const",
                    format!(
                        "failed to lower compile-time List value in `{}` because List handles are runtime-only",
                        self.function.name
                    ),
                    self.function.span,
                    Some(
                        "Construct List values at runtime with `list_new(...)` and keep const values on fixed arrays."
                            .to_owned(),
                    ),
                ));
                self.emit_unit_value()
            }
            RuntimeValue::Enum(value) => {
                let payload = value
                    .payload
                    .as_deref()
                    .map(|payload| self.emit_runtime_value(payload));
                let dest = self.fresh_value();
                self.instructions.push(Inst::MakeEnum {
                    dest,
                    name: value.name.clone(),
                    variant: value.variant.clone(),
                    payload,
                });
                dest
            }
            RuntimeValue::Record(value) => {
                if let Some((element_ty, len)) = synthetic_array_record_info(value) {
                    let array_ty = LowerType::Array(Box::new(element_ty), len);
                    let Some(struct_name) = self.register_type_name(&array_ty) else {
                        self.diagnostics.push(Diagnostic::new(
                            "mir.array-const-type",
                            format!(
                                "failed to lower compile-time array value in `{}` because its element type is unsupported",
                                self.function.name
                            ),
                            self.function.span,
                            Some("Keep compile-time arrays on stage-0 supported element types.".to_owned()),
                        ));
                        return self.emit_unit_value();
                    };
                    let fields = value
                        .fields
                        .iter()
                        .map(|(name, value)| (name.clone(), self.emit_runtime_value(value)))
                        .collect();
                    let dest = self.fresh_value();
                    self.instructions.push(Inst::MakeRecord {
                        dest,
                        name: struct_name,
                        fields,
                    });
                    return dest;
                }
                let fields = value
                    .fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.emit_runtime_value(value)))
                    .collect();
                let dest = self.fresh_value();
                self.instructions.push(Inst::MakeRecord {
                    dest,
                    name: value.name.clone(),
                    fields,
                });
                dest
            }
            RuntimeValue::Unit => self.emit_unit_value(),
        }
    }

    fn lower_text_len_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextLen { dest, text });
        dest
    }

    fn lower_text_builder_new_expr(&mut self, _expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextBuilderNew { dest });
        dest
    }

    fn lower_text_builder_append_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let builder = self.lower_expr(arg0);
        let text = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextBuilderAppend {
            dest,
            builder,
            text,
        });
        dest
    }

    fn lower_text_builder_append_codepoint_expr(
        &mut self,
        expr: &sarif_frontend::hir::CallExpr,
    ) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let builder = self.lower_expr(arg0);
        let codepoint = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextBuilderAppendCodepoint {
            dest,
            builder,
            codepoint,
        });
        dest
    }

    fn lower_text_builder_finish_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let builder = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions
            .push(Inst::TextBuilderFinish { dest, builder });
        dest
    }

    fn lower_list_new_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let len = self.lower_expr(arg0);
        let value = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ListNew { dest, len, value });
        dest
    }

    fn lower_list_len_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let list = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ListLen { dest, list });
        dest
    }

    fn lower_list_get_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let list = self.lower_expr(arg0);
        let index = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ListGet { dest, list, index });
        dest
    }

    fn lower_list_set_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let Some(arg2) = expr.args.get(2) else {
            return self.emit_unit_value();
        };
        let list = self.lower_expr(arg0);
        let index = self.lower_expr(arg1);
        let value = self.lower_expr(arg2);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ListSet {
            dest,
            list,
            index,
            value,
        });
        dest
    }

    fn lower_f64_from_i32_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let value = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::F64FromI32 { dest, value });
        dest
    }

    fn lower_text_concat_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let left = self.lower_expr(arg0);
        let right = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions
            .push(Inst::TextConcat { dest, left, right });
        dest
    }

    fn lower_text_slice_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let Some(arg2) = expr.args.get(2) else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg0);
        let start = self.lower_expr(arg1);
        let end = self.lower_expr(arg2);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextSlice {
            dest,
            text,
            start,
            end,
        });
        dest
    }

    fn lower_text_byte_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg0);
        let index = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextByte { dest, text, index });
        dest
    }

    fn lower_text_cmp_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let left = self.lower_expr(arg0);
        let right = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextCmp { dest, left, right });
        dest
    }

    fn lower_text_eq_range_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let Some(arg2) = expr.args.get(2) else {
            return self.emit_unit_value();
        };
        let Some(arg3) = expr.args.get(3) else {
            return self.emit_unit_value();
        };
        let source = self.lower_expr(arg0);
        let start = self.lower_expr(arg1);
        let end = self.lower_expr(arg2);
        let expected = self.lower_expr(arg3);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextEqRange {
            dest,
            source,
            start,
            end,
            expected,
        });
        dest
    }

    fn lower_text_find_byte_range_expr(
        &mut self,
        expr: &sarif_frontend::hir::CallExpr,
    ) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let Some(arg2) = expr.args.get(2) else {
            return self.emit_unit_value();
        };
        let Some(arg3) = expr.args.get(3) else {
            return self.emit_unit_value();
        };
        let source = self.lower_expr(arg0);
        let start = self.lower_expr(arg1);
        let end = self.lower_expr(arg2);
        let byte = self.lower_expr(arg3);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextFindByteRange {
            dest,
            source,
            start,
            end,
            byte,
        });
        dest
    }

    fn lower_text_from_f64_fixed_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let value = self.lower_expr(arg0);
        let digits = self.lower_expr(arg1);
        let dest = self.fresh_value();
        self.instructions.push(Inst::TextFromF64Fixed {
            dest,
            value,
            digits,
        });
        dest
    }

    fn lower_arg_count_expr(&mut self, _expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let dest = self.fresh_value();
        self.instructions.push(Inst::ArgCount { dest });
        dest
    }

    fn lower_alloc_push_expr(&mut self, _expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        self.instructions.push(Inst::AllocPush);
        self.emit_unit_value()
    }

    fn lower_alloc_pop_expr(&mut self, _expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        self.instructions.push(Inst::AllocPop);
        self.emit_unit_value()
    }

    fn lower_arg_text_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let index = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ArgText { dest, index });
        dest
    }

    fn lower_stdin_text_expr(&mut self, _expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let dest = self.fresh_value();
        self.instructions.push(Inst::StdinText { dest });
        dest
    }

    fn lower_stdout_write_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg);
        self.instructions.push(Inst::StdoutWrite { text });
        self.emit_unit_value()
    }

    fn lower_sqrt_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let value = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::Sqrt { dest, value });
        dest
    }

    fn lower_parse_i32_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ParseI32 { dest, text });
        dest
    }

    fn lower_parse_i32_range_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg0) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let Some(arg1) = expr.args.get(1) else {
            return self.emit_unit_value();
        };
        let Some(arg2) = expr.args.get(2) else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg0);
        let start = self.lower_expr(arg1);
        let end = self.lower_expr(arg2);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ParseI32Range {
            dest,
            text,
            start,
            end,
        });
        dest
    }

    fn lower_parse_f64_expr(&mut self, expr: &sarif_frontend::hir::CallExpr) -> ValueId {
        let Some(arg) = expr.args.first() else {
            return self.emit_unit_value();
        };
        let text = self.lower_expr(arg);
        let dest = self.fresh_value();
        self.instructions.push(Inst::ParseF64 { dest, text });
        dest
    }

    fn emit_unit_value(&mut self) -> ValueId {
        let condition = self.const_true();
        let dest = self.fresh_value();
        self.instructions.push(Inst::If {
            dest,
            condition,
            then_insts: Vec::new(),
            then_result: None,
            else_insts: Vec::new(),
            else_result: None,
        });
        dest
    }
}

struct NestedBodyLowering {
    instructions: Vec<Inst>,
    result: Option<ValueId>,
    falls_through: bool,
}

struct NestedExprLowering {
    instructions: Vec<Inst>,
    value: ValueId,
}

fn expr_returns_unit(expr: &Expr) -> bool {
    match expr {
        Expr::Repeat(_) | Expr::While(_) => true,
        Expr::Unary(expr) => expr_returns_unit(&expr.inner),
        Expr::Group(expr) => expr_returns_unit(&expr.inner),
        Expr::If(expr) => body_returns_unit(&expr.then_body) && body_returns_unit(&expr.else_body),
        Expr::Match(expr) => expr.arms.iter().all(|arm| body_returns_unit(&arm.body)),
        _ => false,
    }
}

fn body_returns_unit(body: &sarif_frontend::hir::Body) -> bool {
    body.tail.as_ref().is_none_or(expr_returns_unit)
}

fn runtime_value_lower_type(value: &RuntimeValue) -> LowerType {
    match value {
        RuntimeValue::Int(_) => LowerType::I32,
        RuntimeValue::F64(_) => LowerType::F64,
        RuntimeValue::Bool(_) => LowerType::Bool,
        RuntimeValue::Text(_) => LowerType::Text,
        RuntimeValue::TextBuilder(_) => LowerType::TextBuilder,
        RuntimeValue::List(_) => LowerType::List(Box::new(LowerType::Error)), // opaque handle
        RuntimeValue::Enum(value) => LowerType::Named(value.name.clone()),
        RuntimeValue::Record(value) => synthetic_array_record_info(value).map_or_else(
            || LowerType::Named(value.name.clone()),
            |(element_ty, len)| LowerType::Array(Box::new(element_ty), len),
        ),
        RuntimeValue::Unit => LowerType::Unit,
    }
}

fn render_runtime_record(record: &RuntimeRecord) -> String {
    if synthetic_array_record_info(record).is_some() {
        return format!(
            "[{}]",
            record
                .fields
                .iter()
                .map(|(_, value)| value.render())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    format!(
        "{}{{{}}}",
        record.name,
        record
            .fields
            .iter()
            .map(|(name, value)| format!("{name}: {}", value.render()))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn synthetic_array_record_info(record: &RuntimeRecord) -> Option<(LowerType, usize)> {
    if !record.name.starts_with("__Array_") || record.fields.is_empty() {
        return None;
    }
    for (index, (field_name, _)) in record.fields.iter().enumerate() {
        if field_name != &array_field_name(index) {
            return None;
        }
    }
    let element_ty = runtime_value_lower_type(&record.fields.first()?.1);
    if record
        .fields
        .iter()
        .skip(1)
        .any(|(_, value)| runtime_value_lower_type(value) != element_ty)
    {
        return None;
    }
    Some((element_ty, record.fields.len()))
}

fn update_runtime_array_index(
    value: RuntimeValue,
    index: i64,
    replacement: RuntimeValue,
) -> Result<RuntimeValue, String> {
    if index < 0 {
        return Err("compile-time array index is out of bounds".to_owned());
    }
    let RuntimeValue::Record(mut record) = value else {
        return Err("compile-time indexed assignment target is not an array".to_owned());
    };
    let Some((_, len)) = synthetic_array_record_info(&record) else {
        return Err("compile-time indexed assignment target is not an array".to_owned());
    };
    let index = usize::try_from(index).map_err(|_| "compile-time array index is out of bounds")?;
    if index >= len {
        return Err("compile-time array index is out of bounds".to_owned());
    }
    let field_name = array_field_name(index);
    let Some((_, value)) = record
        .fields
        .iter_mut()
        .find(|(name, _)| name == &field_name)
    else {
        return Err("compile-time indexed assignment target is not an array".to_owned());
    };
    *value = replacement;
    Ok(RuntimeValue::Record(record))
}

fn array_struct_name(element_ty: &str, len: usize) -> String {
    let sanitized = element_ty
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    format!("__Array_{sanitized}_{len}")
}

fn array_field_name(index: usize) -> String {
    format!("slot{index}")
}

fn split_enum_variant_path(path: &str) -> Option<(&str, &str)> {
    let (enum_name, variant) = path.rsplit_once('.')?;
    (!enum_name.is_empty() && !variant.is_empty()).then_some((enum_name, variant))
}

#[derive(Debug)]
pub enum RuntimeError {
    Message(String),
    EffectUnwind {
        effect: String,
        operation: String,
        args: Vec<RuntimeValue>,
    },
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

struct Interpreter<'a> {
    enums: BTreeMap<&'a str, &'a EnumType>,
    structs: BTreeMap<&'a str, &'a StructType>,
    functions: BTreeMap<&'a str, &'a Function>,
    program_args: &'a [String],
    stdin_text: String,
    stdout_text: String,
    next_text_builder_id: u64,
    text_builders: BTreeMap<u64, Vec<u8>>,
    next_list_id: u64,
    lists: BTreeMap<u64, Vec<RuntimeValue>>,
    handlers: Vec<Vec<HandleArm>>,
}

impl<'a> Interpreter<'a> {
    fn new(program: &'a Program, program_args: &'a [String], stdin_text: String) -> Self {
        Self {
            enums: program
                .enums
                .iter()
                .map(|enum_ty| (enum_ty.name.as_str(), enum_ty))
                .collect(),
            structs: program
                .structs
                .iter()
                .map(|struct_ty| (struct_ty.name.as_str(), struct_ty))
                .collect(),
            functions: program
                .functions
                .iter()
                .map(|function| (function.name.as_str(), function))
                .collect(),
            program_args,
            stdin_text,
            stdout_text: String::new(),
            next_text_builder_id: 0,
            text_builders: BTreeMap::new(),
            next_list_id: 0,
            lists: BTreeMap::new(),
            handlers: Vec::new(),
        }
    }

    fn take_stdout(self) -> String {
        self.stdout_text
    }

    fn alloc_push(&mut self) {}

    fn alloc_pop(&mut self) {}

    fn run_main(&mut self) -> Result<RuntimeValue, RuntimeError> {
        self.run_function("main", &[])
    }

    fn run_function(
        &mut self,
        name: &str,
        args: &[RuntimeValue],
    ) -> Result<RuntimeValue, RuntimeError> {
        let function = *self
            .functions
            .get(name)
            .ok_or_else(|| RuntimeError::new(format!("unknown function `{name}`")))?;
        if name == "main" && !function.params.is_empty() {
            return Err(RuntimeError::new("`main` must not take parameters"));
        }
        if function.params.len() != args.len() {
            return Err(RuntimeError::new(format!(
                "function `{name}` expected {} arguments but received {}",
                function.params.len(),
                args.len(),
            )));
        }
        for (arg, param) in args.iter().zip(&function.params) {
            self.ensure_type(&param.ty, arg)?;
        }
        self.execute_function(function, args)
    }

    #[allow(clippy::too_many_lines)]
    fn execute_function(
        &mut self,
        function: &Function,
        args: &[RuntimeValue],
    ) -> Result<RuntimeValue, RuntimeError> {
        let mut values = BTreeMap::<ValueId, RuntimeValue>::new();
        let mut slots = BTreeMap::<LocalSlotId, RuntimeValue>::new();
        if let ExecFlow::Return(value) = self.execute_insts(
            function,
            &function.instructions,
            &mut values,
            &mut slots,
            args,
        )? {
            return Ok(value);
        }

        function.result.map_or(Ok(RuntimeValue::Unit), |result| {
            values
                .get(&result)
                .cloned()
                .ok_or_else(|| RuntimeError::new("missing function result"))
        })
    }

    #[allow(clippy::too_many_lines)]
    fn execute_insts(
        &mut self,
        function: &Function,
        instructions: &[Inst],
        values: &mut BTreeMap<ValueId, RuntimeValue>,
        slots: &mut BTreeMap<LocalSlotId, RuntimeValue>,
        args: &[RuntimeValue],
    ) -> Result<ExecFlow, RuntimeError> {
        for inst in instructions {
            match inst {
                Inst::LoadParam { dest, index } => {
                    let value = args
                        .get(*index)
                        .ok_or_else(|| RuntimeError::new("parameter load out of bounds"))?;
                    values.insert(*dest, value.clone());
                }
                Inst::LoadLocal { dest, slot } => {
                    let value = slots.get(slot).cloned().ok_or_else(|| {
                        RuntimeError::new(format!(
                            "mutable local {} is unavailable in `{}`",
                            slot.render(),
                            function.name
                        ))
                    })?;
                    values.insert(*dest, value);
                }
                Inst::StoreLocal { slot, src } => {
                    let value = extract_value(values, *src)?;
                    slots.insert(*slot, value);
                }
                Inst::ConstInt { dest, value } => {
                    values.insert(*dest, RuntimeValue::Int(*value));
                }
                Inst::ConstF64 { dest, bits } => {
                    values.insert(*dest, RuntimeValue::F64(f64::from_bits(*bits)));
                }
                Inst::ConstBool { dest, value } => {
                    values.insert(*dest, RuntimeValue::Bool(*value));
                }
                Inst::ConstText { dest, value } => {
                    values.insert(*dest, RuntimeValue::Text(value.clone()));
                }
                Inst::TextBuilderNew { dest } => {
                    let id = self.next_text_builder_id;
                    self.next_text_builder_id += 1;
                    self.text_builders.insert(id, Vec::new());
                    values.insert(*dest, RuntimeValue::TextBuilder(id));
                }
                Inst::TextBuilderAppend {
                    dest,
                    builder,
                    text,
                } => {
                    let builder_val = extract_value(values, *builder)?;
                    let text_val = extract_value(values, *text)?;
                    let RuntimeValue::TextBuilder(id) = builder_val else {
                        return Err(RuntimeError::new("expected TextBuilder"));
                    };
                    let RuntimeValue::Text(text) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let bytes = self
                        .text_builders
                        .get_mut(&id)
                        .ok_or_else(|| RuntimeError::new("text builder handle is unavailable"))?;
                    bytes.extend_from_slice(text.as_bytes());
                    values.insert(*dest, RuntimeValue::TextBuilder(id));
                }
                Inst::TextBuilderAppendCodepoint {
                    dest,
                    builder,
                    codepoint,
                } => {
                    let builder_val = extract_value(values, *builder)?;
                    let codepoint_val = extract_value(values, *codepoint)?;
                    let RuntimeValue::TextBuilder(id) = builder_val else {
                        return Err(RuntimeError::new("expected TextBuilder"));
                    };
                    let RuntimeValue::Int(codepoint) = codepoint_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let codepoint = u32::try_from(codepoint).map_err(|_| {
                        RuntimeError::new("text builder codepoint must be non-negative")
                    })?;
                    let scalar = char::from_u32(codepoint).ok_or_else(|| {
                        RuntimeError::new("text builder codepoint must be a valid Unicode scalar")
                    })?;
                    let bytes = self
                        .text_builders
                        .get_mut(&id)
                        .ok_or_else(|| RuntimeError::new("text builder handle is unavailable"))?;
                    let mut encoded = [0u8; 4];
                    let encoded = scalar.encode_utf8(&mut encoded);
                    bytes.extend_from_slice(encoded.as_bytes());
                    values.insert(*dest, RuntimeValue::TextBuilder(id));
                }
                Inst::TextBuilderFinish { dest, builder } => {
                    let builder_val = extract_value(values, *builder)?;
                    let RuntimeValue::TextBuilder(id) = builder_val else {
                        return Err(RuntimeError::new("expected TextBuilder"));
                    };
                    let bytes = self
                        .text_builders
                        .remove(&id)
                        .ok_or_else(|| RuntimeError::new("text builder handle is unavailable"))?;
                    let text = String::from_utf8(bytes)
                        .map_err(|_| RuntimeError::new("text builder produced invalid UTF-8"))?;
                    values.insert(*dest, RuntimeValue::Text(text));
                }
                Inst::ListNew { dest, len, value } => {
                    let len_val = extract_value(values, *len)?;
                    let value_val = extract_value(values, *value)?;
                    let RuntimeValue::Int(len) = len_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    if len < 0 {
                        return Err(RuntimeError::new("list_new length must be non-negative"));
                    }
                    let len = usize::try_from(len)
                        .map_err(|_| RuntimeError::new("list_new length exceeds limits"))?;
                    let id = self.next_list_id;
                    self.next_list_id += 1;
                    self.lists.insert(id, vec![value_val; len]);
                    values.insert(*dest, RuntimeValue::List(id));
                }
                Inst::ListLen { dest, list } => {
                    let list_val = extract_value(values, *list)?;
                    let RuntimeValue::List(id) = list_val else {
                        return Err(RuntimeError::new("expected List"));
                    };
                    let list_ref = self
                        .lists
                        .get(&id)
                        .ok_or_else(|| RuntimeError::new("list handle is unavailable"))?;
                    let len = i64::try_from(list_ref.len())
                        .map_err(|_| RuntimeError::new("list length exceeds I32 limits"))?;
                    values.insert(*dest, RuntimeValue::Int(len));
                }
                Inst::ListGet { dest, list, index } => {
                    let list_val = extract_value(values, *list)?;
                    let index_val = extract_value(values, *index)?;
                    let RuntimeValue::List(id) = list_val else {
                        return Err(RuntimeError::new("expected List"));
                    };
                    let RuntimeValue::Int(index) = index_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    if index < 0 {
                        return Err(RuntimeError::new("bounds assertion failed in `list_get`"));
                    }
                    let list_ref = self
                        .lists
                        .get(&id)
                        .ok_or_else(|| RuntimeError::new("list handle is unavailable"))?;
                    let index = usize::try_from(index)
                        .map_err(|_| RuntimeError::new("bounds assertion failed in `list_get`"))?;
                    let value = list_ref.get(index).cloned().ok_or_else(|| {
                        RuntimeError::new("bounds assertion failed in `list_get`")
                    })?;
                    values.insert(*dest, value);
                }
                Inst::ListSet {
                    dest,
                    list,
                    index,
                    value,
                } => {
                    let list_val = extract_value(values, *list)?;
                    let index_val = extract_value(values, *index)?;
                    let value_val = extract_value(values, *value)?;
                    let RuntimeValue::List(id) = list_val else {
                        return Err(RuntimeError::new("expected List"));
                    };
                    let RuntimeValue::Int(index) = index_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    if index < 0 {
                        return Err(RuntimeError::new("bounds assertion failed in `list_set`"));
                    }
                    let list_ref = self
                        .lists
                        .get_mut(&id)
                        .ok_or_else(|| RuntimeError::new("list handle is unavailable"))?;
                    let index = usize::try_from(index)
                        .map_err(|_| RuntimeError::new("bounds assertion failed in `list_set`"))?;
                    let slot = list_ref.get_mut(index).ok_or_else(|| {
                        RuntimeError::new("bounds assertion failed in `list_set`")
                    })?;
                    *slot = value_val;
                    values.insert(*dest, RuntimeValue::List(id));
                }
                Inst::F64FromI32 { dest, value } => {
                    let value_val = extract_value(values, *value)?;
                    let RuntimeValue::Int(value) = value_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    values.insert(*dest, RuntimeValue::F64(value as f64));
                }
                Inst::TextLen { dest, text } => {
                    let text_val = extract_value(values, *text)?;
                    let RuntimeValue::Text(t) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    values.insert(*dest, RuntimeValue::Int(t.len() as i64));
                }
                Inst::TextConcat { dest, left, right } => {
                    let left_val = extract_value(values, *left)?;
                    let right_val = extract_value(values, *right)?;
                    let RuntimeValue::Text(left_text) = left_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Text(right_text) = right_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    if left_text.is_empty() {
                        values.insert(*dest, RuntimeValue::Text(right_text));
                        continue;
                    }
                    if right_text.is_empty() {
                        values.insert(*dest, RuntimeValue::Text(left_text));
                        continue;
                    }
                    let mut value = left_text;
                    value.push_str(&right_text);
                    values.insert(*dest, RuntimeValue::Text(value));
                }
                Inst::TextSlice {
                    dest,
                    text,
                    start,
                    end,
                } => {
                    let text_val = extract_value(values, *text)?;
                    let start_val = extract_value(values, *start)?;
                    let end_val = extract_value(values, *end)?;
                    let RuntimeValue::Text(text) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Int(start) = start_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let RuntimeValue::Int(end) = end_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let bytes = text.as_bytes();
                    let clamped_start = clamp_text_slice_start(bytes, start);
                    let clamped_end = clamp_text_slice_end(bytes, end);
                    let sliced = if clamped_end <= clamped_start {
                        String::new()
                    } else if clamped_start == 0 && clamped_end == bytes.len() {
                        text
                    } else {
                        // The clamped indices are UTF-8 boundaries by construction.
                        unsafe { std::str::from_utf8_unchecked(&bytes[clamped_start..clamped_end]) }
                            .to_owned()
                    };
                    values.insert(*dest, RuntimeValue::Text(sliced));
                }
                Inst::TextByte { dest, text, index } => {
                    let text_val = extract_value(values, *text)?;
                    let index_val = extract_value(values, *index)?;
                    let RuntimeValue::Text(t) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Int(idx) = index_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let byte = t.as_bytes().get(idx as usize).copied().unwrap_or(0);
                    values.insert(*dest, RuntimeValue::Int(byte as i64));
                }
                Inst::TextCmp { dest, left, right } => {
                    let left_val = extract_value(values, *left)?;
                    let right_val = extract_value(values, *right)?;
                    let RuntimeValue::Text(left_text) = left_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Text(right_text) = right_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let cmp = left_text.as_bytes().cmp(right_text.as_bytes());
                    let value = match cmp {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    };
                    values.insert(*dest, RuntimeValue::Int(value));
                }
                Inst::TextEqRange {
                    dest,
                    source,
                    start,
                    end,
                    expected,
                } => {
                    let source_val = extract_value(values, *source)?;
                    let start_val = extract_value(values, *start)?;
                    let end_val = extract_value(values, *end)?;
                    let expected_val = extract_value(values, *expected)?;
                    let RuntimeValue::Text(source_text) = source_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Int(start) = start_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let RuntimeValue::Int(end) = end_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let RuntimeValue::Text(expected_text) = expected_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let bytes = source_text.as_bytes();
                    let clamped_start = clamp_text_slice_start(bytes, start);
                    let raw_end = clamp_text_slice_end(bytes, end);
                    let clamped_end = if raw_end < clamped_start {
                        clamped_start
                    } else {
                        raw_end
                    };
                    let matches = &bytes[clamped_start..clamped_end] == expected_text.as_bytes();
                    values.insert(*dest, RuntimeValue::Bool(matches));
                }
                Inst::TextFindByteRange {
                    dest,
                    source,
                    start,
                    end,
                    byte,
                } => {
                    let source_val = extract_value(values, *source)?;
                    let start_val = extract_value(values, *start)?;
                    let end_val = extract_value(values, *end)?;
                    let byte_val = extract_value(values, *byte)?;
                    let RuntimeValue::Text(source_text) = source_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Int(start) = start_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let RuntimeValue::Int(end) = end_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let RuntimeValue::Int(byte) = byte_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let bytes = source_text.as_bytes();
                    let start = clamp_text_slice_start(bytes, start);
                    let raw_end = clamp_text_slice_end(bytes, end);
                    let end = if raw_end < start { start } else { raw_end };
                    let mut found = end as i64;
                    let mut index = start;
                    while index < end {
                        if bytes[index] == byte as u8 {
                            found = index as i64;
                            break;
                        }
                        index += 1;
                    }
                    values.insert(*dest, RuntimeValue::Int(found));
                }
                Inst::ParseI32 { dest, text } => {
                    let text_val = extract_value(values, *text)?;
                    let RuntimeValue::Text(text) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let parsed = text
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| RuntimeError::new("expected base-10 integer text"))?;
                    values.insert(*dest, RuntimeValue::Int(parsed));
                }
                Inst::ParseI32Range {
                    dest,
                    text,
                    start,
                    end,
                } => {
                    let text_val = extract_value(values, *text)?;
                    let start_val = extract_value(values, *start)?;
                    let end_val = extract_value(values, *end)?;
                    let RuntimeValue::Text(text) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let RuntimeValue::Int(start) = start_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let RuntimeValue::Int(end) = end_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let bytes = text.as_bytes();
                    let start = clamp_text_slice_start(bytes, start);
                    let raw_end = clamp_text_slice_end(bytes, end);
                    let end = if raw_end < start { start } else { raw_end };
                    let parsed = std::str::from_utf8(&bytes[start..end])
                        .map_err(|_| RuntimeError::new("expected utf-8 text"))?
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| RuntimeError::new("expected base-10 integer text"))?;
                    values.insert(*dest, RuntimeValue::Int(parsed));
                }
                Inst::ParseF64 { dest, text } => {
                    let text_val = extract_value(values, *text)?;
                    let RuntimeValue::Text(text) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    let parsed = text
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| RuntimeError::new("expected float text"))?;
                    values.insert(*dest, RuntimeValue::F64(parsed));
                }
                Inst::TextFromF64Fixed {
                    dest,
                    value,
                    digits,
                } => {
                    let value = extract_value(values, *value)?;
                    let digits = extract_value(values, *digits)?;
                    let RuntimeValue::F64(value) = value else {
                        return Err(RuntimeError::new("expected F64"));
                    };
                    let RuntimeValue::Int(digits) = digits else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    values.insert(*dest, RuntimeValue::Text(format_f64_fixed(value, digits)));
                }
                Inst::Sqrt { dest, value } => {
                    let value = extract_value(values, *value)?;
                    let RuntimeValue::F64(value) = value else {
                        return Err(RuntimeError::new("expected F64"));
                    };
                    values.insert(*dest, RuntimeValue::F64(value.sqrt()));
                }
                Inst::ArgCount { dest } => {
                    values.insert(*dest, RuntimeValue::Int(self.program_args.len() as i64));
                }
                Inst::AllocPush => {
                    self.alloc_push();
                }
                Inst::AllocPop => {
                    self.alloc_pop();
                }
                Inst::ArgText { dest, index } => {
                    let index_val = extract_value(values, *index)?;
                    let RuntimeValue::Int(idx) = index_val else {
                        return Err(RuntimeError::new("expected Int"));
                    };
                    let text = if idx < 0 {
                        String::new()
                    } else {
                        self.program_args
                            .get(idx as usize)
                            .cloned()
                            .unwrap_or_default()
                    };
                    values.insert(*dest, RuntimeValue::Text(text));
                }
                Inst::StdinText { dest } => {
                    values.insert(*dest, RuntimeValue::Text(self.stdin_text.clone()));
                }
                Inst::StdoutWrite { text } => {
                    let text_val = extract_value(values, *text)?;
                    let RuntimeValue::Text(text) = text_val else {
                        return Err(RuntimeError::new("expected Text"));
                    };
                    self.stdout_text.push_str(&text);
                }
                Inst::MakeEnum {
                    dest,
                    name,
                    variant,
                    payload,
                } => {
                    let payload = payload
                        .map(|value| extract_value(values, value))
                        .transpose()?
                        .map(Box::new);
                    values.insert(
                        *dest,
                        RuntimeValue::Enum(RuntimeEnum {
                            name: name.clone(),
                            variant: variant.clone(),
                            payload,
                        }),
                    );
                }
                Inst::MakeRecord { dest, name, fields } => {
                    let mut runtime_fields = Vec::with_capacity(fields.len());
                    for (field_name, value) in fields {
                        let field_value = extract_value(values, *value)?;
                        runtime_fields.push((field_name.clone(), field_value));
                    }
                    values.insert(
                        *dest,
                        RuntimeValue::Record(RuntimeRecord {
                            name: name.clone(),
                            fields: runtime_fields,
                        }),
                    );
                }
                Inst::Field { dest, base, name } => {
                    let base = extract_value(values, *base)?;
                    let RuntimeValue::Record(record) = base else {
                        return Err(RuntimeError::new(format!(
                            "expected record value for field access `{name}`"
                        )));
                    };
                    let value = record
                        .fields
                        .iter()
                        .find_map(|(field_name, value)| (field_name == name).then(|| value.clone()))
                        .ok_or_else(|| {
                            RuntimeError::new(format!(
                                "record `{}` has no field `{name}`",
                                record.name
                            ))
                        })?;
                    values.insert(*dest, value);
                }
                Inst::EnumTagEq { dest, value, tag } => {
                    let value = extract_value(values, *value)?;
                    let actual = match value {
                        RuntimeValue::Enum(enum_value) => self.enum_tag(&enum_value)?,
                        RuntimeValue::Int(value) => value,
                        _ => {
                            return Err(RuntimeError::new(
                                "expected enum value or enum tag for tag test",
                            ));
                        }
                    };
                    values.insert(*dest, RuntimeValue::Bool(actual == *tag));
                }
                Inst::EnumPayload { dest, value, .. } => {
                    let value = extract_value(values, *value)?;
                    let RuntimeValue::Enum(enum_value) = value else {
                        return Err(RuntimeError::new("expected enum value for payload access"));
                    };
                    let payload = enum_value
                        .payload
                        .map(|payload| *payload)
                        .ok_or_else(|| RuntimeError::new("enum variant has no payload"))?;
                    values.insert(*dest, payload);
                }
                Inst::If {
                    dest,
                    condition,
                    then_insts,
                    then_result,
                    else_insts,
                    else_result,
                } => {
                    let condition = extract_bool(values, *condition)?;
                    let (branch_insts, branch_result) = if condition {
                        (then_insts, then_result)
                    } else {
                        (else_insts, else_result)
                    };
                    let mut branch_values = values.clone();
                    let mut branch_slots = slots.clone();
                    if let ExecFlow::Return(value) = self.execute_insts(
                        function,
                        branch_insts,
                        &mut branch_values,
                        &mut branch_slots,
                        args,
                    )? {
                        return Ok(ExecFlow::Return(value));
                    }
                    *slots = branch_slots;
                    let result = branch_result.map_or(Ok(RuntimeValue::Unit), |result| {
                        branch_values.get(&result).cloned().ok_or_else(|| {
                            RuntimeError::new(format!(
                                "missing conditional branch result in `{}` for {}",
                                function.name,
                                result.render()
                            ))
                        })
                    })?;
                    values.insert(*dest, result);
                }
                Inst::Repeat {
                    dest,
                    count,
                    index_slot,
                    body_insts,
                } => {
                    let count = extract_int(values, *count)?;
                    if count > 0 {
                        for index in 0..count {
                            let mut body_values = values.clone();
                            let mut body_slots = slots.clone();
                            if let Some(slot) = index_slot {
                                body_slots.insert(*slot, RuntimeValue::Int(index));
                            }
                            if let ExecFlow::Return(value) = self.execute_insts(
                                function,
                                body_insts,
                                &mut body_values,
                                &mut body_slots,
                                args,
                            )? {
                                return Ok(ExecFlow::Return(value));
                            }
                            *slots = body_slots;
                        }
                    }
                    values.insert(*dest, RuntimeValue::Unit);
                }
                Inst::While {
                    dest,
                    condition_insts,
                    condition,
                    body_insts,
                } => {
                    loop {
                        let mut condition_values = values.clone();
                        let mut condition_slots = slots.clone();
                        if let ExecFlow::Return(value) = self.execute_insts(
                            function,
                            condition_insts,
                            &mut condition_values,
                            &mut condition_slots,
                            args,
                        )? {
                            return Ok(ExecFlow::Return(value));
                        }
                        let RuntimeValue::Bool(keep_going) =
                            condition_values.get(condition).cloned().ok_or_else(|| {
                                RuntimeError::new(format!(
                                    "missing while condition result in `{}` for {}",
                                    function.name,
                                    condition.render()
                                ))
                            })?
                        else {
                            return Err(RuntimeError::new(format!(
                                "while condition in `{}` did not evaluate to `Bool`",
                                function.name
                            )));
                        };
                        if !keep_going {
                            break;
                        }
                        let mut body_values = values.clone();
                        let mut body_slots = slots.clone();
                        if let ExecFlow::Return(value) = self.execute_insts(
                            function,
                            body_insts,
                            &mut body_values,
                            &mut body_slots,
                            args,
                        )? {
                            return Ok(ExecFlow::Return(value));
                        }
                        *slots = body_slots;
                    }
                    values.insert(*dest, RuntimeValue::Unit);
                }
                Inst::Add { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let value = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                            RuntimeValue::Int(left + right)
                        }
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                            RuntimeValue::F64(left + right)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for add",
                            ));
                        }
                    };
                    values.insert(*dest, value);
                }
                Inst::Sub { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let value = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                            RuntimeValue::Int(left - right)
                        }
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                            RuntimeValue::F64(left - right)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for sub",
                            ));
                        }
                    };
                    values.insert(*dest, value);
                }
                Inst::Mul { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let value = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                            RuntimeValue::Int(left * right)
                        }
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                            RuntimeValue::F64(left * right)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for mul",
                            ));
                        }
                    };
                    values.insert(*dest, value);
                }
                Inst::Div { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let value = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
                            if right == 0 {
                                return Err(RuntimeError::new("division by zero"));
                            }
                            RuntimeValue::Int(left / right)
                        }
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => {
                            if right == 0.0 {
                                return Err(RuntimeError::new("division by zero"));
                            }
                            RuntimeValue::F64(left / right)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for div",
                            ));
                        }
                    };
                    values.insert(*dest, value);
                }
                Inst::And { dest, left, right } => {
                    let left = extract_bool(values, *left)?;
                    let right = extract_bool(values, *right)?;
                    values.insert(*dest, RuntimeValue::Bool(left && right));
                }
                Inst::Or { dest, left, right } => {
                    let left = extract_bool(values, *left)?;
                    let right = extract_bool(values, *right)?;
                    values.insert(*dest, RuntimeValue::Bool(left || right));
                }
                Inst::Eq { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    values.insert(*dest, RuntimeValue::Bool(left == right));
                }
                Inst::Ne { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    values.insert(*dest, RuntimeValue::Bool(left != right));
                }
                Inst::Lt { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let result = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left < right,
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left < right,
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for lt",
                            ));
                        }
                    };
                    values.insert(*dest, RuntimeValue::Bool(result));
                }
                Inst::Le { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let result = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left <= right,
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left <= right,
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for le",
                            ));
                        }
                    };
                    values.insert(*dest, RuntimeValue::Bool(result));
                }
                Inst::Gt { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let result = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left > right,
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left > right,
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for gt",
                            ));
                        }
                    };
                    values.insert(*dest, RuntimeValue::Bool(result));
                }
                Inst::Ge { dest, left, right } => {
                    let left = extract_value(values, *left)?;
                    let right = extract_value(values, *right)?;
                    let result = match (left, right) {
                        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left >= right,
                        (RuntimeValue::F64(left), RuntimeValue::F64(right)) => left >= right,
                        _ => {
                            return Err(RuntimeError::new(
                                "expected matching numeric operands for ge",
                            ));
                        }
                    };
                    values.insert(*dest, RuntimeValue::Bool(result));
                }
                Inst::Call { dest, callee, args } => {
                    let callee_fn = *self
                        .functions
                        .get(callee.as_str())
                        .ok_or_else(|| RuntimeError::new(format!("unknown callee `{callee}`")))?;
                    let arg_values = args
                        .iter()
                        .map(|value| {
                            values.get(value).cloned().ok_or_else(|| {
                                RuntimeError::new(format!("unknown value {}", value.render()))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let result = self.execute_function(callee_fn, &arg_values)?;
                    values.insert(*dest, result);
                }
                Inst::Assert { condition, kind } => {
                    let condition = extract_bool(values, *condition)?;
                    if !condition {
                        let message = match kind {
                            ContractKind::Requires | ContractKind::Ensures => {
                                format!("{} contract failed in `{}`", kind.keyword(), function.name)
                            }
                            ContractKind::Bounds => {
                                format!("bounds assertion failed in `{}`", function.name)
                            }
                        };
                        return Err(RuntimeError::new(message));
                    }
                }
                Inst::Perform {
                    dest,
                    effect,
                    operation,
                    args,
                } => {
                    let arg_values = args
                        .iter()
                        .map(|id| {
                            values.get(id).cloned().ok_or_else(|| {
                                RuntimeError::new(format!("unknown value {}", id.render()))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let matched_arm = self.handlers.iter().rev().find_map(|frame| {
                        frame
                            .iter()
                            .find(|arm| arm.effect == *effect && arm.operation == *operation)
                            .cloned()
                    });
                    if let Some(arm) = matched_arm {
                        // For non-resumable handlers, we just run the instructions and take the result.
                        let mut local_values = BTreeMap::new();
                        let mut local_slots = BTreeMap::new();
                        if let ExecFlow::Return(value) = self.execute_insts(
                            function,
                            &arm.body_insts,
                            &mut local_values,
                            &mut local_slots,
                            &arg_values,
                        )? {
                            values.insert(*dest, value);
                        } else if let Some(result_id) = arm.body_result {
                            let value = local_values
                                .get(&result_id)
                                .cloned()
                                .ok_or_else(|| RuntimeError::new("missing handler arm result"))?;
                            values.insert(*dest, value);
                        } else {
                            values.insert(*dest, RuntimeValue::Unit);
                        }
                    } else {
                        return Err(RuntimeError::EffectUnwind {
                            effect: effect.clone(),
                            operation: operation.clone(),
                            args: arg_values,
                        });
                    }
                }
                Inst::Handle {
                    dest,
                    body_insts,
                    body_result,
                    arms,
                } => {
                    self.handlers.push(arms.clone());
                    let mut local_values = BTreeMap::new();
                    let mut local_slots = BTreeMap::new();
                    let flow = self.execute_insts(
                        function,
                        body_insts,
                        &mut local_values,
                        &mut local_slots,
                        &[],
                    )?;
                    self.handlers.pop();
                    match flow {
                        ExecFlow::Return(value) => return Ok(ExecFlow::Return(value)),
                        ExecFlow::Continue => {
                            if let Some(result_id) = body_result {
                                let value =
                                    local_values.get(result_id).cloned().ok_or_else(|| {
                                        RuntimeError::new("missing handle body result")
                                    })?;
                                values.insert(*dest, value);
                            } else {
                                values.insert(*dest, RuntimeValue::Unit);
                            }
                        }
                    }
                }
            }
        }
        Ok(ExecFlow::Continue)
    }

    fn ensure_type(&self, ty: &str, value: &RuntimeValue) -> Result<(), RuntimeError> {
        match (ty, value) {
            ("I32", RuntimeValue::Int(_))
            | ("F64", RuntimeValue::F64(_))
            | ("Bool", RuntimeValue::Bool(_))
            | ("Text", RuntimeValue::Text(_))
            | ("TextBuilder", RuntimeValue::TextBuilder(_))
            | ("List", RuntimeValue::List(_))
            | ("Unit", RuntimeValue::Unit) => Ok(()),
            (name, RuntimeValue::Enum(enum_value)) => {
                let enum_ty = self
                    .enums
                    .get(name)
                    .ok_or_else(|| RuntimeError::new(format!("unknown enum type `{name}`")))?;
                if enum_value.name != name {
                    return Err(RuntimeError::new(format!(
                        "expected enum `{name}`, found `{}`",
                        enum_value.name
                    )));
                }
                let Some(variant) = enum_ty
                    .variants
                    .iter()
                    .find(|variant| variant.name == enum_value.variant)
                else {
                    return Err(RuntimeError::new(format!(
                        "enum `{name}` has no variant `{}`",
                        enum_value.variant
                    )));
                };
                match (&variant.payload_type, &enum_value.payload) {
                    (Some(expected), Some(payload)) => self.ensure_type(expected, payload)?,
                    (Some(_), None) => {
                        return Err(RuntimeError::new(format!(
                            "enum variant `{name}.{}`
                             requires a payload",
                            enum_value.variant
                        )));
                    }
                    (None, Some(_)) => {
                        return Err(RuntimeError::new(format!(
                            "enum variant `{name}.{}`
                             does not accept a payload",
                            enum_value.variant
                        )));
                    }
                    (None, None) => {}
                }
                Ok(())
            }
            (name, RuntimeValue::Record(record)) => {
                let struct_ty = self
                    .structs
                    .get(name)
                    .ok_or_else(|| RuntimeError::new(format!("unknown struct type `{name}`")))?;
                if record.name != name {
                    return Err(RuntimeError::new(format!(
                        "expected record `{name}`, found `{}`",
                        record.name
                    )));
                }
                if record.fields.len() != struct_ty.fields.len() {
                    return Err(RuntimeError::new(format!(
                        "record `{name}` field count mismatch"
                    )));
                }
                for ((field_name, field_value), field_ty) in
                    record.fields.iter().zip(&struct_ty.fields)
                {
                    if *field_name != field_ty.name {
                        return Err(RuntimeError::new(format!(
                            "record `{name}` field order mismatch: expected `{}`, found `{field_name}`",
                            field_ty.name
                        )));
                    }
                    self.ensure_type(&field_ty.ty, field_value)?;
                }
                Ok(())
            }
            (other, value) => Err(RuntimeError::new(format!(
                "expected `{other}`, found {}",
                value.render()
            ))),
        }
    }

    fn enum_tag(&self, value: &RuntimeEnum) -> Result<i64, RuntimeError> {
        let enum_ty = self
            .enums
            .get(value.name.as_str())
            .ok_or_else(|| RuntimeError::new(format!("unknown enum type `{}`", value.name)))?;
        enum_ty
            .variants
            .iter()
            .position(|variant| variant.name == value.variant)
            .map(|index| i64::try_from(index).expect("enum index fits in i64"))
            .ok_or_else(|| {
                RuntimeError::new(format!(
                    "enum `{}` has no variant `{}`",
                    value.name, value.variant
                ))
            })
    }
}

#[must_use]
pub fn enum_variants(program: &Program) -> BTreeMap<String, Vec<String>> {
    program
        .enums
        .iter()
        .map(|enum_ty| {
            (
                enum_ty.name.clone(),
                enum_ty
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect(),
            )
        })
        .collect()
}

/// # Errors
///
/// Returns an error if the enum type is unknown, the variant is not declared,
/// or the computed stage-0 tag would exceed `i64` limits.
pub fn encode_enum_tag(
    value: &RuntimeEnum,
    enums: &BTreeMap<String, Vec<String>>,
) -> Result<i64, String> {
    let variants = enums
        .get(&value.name)
        .ok_or_else(|| format!("unknown enum type `{}`", value.name))?;
    variants
        .iter()
        .position(|variant| variant == &value.variant)
        .map_or_else(
            || {
                Err(format!(
                    "enum `{}` has no variant `{}`",
                    value.name, value.variant
                ))
            },
            |index| {
                i64::try_from(index)
                    .map_err(|_| format!("enum `{}` exceeds stage-0 limits", value.name))
            },
        )
}

/// # Errors
///
/// Returns an error if the enum type is unknown or the provided stage-0 tag
/// does not name a declared variant.
pub fn decode_enum_tag(
    tag: i64,
    name: &str,
    enums: &BTreeMap<String, Vec<String>>,
) -> Result<RuntimeValue, String> {
    let variants = enums
        .get(name)
        .ok_or_else(|| format!("unknown enum type `{name}`"))?;
    let index =
        usize::try_from(tag).map_err(|_| format!("enum `{name}` tag `{tag}` is out of range"))?;
    let variant = variants
        .get(index)
        .ok_or_else(|| format!("enum `{name}` tag `{tag}` is out of range"))?;
    Ok(RuntimeValue::Enum(RuntimeEnum {
        name: name.to_owned(),
        variant: variant.clone(),
        payload: None,
    }))
}

enum ExecFlow {
    Continue,
    Return(RuntimeValue),
}

fn extract_int(
    values: &BTreeMap<ValueId, RuntimeValue>,
    value: ValueId,
) -> Result<i64, RuntimeError> {
    match values.get(&value) {
        Some(RuntimeValue::Int(value)) => Ok(*value),
        Some(other) => Err(RuntimeError::new(format!(
            "expected integer value, found {}",
            other.render(),
        ))),
        None => Err(RuntimeError::new(format!(
            "unknown value {}",
            value.render()
        ))),
    }
}

fn extract_bool(
    values: &BTreeMap<ValueId, RuntimeValue>,
    value: ValueId,
) -> Result<bool, RuntimeError> {
    match values.get(&value) {
        Some(RuntimeValue::Bool(value)) => Ok(*value),
        Some(other) => Err(RuntimeError::new(format!(
            "expected boolean value, found {}",
            other.render(),
        ))),
        None => Err(RuntimeError::new(format!(
            "unknown value {}",
            value.render()
        ))),
    }
}

fn extract_value(
    values: &BTreeMap<ValueId, RuntimeValue>,
    value: ValueId,
) -> Result<RuntimeValue, RuntimeError> {
    values
        .get(&value)
        .cloned()
        .ok_or_else(|| RuntimeError::new(format!("unknown value {}", value.render())))
}

fn format_f64_fixed(value: f64, digits: i64) -> String {
    let digits = clamp_fixed_decimal_digits(digits);
    format!("{value:.digits$}")
}

const fn clamp_fixed_decimal_digits(digits: i64) -> usize {
    if digits <= 0 {
        0
    } else if digits >= 1_000 {
        1_000
    } else {
        digits as usize
    }
}

fn slice_text(text: &str, start: i64, end: i64) -> String {
    let bytes = text.as_bytes();
    let start = clamp_text_slice_start(bytes, start);
    let end = clamp_text_slice_end(bytes, end);
    if end <= start {
        return String::new();
    }
    // The clamped indices are UTF-8 boundaries by construction.
    unsafe { std::str::from_utf8_unchecked(&bytes[start..end]) }.to_owned()
}

fn clamp_text_slice_start(bytes: &[u8], index: i64) -> usize {
    let len = bytes.len();
    let mut index = usize::try_from(index.max(0)).unwrap_or(usize::MAX).min(len);
    while index < len && is_utf8_continuation(bytes[index]) {
        index += 1;
    }
    index
}

fn clamp_text_slice_end(bytes: &[u8], index: i64) -> usize {
    let len = bytes.len();
    let mut index = usize::try_from(index.max(0)).unwrap_or(usize::MAX).min(len);
    while index < len && is_utf8_continuation(bytes[index]) {
        index -= 1;
    }
    index
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    use crate::{Inst, RuntimeValue, for_each_inst_recursive, lower, run_main, run_main_with_args};

    fn lower_source(source: &str) -> crate::MirLowering {
        let lexed = lex(source);
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        lower(&hir.module)
    }

    fn bootstrap_syntax_source() -> String {
        let root = format!(
            "{}/../../bootstrap/sarif_syntax/src",
            env!("CARGO_MANIFEST_DIR")
        );
        let core = fs::read_to_string(format!("{root}/main.sarif"))
            .expect("bootstrap syntax core should be readable");
        let entry = fs::read_to_string(format!("{root}/selfcheck.sarif"))
            .expect("bootstrap syntax entrypoint should be readable");
        format!("{core}\n{entry}")
    }

    fn run_with_large_stack<T>(label: &'static str, f: impl FnOnce() -> T + Send + 'static) -> T
    where
        T: Send + 'static,
    {
        std::thread::Builder::new()
            .name(label.to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(f)
            .expect("large-stack test thread should start")
            .join()
            .expect("large-stack test thread should complete")
    }

    #[test]
    fn evaluates_comptime_blocks_during_lowering() {
        let mir = lower_source("fn main() -> I32 { let x = comptime { 20 + 22 }; x }");
        let result = run_main(&mir.program).unwrap();
        assert_eq!(result, RuntimeValue::Int(42));
    }

    #[test]
    fn evaluates_comptime_blocks_with_function_calls() {
        let mir = lower_source(
            "fn add(a: I32, b: I32) -> I32 { a + b }\nfn main() -> I32 { comptime { add(20, 22) } }",
        );
        let result = run_main(&mir.program).unwrap();
        assert_eq!(result, RuntimeValue::Int(42));
    }

    #[test]
    fn lowers_and_runs_const_generic_functions() {
        let mir = lower_source(
            "fn first[N](xs: [I32; N]) -> I32 { xs[0] }\nfn main() -> I32 { let xs = [42]; first(xs) }",
        );
        let result = run_main(&mir.program).unwrap();
        assert_eq!(result, RuntimeValue::Int(42));
    }

    #[test]
    fn lowers_and_runs_simple_programs() {
        let mir = lower_source(
            "fn add(left: I32, right: I32) -> I32 { left + right }\nfn main() -> I32 { add(20, 22) }",
        );

        assert!(mir.diagnostics.is_empty());
        let result = run_main(&mir.program).expect("program should run");
        assert_eq!(result, RuntimeValue::Int(42));
    }

    #[test]
    fn bootstrap_syntax_has_no_asymmetric_if_results() {
        run_with_large_stack("bootstrap_syntax_has_no_asymmetric_if_results", || {
            let source = bootstrap_syntax_source();
            let mir = lower_source(&source);

            assert!(mir.diagnostics.is_empty(), "{:#?}", mir.diagnostics);
            for function in &mir.program.functions {
                for_each_inst_recursive(&function.instructions, &mut |inst| {
                    if let Inst::If {
                        then_result,
                        else_result,
                        ..
                    } = inst
                    {
                        assert_eq!(
                            then_result.is_some(),
                            else_result.is_some(),
                            "function `{}` lowers an `if` with asymmetric branch results:\n{}",
                            function.name,
                            mir.program.pretty()
                        );
                    }
                });
            }
        });
    }

    #[test]
    fn bootstrap_syntax_runs_to_expected_score() {
        run_with_large_stack("bootstrap_syntax_runs_to_expected_score", || {
            let source = bootstrap_syntax_source();
            let mir = lower_source(&source);

            assert!(mir.diagnostics.is_empty(), "{:#?}", mir.diagnostics);
            let result = run_main(&mir.program).expect("bootstrap syntax should run");
            assert_eq!(result, RuntimeValue::Int(32));
        });
    }

    #[test]
    fn runs_scalar_match_wildcard_fallbacks() {
        let mir = lower_source(
            r#"
fn classify_len(len: I32) -> I32 {
    match len {
        2 => { 20 },
        3 => { 30 },
        _ => { 99 },
    }
}

fn classify_text(value: Text) -> I32 {
    match value {
        "fn" => { 1 },
        "if" => { 2 },
        _ => { 7 },
    }
}

fn main() -> I32 {
    classify_len(9) + classify_text("let")
}
"#,
        );

        assert!(mir.diagnostics.is_empty(), "{:#?}", mir.diagnostics);
        let result = run_main(&mir.program).expect("program should run");
        assert_eq!(result, RuntimeValue::Int(106));
    }

    #[test]
    fn runs_argument_builtins_with_runtime_args() {
        let mir =
            lower_source("fn main() -> Text { if arg_count() > 1 { arg_text(1) } else { \"\" } }");

        assert!(mir.diagnostics.is_empty(), "{:#?}", mir.diagnostics);
        let result = run_main_with_args(&mir.program, &["sarif".to_owned(), "stage0".to_owned()])
            .expect("program should run");
        assert_eq!(result, RuntimeValue::Text("stage0".to_owned()));
    }

    #[test]
    fn runs_text_builder_append_codepoint_builtin() {
        let mir = lower_source(
            "fn main() -> Text { let mut builder = text_builder_new(); builder = text_builder_append_codepoint(builder, 65); builder = text_builder_append_codepoint(builder, 10); text_builder_finish(builder) }",
        );

        assert!(mir.diagnostics.is_empty(), "{:#?}", mir.diagnostics);
        let result = run_main(&mir.program).expect("program should run");
        assert_eq!(result, RuntimeValue::Text("A\n".to_owned()));
    }

    #[test]
    fn preserves_typed_list_builtin_lowering() {
        let mir = lower_source(
            "fn head(xs: List[I32]) -> I32 { let mut ys = xs; ys = list_set(ys, 0, 42); list_get(ys, 0) }\nfn main() -> I32 { head(list_new(1, 7)) }",
        );

        assert!(mir.diagnostics.is_empty(), "{:#?}", mir.diagnostics);
        assert!(
            mir.program
                .pretty()
                .contains("fn head(xs: List[I32]) -> I32"),
            "{}",
            mir.program.pretty()
        );
        let result = run_main(&mir.program).expect("program should run");
        assert_eq!(result, RuntimeValue::Int(42));
    }
}
