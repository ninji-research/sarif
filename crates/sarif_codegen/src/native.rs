use std::collections::{BTreeMap, BTreeSet};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{AbiParam, BlockArg, InstBuilder, MemFlags, TrapCode, Value, types};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{DataId, FuncId, Linkage, Module};

pub use crate::CodegenValueKind as NativeValueKind;
use crate::{Function, Inst, Program, ValueId, insts_fall_through};

const LIST_LEN_OFFSET: i32 = 0;
const LIST_VALUES_OFFSET: i32 = 8;

#[derive(Clone, Debug)]
pub struct NativeRecord {
    pub name: String,
    pub fields: Vec<NativeRecordField>,
    pub size: u32,
}

#[derive(Clone, Debug)]
pub struct NativeRecordField {
    pub name: String,
    pub kind: NativeValueKind,
    pub offset: u32,
}

#[derive(Clone, Debug)]
pub struct NativeEnum {
    pub variants: Vec<NativeEnumVariant>,
}

#[derive(Clone, Debug)]
pub struct NativeEnumVariant {
    pub name: String,
    pub payload_type: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum NativeValueRepr {
    Native(cranelift_codegen::ir::Value),
    Unit,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ListHeader {
    len: Value,
    values_ptr: Value,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TrustedListAccesses {
    pub pairs: BTreeSet<(ValueId, ValueId)>,
}

impl TrustedListAccesses {
    fn contains(&self, list: ValueId, index: ValueId) -> bool {
        self.pairs.contains(&(list, index))
    }

    fn unique_vecs(&self) -> BTreeSet<ValueId> {
        self.pairs.iter().map(|(vec, _)| *vec).collect()
    }
}

const PAYLOAD_ENUM_SIZE: u32 = 16;

fn native_kind_type(kind: &NativeValueKind) -> cranelift_codegen::ir::types::Type {
    match kind {
        NativeValueKind::Unit => types::I64, // Represented as 0 handle
        NativeValueKind::F64 => types::F64,
        _ => types::I64,
    }
}

fn coerce_var_value(
    builder: &mut FunctionBuilder<'_>,
    value: Value,
    expected: types::Type,
    function: &Function,
    backend: &str,
) -> Result<Value, String> {
    let actual = builder.func.dfg.value_type(value);
    if actual == expected {
        return Ok(value);
    }
    if expected == types::I64 && actual.is_int() {
        return Ok(builder.ins().uextend(types::I64, value));
    }
    if expected == types::I32 && actual.is_int() {
        return Ok(if actual == types::I64 {
            builder.ins().ireduce(types::I32, value)
        } else {
            builder.ins().uextend(types::I32, value)
        });
    }
    Err(format!(
        "{backend} cannot store `{actual}` into mutable local declared as `{expected}` in `{}`",
        function.name
    ))
}

fn float_cc(condition: IntCC) -> Option<FloatCC> {
    match condition {
        IntCC::Equal => Some(FloatCC::Equal),
        IntCC::NotEqual => Some(FloatCC::NotEqual),
        IntCC::SignedLessThan => Some(FloatCC::LessThan),
        IntCC::SignedLessThanOrEqual => Some(FloatCC::LessThanOrEqual),
        IntCC::SignedGreaterThan => Some(FloatCC::GreaterThan),
        IntCC::SignedGreaterThanOrEqual => Some(FloatCC::GreaterThanOrEqual),
        _ => None,
    }
}

/// Build the stage-0 native record layout table for a lowered MIR program.
///
/// # Errors
///
/// Returns an error if a record field uses a native-unsupported type or if a
/// record exceeds stage-0 size limits.
pub fn collect_native_records(program: &Program) -> Result<BTreeMap<String, NativeRecord>, String> {
    let enums = collect_native_enums(program);
    let mut records = BTreeMap::new();
    for record in &program.structs {
        records.insert(
            record.name.clone(),
            NativeRecord {
                name: record.name.clone(),
                fields: Vec::new(),
                size: 0,
            },
        );
    }
    for record in &program.structs {
        let fields = record
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let offset = record_offset(index)?;
                Ok(NativeRecordField {
                    name: field.name.clone(),
                    kind: native_value_kind(&field.ty, &records, &enums)?,
                    offset,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let size = u32::try_from(fields.len())
            .map_err(|_| format!("record `{}` exceeds stage-0 field limits", record.name))?
            .checked_mul(8)
            .ok_or_else(|| format!("record `{}` exceeds stage-0 size limits", record.name))?;
        let entry = records
            .get_mut(&record.name)
            .ok_or_else(|| format!("missing native record entry for `{}`", record.name))?;
        entry.fields = fields;
        entry.size = size;
    }
    Ok(records)
}

#[must_use]
pub fn collect_native_enums(program: &Program) -> BTreeMap<String, NativeEnum> {
    program
        .enums
        .iter()
        .map(|enum_ty| {
            (
                enum_ty.name.clone(),
                NativeEnum {
                    variants: enum_ty
                        .variants
                        .iter()
                        .map(|variant| NativeEnumVariant {
                            name: variant.name.clone(),
                            payload_type: variant.payload_type.clone(),
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

#[must_use]
pub fn native_enum_is_payload_free(enum_ty: &NativeEnum) -> bool {
    enum_ty
        .variants
        .iter()
        .all(|variant| variant.payload_type.is_none())
}

#[must_use]
pub fn native_enum_variant_index(enum_ty: &NativeEnum, name: &str) -> Option<usize> {
    enum_ty
        .variants
        .iter()
        .position(|variant| variant.name == name)
}

pub fn infer_value_kinds(
    function: &Function,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    functions: &[Function],
) -> Result<BTreeMap<ValueId, NativeValueKind>, String> {
    let mut kinds = BTreeMap::new();
    infer_inst_kinds(
        function,
        &function.instructions,
        records,
        enums,
        functions,
        &mut kinds,
    )?;
    Ok(kinds)
}

#[allow(clippy::too_many_lines)]
fn infer_inst_kinds(
    function: &Function,
    instructions: &[Inst],
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    functions: &[Function],
    kinds: &mut BTreeMap<ValueId, NativeValueKind>,
) -> Result<(), String> {
    for inst in instructions {
        match inst {
            Inst::LoadParam { dest, index } => {
                let ty = function
                    .params
                    .get(*index)
                    .map(|param| param.ty.as_str())
                    .ok_or_else(|| {
                        format!(
                            "native parameter load out of bounds in `{}` at index {}",
                            function.name, index
                        )
                    })?;
                kinds.insert(*dest, native_value_kind(ty, records, enums)?);
            }
            Inst::LoadLocal { dest, slot } => {
                let ty = function.mutable_local_type(*slot).ok_or_else(|| {
                    format!(
                        "native mutable local {} is unknown in `{}`",
                        slot.render(),
                        function.name
                    )
                })?;
                kinds.insert(*dest, native_value_kind(ty, records, enums)?);
            }
            Inst::ConstInt { dest, .. }
            | Inst::TextLen { dest, .. }
            | Inst::TextByte { dest, .. }
            | Inst::ArgCount { dest, .. }
            | Inst::ListLen { dest, .. }
            | Inst::ParseI32 { dest, .. } => {
                kinds.insert(*dest, NativeValueKind::I32);
            }
            Inst::ParseF64 { dest, .. } | Inst::F64FromI32 { dest, .. } => {
                kinds.insert(*dest, NativeValueKind::F64);
            }
            Inst::ListGet { dest, list, .. } => {
                let Some(NativeValueKind::List(element)) = kinds.get(list) else {
                    return Err(format!(
                        "native list_get input {} is not a list in `{}`",
                        list.render(),
                        function.name
                    ));
                };
                kinds.insert(*dest, (**element).clone());
            }

            Inst::TextBuilderNew { dest }
            | Inst::TextBuilderAppend { dest, .. }
            | Inst::TextBuilderAppendCodepoint { dest, .. } => {
                kinds.insert(*dest, NativeValueKind::TextBuilder);
            }
            Inst::ListNew { dest, value, .. } => {
                let Some(kind) = kinds.get(value).cloned() else {
                    return Err(format!(
                        "native list_new input {} has unknown kind in `{}`",
                        value.render(),
                        function.name
                    ));
                };
                kinds.insert(*dest, NativeValueKind::List(Box::new(kind)));
            }
            Inst::ListSet { dest, list, .. } => {
                let Some(kind) = kinds.get(list).cloned() else {
                    return Err(format!(
                        "native list_set input {} has unknown kind in `{}`",
                        list.render(),
                        function.name
                    ));
                };
                kinds.insert(*dest, kind);
            }

            Inst::TextConcat { dest, .. }
            | Inst::TextSlice { dest, .. }
            | Inst::TextBuilderFinish { dest, .. }
            | Inst::TextFromF64Fixed { dest, .. }
            | Inst::ArgText { dest, .. }
            | Inst::StdinText { dest } => {
                kinds.insert(*dest, NativeValueKind::Text);
            }
            Inst::StdoutWrite { .. } => {}
            Inst::ConstF64 { dest, .. } | Inst::Sqrt { dest, .. } => {
                kinds.insert(*dest, NativeValueKind::F64);
            }
            Inst::Add { dest, left, .. }
            | Inst::Sub { dest, left, .. }
            | Inst::Mul { dest, left, .. }
            | Inst::Div { dest, left, .. } => {
                let Some(kind) = kinds.get(left).cloned() else {
                    return Err(format!(
                        "native arithmetic input {} has unknown kind in `{}`",
                        left.render(),
                        function.name
                    ));
                };
                match kind {
                    NativeValueKind::I32 | NativeValueKind::F64 => {
                        kinds.insert(*dest, kind);
                    }
                    other => {
                        return Err(format!(
                            "native arithmetic in `{}` only supports numeric kinds, found `{other:?}`",
                            function.name
                        ));
                    }
                }
            }
            Inst::ConstBool { dest, .. }
            | Inst::And { dest, .. }
            | Inst::Or { dest, .. }
            | Inst::Eq { dest, .. }
            | Inst::Ne { dest, .. }
            | Inst::Lt { dest, .. }
            | Inst::Le { dest, .. }
            | Inst::Gt { dest, .. }
            | Inst::Ge { dest, .. }
            | Inst::EnumTagEq { dest, .. } => {
                kinds.insert(*dest, NativeValueKind::Bool);
            }
            Inst::ConstText { dest, .. } => {
                kinds.insert(*dest, NativeValueKind::Text);
            }
            Inst::MakeEnum { dest, name, .. } => {
                kinds.insert(*dest, NativeValueKind::Enum(name.clone()));
            }
            Inst::EnumPayload {
                dest, payload_type, ..
            } => {
                kinds.insert(*dest, native_value_kind(payload_type, records, enums)?);
            }
            Inst::MakeRecord { dest, name, .. } => {
                kinds.insert(*dest, NativeValueKind::Record(name.clone()));
            }
            Inst::Field { dest, base, name } => {
                let Some(NativeValueKind::Record(record_name)) = kinds.get(base) else {
                    return Err(format!(
                        "native field base {} is not a record in `{}`",
                        base.render(),
                        function.name
                    ));
                };
                let record = records
                    .get(record_name)
                    .ok_or_else(|| format!("missing native record metadata for `{record_name}`"))?;
                let field = record
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .ok_or_else(|| {
                        format!(
                            "record `{record_name}` has no native field `{name}` in `{}`",
                            function.name
                        )
                    })?;
                kinds.insert(*dest, field.kind.clone());
            }
            Inst::Call { dest, callee, .. } => {
                let Some(return_type) = functions
                    .iter()
                    .find(|candidate| candidate.name == *callee)
                    .and_then(|callee| callee.return_type.as_deref())
                else {
                    continue;
                };
                if return_type != "Unit" {
                    kinds.insert(*dest, native_value_kind(return_type, records, enums)?);
                }
            }
            Inst::If {
                dest,
                then_insts,
                then_result,
                else_insts,
                else_result,
                ..
            } => {
                infer_inst_kinds(function, then_insts, records, enums, functions, kinds)?;
                infer_inst_kinds(function, else_insts, records, enums, functions, kinds)?;
                let then_falls = insts_fall_through(then_insts);
                let else_falls = insts_fall_through(else_insts);
                let then_kind = branch_result_kind(kinds, *then_result, function, "then")?;
                let else_kind = branch_result_kind(kinds, *else_result, function, "else")?;
                if then_falls && else_falls {
                    match (then_kind, else_kind) {
                        (Some(left), Some(right)) if left == right => {
                            kinds.insert(*dest, left);
                        }
                        (Some(left), Some(right)) => {
                            return Err(format!(
                                "native conditional branches in `{}` produce incompatible kinds `{left:?}` and `{right:?}`",
                                function.name
                            ));
                        }
                        (None, None) => {}
                        _ => {
                            return Err(format!(
                                "native conditional fallthrough branches in `{}` do not agree on whether they produce a value",
                                function.name
                            ));
                        }
                    }
                } else if then_falls {
                    if let Some(kind) = then_kind {
                        kinds.insert(*dest, kind);
                    }
                } else if else_falls && let Some(kind) = else_kind {
                    kinds.insert(*dest, kind);
                }
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                infer_inst_kinds(function, condition_insts, records, enums, functions, kinds)?;
                infer_inst_kinds(function, body_insts, records, enums, functions, kinds)?;
            }
            Inst::Repeat { body_insts, .. } => {
                infer_inst_kinds(function, body_insts, records, enums, functions, kinds)?;
            }
            Inst::StoreLocal { .. } | Inst::Assert { .. } => {}
            Inst::Perform { .. } | Inst::Handle { .. } => {}
        }
    }
    Ok(())
}

pub fn native_value_kind(
    name: &str,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) -> Result<NativeValueKind, String> {
    if let Some(element) = name.strip_prefix("List[").and_then(|s| s.strip_suffix(']')) {
        let element_kind = native_value_kind(element, records, enums)?;
        return Ok(NativeValueKind::List(Box::new(element_kind)));
    }
    match name {
        "I32" => Ok(NativeValueKind::I32),
        "F64" => Ok(NativeValueKind::F64),
        "Bool" => Ok(NativeValueKind::Bool),
        "Text" => Ok(NativeValueKind::Text),
        "TextBuilder" => Ok(NativeValueKind::TextBuilder),
        "List" => Ok(NativeValueKind::List(Box::new(NativeValueKind::F64))),
        "Unit" => Err("unit should be represented as an omitted native value type".to_owned()),
        other if enums.contains_key(other) => Ok(NativeValueKind::Enum(other.to_owned())),
        other if records.contains_key(other) => Ok(NativeValueKind::Record(other.to_owned())),
        other => Err(format!(
            "native backend does not support values of type `{other}` in stage-0"
        )),
    }
}

pub fn record_offset(index: usize) -> Result<u32, String> {
    let index =
        u32::try_from(index).map_err(|_| "record index exceeds stage-0 limits".to_owned())?;
    index
        .checked_mul(8)
        .ok_or_else(|| "record offset exceeds stage-0 limits".to_owned())
}

#[allow(clippy::too_many_arguments)]
pub fn lower_comparison<M: Module>(
    module: &mut M,
    builder: &mut FunctionBuilder<'_>,
    values: &BTreeMap<ValueId, NativeValueRepr>,
    value_kinds: &BTreeMap<ValueId, NativeValueKind>,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    left: ValueId,
    right: ValueId,
    function: &Function,
    condition: IntCC,
    backend: &str,
) -> Result<NativeValueRepr, String> {
    if let Some(kind) = value_kinds.get(&left)
        && matches!(
            kind,
            NativeValueKind::Text | NativeValueKind::Record(_) | NativeValueKind::Enum(_)
        )
    {
        let left = native_value(values, left, function, "comparison left operand", backend)?;
        let right = native_value(values, right, function, "comparison right operand", backend)?;
        let value = lower_native_kind_comparison(
            module, builder, text_eq_id, records, enums, left, right, kind, condition, backend,
            function,
        )?;
        return Ok(NativeValueRepr::Native(value));
    }
    if matches!(value_kinds.get(&left), Some(NativeValueKind::F64)) {
        let left_float = native_value(values, left, function, "comparison left operand", backend)?;
        let right_float =
            native_value(values, right, function, "comparison right operand", backend)?;
        if matches!(condition, IntCC::NotEqual) {
            let eq = builder.ins().fcmp(FloatCC::Equal, left_float, right_float);
            let ne = builder.ins().bnot(eq);
            let native = builder.ins().uextend(types::I64, ne);
            return Ok(NativeValueRepr::Native(native));
        }
        let Some(float_condition) = float_cc(condition) else {
            return Err(format!(
                "{backend} cannot lower float comparison `{condition:?}` in `{}`",
                function.name
            ));
        };
        let compare = builder.ins().fcmp(float_condition, left_float, right_float);
        let native = builder.ins().uextend(types::I64, compare);
        return Ok(NativeValueRepr::Native(native));
    }
    match (
        value_repr(values, left, function, "comparison left operand", backend)?,
        value_repr(values, right, function, "comparison right operand", backend)?,
    ) {
        (NativeValueRepr::Unit, NativeValueRepr::Unit) => {
            let value = builder
                .ins()
                .iconst(types::I64, i64::from(matches!(condition, IntCC::Equal)));
            Ok(NativeValueRepr::Native(value))
        }
        (NativeValueRepr::Native(left), NativeValueRepr::Native(right)) => {
            let compare = builder.ins().icmp(condition, left, right);
            let native = builder.ins().uextend(types::I64, compare);
            Ok(NativeValueRepr::Native(native))
        }
        _ => Err(format!(
            "{backend} cannot compare unit and non-unit values in `{}`",
            function.name
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_native_kind_comparison<M: Module>(
    module: &mut M,
    builder: &mut FunctionBuilder<'_>,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    left: cranelift_codegen::ir::Value,
    right: cranelift_codegen::ir::Value,
    kind: &NativeValueKind,
    condition: IntCC,
    backend: &str,
    function: &Function,
) -> Result<cranelift_codegen::ir::Value, String> {
    match kind {
        NativeValueKind::F64 => {
            let left_float = left;
            let right_float = right;
            if matches!(condition, IntCC::NotEqual) {
                let eq = builder.ins().fcmp(FloatCC::Equal, left_float, right_float);
                let ne = builder.ins().bnot(eq);
                return Ok(builder.ins().uextend(types::I64, ne));
            }
            let Some(float_condition) = float_cc(condition) else {
                return Err(format!(
                    "{backend} cannot lower float comparison `{condition:?}` in `{}`",
                    function.name
                ));
            };
            let compare = builder.ins().fcmp(float_condition, left_float, right_float);
            Ok(builder.ins().uextend(types::I64, compare))
        }
        _ => {
            let equal = lower_native_kind_equality(
                module, builder, text_eq_id, records, enums, left, right, kind, backend, function,
            )?;
            if matches!(condition, IntCC::NotEqual) {
                let compare = builder.ins().icmp_imm(IntCC::Equal, equal, 0);
                Ok(builder.ins().uextend(types::I64, compare))
            } else {
                Ok(equal)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_native_kind_equality<M: Module>(
    module: &mut M,
    builder: &mut FunctionBuilder<'_>,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    left: cranelift_codegen::ir::Value,
    right: cranelift_codegen::ir::Value,
    kind: &NativeValueKind,
    backend: &str,
    function: &Function,
) -> Result<cranelift_codegen::ir::Value, String> {
    match kind {
        NativeValueKind::Unit
        | NativeValueKind::I32
        | NativeValueKind::Bool
        | NativeValueKind::TextBuilder
        | NativeValueKind::List(_) => {
            let compare = builder.ins().icmp(IntCC::Equal, left, right);
            Ok(builder.ins().uextend(types::I64, compare))
        }
        NativeValueKind::F64 => {
            let left_float = left;
            let right_float = right;
            let compare = builder.ins().fcmp(FloatCC::Equal, left_float, right_float);
            Ok(builder.ins().uextend(types::I64, compare))
        }
        NativeValueKind::Text => {
            let helper = module.declare_func_in_func(text_eq_id, builder.func);
            let call = builder.ins().call(helper, &[left, right]);
            Ok(*builder
                .inst_results(call)
                .first()
                .expect("text equality helper returns a value"))
        }
        NativeValueKind::Record(name) => {
            let record = records
                .get(name)
                .ok_or_else(|| format!("missing native record metadata for `{name}`"))?;
            lower_record_pointer_comparison(
                module, builder, text_eq_id, records, enums, left, right, record, backend, function,
            )
        }
        NativeValueKind::Enum(name) => {
            let enum_ty = enums
                .get(name)
                .ok_or_else(|| format!("missing native enum metadata for `{name}`"))?;
            if native_enum_is_payload_free(enum_ty) {
                let compare = builder.ins().icmp(IntCC::Equal, left, right);
                Ok(builder.ins().uextend(types::I64, compare))
            } else {
                lower_enum_pointer_comparison(
                    module, builder, text_eq_id, records, enums, left, right, enum_ty, backend,
                    function,
                )
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_record_pointer_comparison<M: Module>(
    module: &mut M,
    builder: &mut FunctionBuilder<'_>,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    left: cranelift_codegen::ir::Value,
    right: cranelift_codegen::ir::Value,
    record: &NativeRecord,
    backend: &str,
    function: &Function,
) -> Result<cranelift_codegen::ir::Value, String> {
    let mut result = builder.ins().iconst(types::I64, 1);
    for field in &record.fields {
        let field_ty = native_kind_type(&field.kind);
        let left_field = builder.ins().load(
            field_ty,
            MemFlags::new(),
            left,
            i32::try_from(field.offset)
                .map_err(|_| "record offset exceeds backend limits".to_owned())?,
        );
        let right_field = builder.ins().load(
            field_ty,
            MemFlags::new(),
            right,
            i32::try_from(field.offset)
                .map_err(|_| "record offset exceeds backend limits".to_owned())?,
        );
        let field_equal = lower_native_kind_equality(
            module,
            builder,
            text_eq_id,
            records,
            enums,
            left_field,
            right_field,
            &field.kind,
            backend,
            function,
        )?;
        result = builder.ins().band(result, field_equal);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn lower_enum_pointer_comparison<M: Module>(
    module: &mut M,
    builder: &mut FunctionBuilder<'_>,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    left: cranelift_codegen::ir::Value,
    right: cranelift_codegen::ir::Value,
    enum_ty: &NativeEnum,
    backend: &str,
    function: &Function,
) -> Result<cranelift_codegen::ir::Value, String> {
    let left_tag = builder.ins().load(types::I64, MemFlags::new(), left, 0);
    let right_tag = builder.ins().load(types::I64, MemFlags::new(), right, 0);
    let tag_equal = builder.ins().icmp(IntCC::Equal, left_tag, right_tag);
    let mut result = builder.ins().uextend(types::I64, tag_equal);

    for (index, variant) in enum_ty.variants.iter().enumerate() {
        let Some(payload_type) = &variant.payload_type else {
            continue;
        };
        let left_matches = builder.ins().icmp_imm(
            IntCC::Equal,
            left_tag,
            i64::try_from(index).expect("enum tag should fit i64"),
        );
        let left_matches = builder.ins().uextend(types::I64, left_matches);
        let payload_kind = native_value_kind(payload_type, records, enums)?;
        let payload_ty = native_kind_type(&payload_kind);
        let left_payload = builder.ins().load(payload_ty, MemFlags::new(), left, 8);
        let right_payload = builder.ins().load(payload_ty, MemFlags::new(), right, 8);
        let payload_equal = lower_native_kind_equality(
            module,
            builder,
            text_eq_id,
            records,
            enums,
            left_payload,
            right_payload,
            &payload_kind,
            backend,
            function,
        )?;
        let one = builder.ins().iconst(types::I64, 1);
        let not_variant = builder.ins().bxor(left_matches, one);
        let variant_ok = builder.ins().bor(not_variant, payload_equal);
        result = builder.ins().band(result, variant_ok);
    }

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub fn lower_make_enum<M: Module>(
    module: &mut M,
    builder: &mut FunctionBuilder<'_>,
    allocator_id: FuncId,
    values: &mut BTreeMap<ValueId, NativeValueRepr>,
    enums: &BTreeMap<String, NativeEnum>,
    function: &Function,
    dest: ValueId,
    name: &str,
    variant: &str,
    payload: Option<ValueId>,
    backend: &str,
) -> Result<(), String> {
    let enum_ty = enums
        .get(name)
        .ok_or_else(|| format!("missing native enum metadata for `{name}`"))?;
    let tag = native_enum_variant_index(enum_ty, variant).ok_or_else(|| {
        format!(
            "enum `{name}` has no variant `{variant}` in `{}`",
            function.name
        )
    })?;
    if native_enum_is_payload_free(enum_ty) {
        let native = builder.ins().iconst(
            types::I64,
            i64::try_from(tag).expect("enum tag should fit i64"),
        );
        values.insert(dest, NativeValueRepr::Native(native));
        return Ok(());
    }

    let allocator = module.declare_func_in_func(allocator_id, builder.func);
    let size = builder
        .ins()
        .iconst(types::I64, i64::from(PAYLOAD_ENUM_SIZE));
    let call = builder.ins().call(allocator, &[size]);
    let ptr = match builder.inst_results(call) {
        [ptr] => *ptr,
        _ => {
            return Err(format!(
                "{backend} enum allocator returned an unexpected result shape in `{}`",
                function.name
            ));
        }
    };
    let null = builder.ins().iconst(types::I64, 0);
    let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
    builder
        .ins()
        .trapnz(is_null, cranelift_codegen::ir::TrapCode::HEAP_OUT_OF_BOUNDS);

    let tag_value = builder.ins().iconst(
        types::I64,
        i64::try_from(tag).expect("enum tag should fit i64"),
    );
    builder.ins().store(MemFlags::new(), tag_value, ptr, 0);
    let payload_repr = if let Some(payload_id) = payload {
        value_repr(values, payload_id, function, "enum payload", backend)?
    } else {
        NativeValueRepr::Unit
    };
    let payload_raw = match payload_repr {
        NativeValueRepr::Native(v) => v,
        NativeValueRepr::Unit => builder.ins().iconst(types::I64, 0),
    };
    builder.ins().store(MemFlags::new(), payload_raw, ptr, 8);
    values.insert(dest, NativeValueRepr::Native(ptr));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn lower_enum_payload(
    builder: &mut FunctionBuilder<'_>,
    values: &mut BTreeMap<ValueId, NativeValueRepr>,
    value_kinds: &BTreeMap<ValueId, NativeValueKind>,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    function: &Function,
    dest: ValueId,
    value: ValueId,
    payload_type: &str,
    backend: &str,
) -> Result<(), String> {
    let Some(NativeValueKind::Enum(enum_name)) = value_kinds.get(&value) else {
        return Err(format!(
            "{backend} enum payload base {} is not an enum in `{}`",
            value.render(),
            function.name
        ));
    };
    let enum_ty = enums
        .get(enum_name)
        .ok_or_else(|| format!("missing native enum metadata for `{enum_name}`"))?;
    if native_enum_is_payload_free(enum_ty) {
        return Err(format!(
            "{backend} cannot extract a payload from payload-free enum `{enum_name}` in `{}`",
            function.name
        ));
    }
    let base = native_value(values, value, function, "enum payload base", backend)?;
    let payload_kind = native_value_kind(payload_type, records, enums)?;
    if matches!(payload_kind, NativeValueKind::Unit) {
        values.insert(dest, NativeValueRepr::Unit);
        return Ok(());
    }
    let native = builder
        .ins()
        .load(native_kind_type(&payload_kind), MemFlags::new(), base, 8);
    values.insert(dest, NativeValueRepr::Native(native));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn lower_enum_tag_eq(
    builder: &mut FunctionBuilder<'_>,
    values: &mut BTreeMap<ValueId, NativeValueRepr>,
    value_kinds: &BTreeMap<ValueId, NativeValueKind>,
    enums: &BTreeMap<String, NativeEnum>,
    function: &Function,
    dest: ValueId,
    value: ValueId,
    tag: i64,
    backend: &str,
) -> Result<(), String> {
    let left = native_value(values, value, function, "enum tag test", backend)?;
    let left = if let Some(NativeValueKind::Enum(enum_name)) = value_kinds.get(&value) {
        let enum_ty = enums
            .get(enum_name)
            .ok_or_else(|| format!("missing native enum metadata for `{enum_name}`"))?;
        if native_enum_is_payload_free(enum_ty) {
            left
        } else {
            builder.ins().load(types::I64, MemFlags::new(), left, 0)
        }
    } else {
        // Payload-free enum matches still lower through raw tag values in MIR.
        left
    };
    let right = builder.ins().iconst(types::I64, tag);
    let native = builder.ins().icmp(IntCC::Equal, left, right);
    let widened = builder.ins().uextend(types::I64, native);
    values.insert(dest, NativeValueRepr::Native(widened));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn lower_insts<M: Module>(
    function_ids: &BTreeMap<String, FuncId>,
    data_ids: &BTreeMap<String, DataId>,
    allocator_id: FuncId,
    text_builder_new_id: FuncId,
    text_builder_append_id: FuncId,
    text_builder_append_codepoint_id: FuncId,
    text_builder_finish_id: FuncId,
    list_new_id: FuncId,
    text_concat_id: FuncId,
    text_slice_id: FuncId,
    text_from_f64_fixed_id: FuncId,
    parse_i32_id: FuncId,
    parse_f64_id: FuncId,
    arg_count_id: FuncId,
    arg_text_id: FuncId,
    stdin_text_id: FuncId,
    stdout_write_id: FuncId,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    value_kinds: &BTreeMap<ValueId, NativeValueKind>,
    module: &mut M,
    function: &Function,
    builder: &mut FunctionBuilder<'_>,
    block_params: &[cranelift_codegen::ir::Value],
    slot_vars: &BTreeMap<crate::LocalSlotId, Variable>,
    slot_types: &BTreeMap<crate::LocalSlotId, types::Type>,
    values: &mut BTreeMap<ValueId, NativeValueRepr>,
    list_headers: &mut BTreeMap<Value, ListHeader>,
    trusted_list_accesses: &TrustedListAccesses,
    instructions: &[Inst],
    backend: &str,
) -> Result<bool, String> {
    for inst in instructions {
        if !lower_inst(
            function_ids,
            data_ids,
            allocator_id,
            text_builder_new_id,
            text_builder_append_id,
            text_builder_append_codepoint_id,
            text_builder_finish_id,
            list_new_id,
            text_concat_id,
            text_slice_id,
            text_from_f64_fixed_id,
            parse_i32_id,
            parse_f64_id,
            arg_count_id,
            arg_text_id,
            stdin_text_id,
            stdout_write_id,
            text_eq_id,
            records,
            enums,
            value_kinds,
            module,
            function,
            builder,
            block_params,
            slot_vars,
            slot_types,
            values,
            list_headers,
            trusted_list_accesses,
            inst,
            backend,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn lower_inst<M: Module>(
    function_ids: &BTreeMap<String, FuncId>,
    data_ids: &BTreeMap<String, DataId>,
    allocator_id: FuncId,
    text_builder_new_id: FuncId,
    text_builder_append_id: FuncId,
    text_builder_append_codepoint_id: FuncId,
    text_builder_finish_id: FuncId,
    list_new_id: FuncId,
    text_concat_id: FuncId,
    text_slice_id: FuncId,
    text_from_f64_fixed_id: FuncId,
    parse_i32_id: FuncId,
    parse_f64_id: FuncId,
    arg_count_id: FuncId,
    arg_text_id: FuncId,
    stdin_text_id: FuncId,
    stdout_write_id: FuncId,
    text_eq_id: FuncId,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
    value_kinds: &BTreeMap<ValueId, NativeValueKind>,
    module: &mut M,
    function: &Function,
    builder: &mut FunctionBuilder<'_>,
    block_params: &[cranelift_codegen::ir::Value],
    slot_vars: &BTreeMap<crate::LocalSlotId, Variable>,
    slot_types: &BTreeMap<crate::LocalSlotId, types::Type>,
    values: &mut BTreeMap<ValueId, NativeValueRepr>,
    list_headers: &mut BTreeMap<Value, ListHeader>,
    trusted_list_accesses: &TrustedListAccesses,
    inst: &Inst,
    backend: &str,
) -> Result<bool, String> {
    match inst {
        Inst::LoadParam { dest, index } => {
            let value = *block_params.get(*index).ok_or_else(|| {
                format!(
                    "{backend} parameter load out of bounds in `{}` at index {}",
                    function.name, index
                )
            })?;
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::LoadLocal { dest, slot } => {
            let var = *slot_vars.get(slot).ok_or_else(|| {
                format!(
                    "{backend} mutable local {} is unavailable in `{}`",
                    slot.render(),
                    function.name
                )
            })?;
            let value = builder.use_var(var);
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::StoreLocal { slot, src } => {
            let var = *slot_vars.get(slot).ok_or_else(|| {
                format!(
                    "{backend} mutable local {} is unavailable in `{}`",
                    slot.render(),
                    function.name
                )
            })?;
            let expected = *slot_types.get(slot).ok_or_else(|| {
                format!(
                    "{backend} mutable local {} is missing a declared native type in `{}`",
                    slot.render(),
                    function.name
                )
            })?;
            let value = native_value(values, *src, function, "mutable store", backend)?;
            let value = coerce_var_value(builder, value, expected, function, backend)?;
            builder.def_var(var, value);
            Ok(true)
        }
        Inst::ConstInt { dest, value } => {
            let native = builder.ins().iconst(types::I64, *value);
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::ConstF64 { dest, bits } => {
            let float = builder.ins().f64const(f64::from_bits(*bits));
            values.insert(*dest, NativeValueRepr::Native(float));
            Ok(true)
        }
        Inst::ConstBool { dest, value } => {
            let native = builder.ins().iconst(types::I64, i64::from(*value));
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::ConstText { dest, value } => {
            let data_id = *data_ids.get(value).ok_or_else(|| {
                format!(
                    "{backend} is missing text data for {:?} in `{}`",
                    value, function.name
                )
            })?;
            let global = module.declare_data_in_func(data_id, builder.func);
            let native = builder.ins().symbol_value(types::I64, global);
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::TextBuilderNew { dest } => {
            let helper = module.declare_func_in_func(text_builder_new_id, builder.func);
            let call = builder.ins().call(helper, &[]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text builder new helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::TextBuilderAppend {
            dest,
            builder: builder_value,
            text,
        } => {
            let builder_val = native_value(
                values,
                *builder_value,
                function,
                "text_builder_append builder",
                backend,
            )?;
            let text_val =
                native_value(values, *text, function, "text_builder_append text", backend)?;
            let helper = module.declare_func_in_func(text_builder_append_id, builder.func);
            let call = builder.ins().call(helper, &[builder_val, text_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text builder append helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::TextBuilderAppendCodepoint {
            dest,
            builder: builder_value,
            codepoint,
        } => {
            let builder_val = native_value(
                values,
                *builder_value,
                function,
                "text_builder_append_codepoint builder",
                backend,
            )?;
            let codepoint_val = native_value(
                values,
                *codepoint,
                function,
                "text_builder_append_codepoint codepoint",
                backend,
            )?;
            let helper =
                module.declare_func_in_func(text_builder_append_codepoint_id, builder.func);
            let call = builder.ins().call(helper, &[builder_val, codepoint_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text builder append codepoint helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::TextBuilderFinish {
            dest,
            builder: builder_value,
        } => {
            let builder_val = native_value(
                values,
                *builder_value,
                function,
                "text_builder_finish builder",
                backend,
            )?;
            let helper = module.declare_func_in_func(text_builder_finish_id, builder.func);
            let call = builder.ins().call(helper, &[builder_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text builder finish helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::ListNew { dest, len, value } => {
            let len_val = native_value(values, *len, function, "list_new len", backend)?;
            let mut value_val = native_value(values, *value, function, "list_new value", backend)?;
            let value_kind = value_kinds
                .get(value)
                .expect("kind inference ensures value kind");
            if *value_kind == NativeValueKind::F64 {
                value_val = builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), value_val);
            }
            let helper = module.declare_func_in_func(list_new_id, builder.func);
            let call = builder.ins().call(helper, &[len_val, value_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} list new helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::ListLen { dest, list } => {
            let vec_val = native_value(values, *list, function, "list_len list", backend)?;
            let header = cached_list_header(
                builder,
                list_headers,
                vec_val,
                function,
                "list_len list",
                backend,
            )?;
            let value = header.len;
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::ListGet { dest, list, index } => {
            let vec_val = native_value(values, *list, function, "list_get list", backend)?;
            let index_val = native_value(values, *index, function, "list_get index", backend)?;
            let header = cached_list_header(
                builder,
                list_headers,
                vec_val,
                function,
                "list_get list",
                backend,
            )?;
            let NativeValueKind::List(element) =
                value_kinds.get(list).expect("kind inference ensures list")
            else {
                return Err(format!(
                    "native list_get input is not a list in `{}`",
                    function.name
                ));
            };
            if !trusted_list_accesses.contains(*list, *index) {
                let too_large =
                    builder
                        .ins()
                        .icmp(IntCC::UnsignedGreaterThanOrEqual, index_val, header.len);
                builder
                    .ins()
                    .trapnz(too_large, TrapCode::HEAP_OUT_OF_BOUNDS);
            }
            let byte_offset = builder.ins().imul_imm(index_val, 8);
            let addr = builder.ins().iadd(header.values_ptr, byte_offset);
            let value = builder
                .ins()
                .load(native_kind_type(element), MemFlags::trusted(), addr, 0);
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::ListSet {
            dest,
            list,
            index,
            value,
        } => {
            let vec_val = native_value(values, *list, function, "list_set list", backend)?;
            let index_val = native_value(values, *index, function, "list_set index", backend)?;
            let value_val = native_value(values, *value, function, "list_set value", backend)?;
            let header = cached_list_header(
                builder,
                list_headers,
                vec_val,
                function,
                "list_set list",
                backend,
            )?;
            if !trusted_list_accesses.contains(*list, *index) {
                let too_large =
                    builder
                        .ins()
                        .icmp(IntCC::UnsignedGreaterThanOrEqual, index_val, header.len);
                builder
                    .ins()
                    .trapnz(too_large, TrapCode::HEAP_OUT_OF_BOUNDS);
            }
            let byte_offset = builder.ins().imul_imm(index_val, 8);
            let addr = builder.ins().iadd(header.values_ptr, byte_offset);
            builder.ins().store(MemFlags::trusted(), value_val, addr, 0);
            values.insert(*dest, NativeValueRepr::Native(vec_val));
            Ok(true)
        }
        Inst::F64FromI32 { dest, value } => {
            let int_val = native_value(values, *value, function, "f64_from_i32 value", backend)?;
            let float = builder.ins().fcvt_from_sint(types::F64, int_val);
            values.insert(*dest, NativeValueRepr::Native(float));
            Ok(true)
        }
        Inst::TextLen { dest, text } => {
            let text_val = native_value(values, *text, function, "text_len", backend)?;
            let len = builder.ins().load(
                types::I64,
                cranelift_codegen::ir::MemFlags::trusted(),
                text_val,
                0,
            );
            values.insert(*dest, NativeValueRepr::Native(len));
            Ok(true)
        }
        Inst::TextConcat { dest, left, right } => {
            let left_val = native_value(values, *left, function, "text_concat left", backend)?;
            let right_val = native_value(values, *right, function, "text_concat right", backend)?;
            let helper = module.declare_func_in_func(text_concat_id, builder.func);
            let call = builder.ins().call(helper, &[left_val, right_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text concat helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::TextSlice {
            dest,
            text,
            start,
            end,
        } => {
            let text_val = native_value(values, *text, function, "text_slice text", backend)?;
            let start_val = native_value(values, *start, function, "text_slice start", backend)?;
            let end_val = native_value(values, *end, function, "text_slice end", backend)?;
            let helper = module.declare_func_in_func(text_slice_id, builder.func);
            let call = builder.ins().call(helper, &[text_val, start_val, end_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text slice helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::TextByte { dest, text, index } => {
            let text_val = native_value(values, *text, function, "text_byte text", backend)?;
            let index_val = native_value(values, *index, function, "text_byte index", backend)?;
            let offset = builder.ins().iadd_imm(index_val, 8);
            let addr = builder.ins().iadd(text_val, offset);
            let byte = builder.ins().load(
                types::I8,
                cranelift_codegen::ir::MemFlags::trusted(),
                addr,
                0,
            );
            let byte_i64 = builder.ins().uextend(types::I64, byte);
            values.insert(*dest, NativeValueRepr::Native(byte_i64));
            Ok(true)
        }
        Inst::TextFromF64Fixed {
            dest,
            value,
            digits,
        } => {
            let value_val = native_value(
                values,
                *value,
                function,
                "text_from_f64_fixed value",
                backend,
            )?;
            let digits_val = native_value(
                values,
                *digits,
                function,
                "text_from_f64_fixed digits",
                backend,
            )?;
            let helper = module.declare_func_in_func(text_from_f64_fixed_id, builder.func);
            let call = builder.ins().call(helper, &[value_val, digits_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} text_from_f64_fixed helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::ParseI32 { dest, text } => {
            let text_val = native_value(values, *text, function, "parse_i32 text", backend)?;
            let helper = module.declare_func_in_func(parse_i32_id, builder.func);
            let call = builder.ins().call(helper, &[text_val]);
            let value = match builder.inst_results(call) {
                [value] => *value,
                _ => {
                    return Err(format!(
                        "{backend} parse_i32 helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::ParseF64 { dest, text } => {
            let text_val = native_value(values, *text, function, "parse_f64 text", backend)?;
            let helper = module.declare_func_in_func(parse_f64_id, builder.func);
            let call = builder.ins().call(helper, &[text_val]);
            let value = match builder.inst_results(call) {
                [value] => *value,
                _ => {
                    return Err(format!(
                        "{backend} parse_f64 helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::ArgCount { dest } => {
            let helper = module.declare_func_in_func(arg_count_id, builder.func);
            let call = builder.ins().call(helper, &[]);
            let value = match builder.inst_results(call) {
                [value] => *value,
                _ => {
                    return Err(format!(
                        "{backend} arg count helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            values.insert(*dest, NativeValueRepr::Native(value));
            Ok(true)
        }
        Inst::ArgText { dest, index } => {
            let index_val = native_value(values, *index, function, "arg_text index", backend)?;
            let helper = module.declare_func_in_func(arg_text_id, builder.func);
            let call = builder.ins().call(helper, &[index_val]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} arg text helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::StdinText { dest } => {
            let helper = module.declare_func_in_func(stdin_text_id, builder.func);
            let call = builder.ins().call(helper, &[]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} stdin text helper returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::StdoutWrite { text } => {
            let text_val = native_value(values, *text, function, "stdout_write text", backend)?;
            let helper = module.declare_func_in_func(stdout_write_id, builder.func);
            let _call = builder.ins().call(helper, &[text_val]);
            Ok(true)
        }
        Inst::MakeEnum {
            dest,
            name,
            variant,
            payload,
        } => lower_make_enum(
            module,
            builder,
            allocator_id,
            values,
            enums,
            function,
            *dest,
            name,
            variant,
            *payload,
            backend,
        )
        .map(|()| true),
        Inst::EnumPayload {
            dest,
            value,
            payload_type,
        } => lower_enum_payload(
            builder,
            values,
            value_kinds,
            records,
            enums,
            function,
            *dest,
            *value,
            payload_type,
            backend,
        )
        .map(|()| true),
        Inst::EnumTagEq { dest, value, tag } => lower_enum_tag_eq(
            builder,
            values,
            value_kinds,
            enums,
            function,
            *dest,
            *value,
            *tag,
            backend,
        )
        .map(|()| true),
        Inst::MakeRecord { dest, name, fields } => {
            let record = records
                .get(name)
                .ok_or_else(|| format!("missing native record metadata for `{name}`"))?;
            let allocator = module.declare_func_in_func(allocator_id, builder.func);
            let size = builder.ins().iconst(types::I64, i64::from(record.size));
            let call = builder.ins().call(allocator, &[size]);
            let ptr = match builder.inst_results(call) {
                [ptr] => *ptr,
                _ => {
                    return Err(format!(
                        "{backend} record allocator returned an unexpected result shape in `{}`",
                        function.name
                    ));
                }
            };
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
            builder.ins().trapnz(is_null, TrapCode::HEAP_OUT_OF_BOUNDS);
            for field in &record.fields {
                let source = fields
                    .iter()
                    .find_map(|(field_name, value)| (field_name == &field.name).then_some(*value))
                    .ok_or_else(|| {
                        format!(
                            "record instruction is missing field `{}` in `{}`",
                            field.name, function.name
                        )
                    })?;
                builder.ins().store(
                    MemFlags::new(),
                    native_value(values, source, function, "record field", backend)?,
                    ptr,
                    i32::try_from(field.offset)
                        .map_err(|_| format!("{backend} record offset exceeds backend limits"))?,
                );
            }
            values.insert(*dest, NativeValueRepr::Native(ptr));
            Ok(true)
        }
        Inst::Field { dest, base, name } => {
            let Some(NativeValueKind::Record(record_name)) = value_kinds.get(base) else {
                return Err(format!(
                    "{backend} field base {} is not a record in `{}`",
                    base.render(),
                    function.name
                ));
            };
            let record = records
                .get(record_name)
                .ok_or_else(|| format!("missing native record metadata for `{record_name}`"))?;
            let field = record
                .fields
                .iter()
                .find(|field| field.name == *name)
                .ok_or_else(|| {
                    format!(
                        "record `{record_name}` has no native field `{name}` in `{}`",
                        function.name
                    )
                })?;
            let base = native_value(values, *base, function, "field base", backend)?;
            let native = builder.ins().load(
                native_kind_type(&field.kind),
                MemFlags::new(),
                base,
                i32::try_from(field.offset)
                    .map_err(|_| format!("{backend} record offset exceeds backend limits"))?,
            );
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::Add { dest, left, right } => {
            let left_kind = value_kinds.get(left).ok_or_else(|| {
                format!(
                    "{backend} could not resolve add left operand kind for `{}`",
                    function.name
                )
            })?;
            let left_value = native_value(values, *left, function, "add left operand", backend)?;
            let right_value = native_value(values, *right, function, "add right operand", backend)?;
            let native = match left_kind {
                NativeValueKind::F64 => {
                    let left_float = left_value;
                    let right_float = right_value;
                    builder.ins().fadd(left_float, right_float)
                }
                _ => builder.ins().iadd(left_value, right_value),
            };
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::Sub { dest, left, right } => {
            let left_kind = value_kinds.get(left).ok_or_else(|| {
                format!(
                    "{backend} could not resolve sub left operand kind for `{}`",
                    function.name
                )
            })?;
            let left_value = native_value(values, *left, function, "sub left operand", backend)?;
            let right_value = native_value(values, *right, function, "sub right operand", backend)?;
            let native = match left_kind {
                NativeValueKind::F64 => {
                    let left_float = left_value;
                    let right_float = right_value;
                    builder.ins().fsub(left_float, right_float)
                }
                _ => builder.ins().isub(left_value, right_value),
            };
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::Mul { dest, left, right } => {
            let left_kind = value_kinds.get(left).ok_or_else(|| {
                format!(
                    "{backend} could not resolve mul left operand kind for `{}`",
                    function.name
                )
            })?;
            let left_value = native_value(values, *left, function, "mul left operand", backend)?;
            let right_value = native_value(values, *right, function, "mul right operand", backend)?;
            let native = match left_kind {
                NativeValueKind::F64 => {
                    let left_float = left_value;
                    let right_float = right_value;
                    builder.ins().fmul(left_float, right_float)
                }
                _ => builder.ins().imul(left_value, right_value),
            };
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::Div { dest, left, right } => {
            let left_kind = value_kinds.get(left).ok_or_else(|| {
                format!(
                    "{backend} could not resolve div left operand kind for `{}`",
                    function.name
                )
            })?;
            let left_value = native_value(values, *left, function, "div left operand", backend)?;
            let right_value = native_value(values, *right, function, "div right operand", backend)?;
            let native = match left_kind {
                NativeValueKind::F64 => {
                    let left_float = left_value;
                    let right_float = right_value;
                    builder.ins().fdiv(left_float, right_float)
                }
                _ => builder.ins().sdiv(left_value, right_value),
            };
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::And { dest, left, right } => {
            let native = builder.ins().band(
                native_value(values, *left, function, "and left operand", backend)?,
                native_value(values, *right, function, "and right operand", backend)?,
            );
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::Or { dest, left, right } => {
            let native = builder.ins().bor(
                native_value(values, *left, function, "or left operand", backend)?,
                native_value(values, *right, function, "or right operand", backend)?,
            );
            values.insert(*dest, NativeValueRepr::Native(native));
            Ok(true)
        }
        Inst::Eq { dest, left, right } => {
            let native = lower_comparison(
                module,
                builder,
                values,
                value_kinds,
                text_eq_id,
                records,
                enums,
                *left,
                *right,
                function,
                IntCC::Equal,
                backend,
            )?;
            values.insert(*dest, native);
            Ok(true)
        }
        Inst::Ne { dest, left, right } => {
            let native = lower_comparison(
                module,
                builder,
                values,
                value_kinds,
                text_eq_id,
                records,
                enums,
                *left,
                *right,
                function,
                IntCC::NotEqual,
                backend,
            )?;
            values.insert(*dest, native);
            Ok(true)
        }
        Inst::Lt { dest, left, right } => {
            let native = lower_comparison(
                module,
                builder,
                values,
                value_kinds,
                text_eq_id,
                records,
                enums,
                *left,
                *right,
                function,
                IntCC::SignedLessThan,
                backend,
            )?;
            values.insert(*dest, native);
            Ok(true)
        }
        Inst::Le { dest, left, right } => {
            let native = lower_comparison(
                module,
                builder,
                values,
                value_kinds,
                text_eq_id,
                records,
                enums,
                *left,
                *right,
                function,
                IntCC::SignedLessThanOrEqual,
                backend,
            )?;
            values.insert(*dest, native);
            Ok(true)
        }
        Inst::Gt { dest, left, right } => {
            let native = lower_comparison(
                module,
                builder,
                values,
                value_kinds,
                text_eq_id,
                records,
                enums,
                *left,
                *right,
                function,
                IntCC::SignedGreaterThan,
                backend,
            )?;
            values.insert(*dest, native);
            Ok(true)
        }
        Inst::Ge { dest, left, right } => {
            let native = lower_comparison(
                module,
                builder,
                values,
                value_kinds,
                text_eq_id,
                records,
                enums,
                *left,
                *right,
                function,
                IntCC::SignedGreaterThanOrEqual,
                backend,
            )?;
            values.insert(*dest, native);
            Ok(true)
        }
        Inst::Sqrt { dest, value } => {
            let float = native_value(values, *value, function, "sqrt operand", backend)?;
            let sqrt = builder.ins().sqrt(float);
            values.insert(*dest, NativeValueRepr::Native(sqrt));
            Ok(true)
        }
        Inst::Call { dest, callee, args } => {
            let id = *function_ids.get(callee).ok_or_else(|| {
                format!(
                    "{backend} lowering could not find callee `{callee}` in `{}`",
                    function.name
                )
            })?;
            let local = module.declare_func_in_func(id, builder.func);
            let native_args = args
                .iter()
                .map(|value| native_value(values, *value, function, "call argument", backend))
                .collect::<Result<Vec<_>, String>>()?;
            let call = builder.ins().call(local, &native_args);
            match builder.inst_results(call) {
                [] => {
                    values.insert(*dest, NativeValueRepr::Unit);
                }
                [result] => {
                    values.insert(*dest, NativeValueRepr::Native(*result));
                }
                _ => {
                    return Err(format!(
                        "{backend} does not support multi-value returns in `{}`",
                        function.name
                    ));
                }
            }
            Ok(true)
        }
        Inst::If {
            dest,
            condition,
            then_insts,
            then_result,
            else_insts,
            else_result,
        } => {
            let condition = native_value(values, *condition, function, "if condition", backend)?;
            let zero = builder.ins().iconst(types::I64, 0);
            let condition = builder.ins().icmp(IntCC::NotEqual, condition, zero);
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();
            let dest_type = match value_kinds.get(dest) {
                Some(kind) => {
                    let ty = native_kind_type(kind);
                    builder.append_block_param(merge_block, ty);
                    Some(ty)
                }
                None => None,
            };
            builder
                .ins()
                .brif(condition, then_block, &[], else_block, &[]);
            builder.seal_block(then_block);
            builder.seal_block(else_block);

            let mut then_values = values.clone();
            let mut then_headers = list_headers.clone();
            builder.switch_to_block(then_block);
            let then_falls = lower_insts(
                function_ids,
                data_ids,
                allocator_id,
                text_builder_new_id,
                text_builder_append_id,
                text_builder_append_codepoint_id,
                text_builder_finish_id,
                list_new_id,
                text_concat_id,
                text_slice_id,
                text_from_f64_fixed_id,
                parse_i32_id,
                parse_f64_id,
                arg_count_id,
                arg_text_id,
                stdin_text_id,
                stdout_write_id,
                text_eq_id,
                records,
                enums,
                value_kinds,
                module,
                function,
                builder,
                block_params,
                slot_vars,
                slot_types,
                &mut then_values,
                &mut then_headers,
                trusted_list_accesses,
                then_insts,
                backend,
            )?;
            if then_falls {
                let then_args = branch_jump_args(
                    &then_values,
                    *then_result,
                    dest_type,
                    function,
                    "then",
                    backend,
                )?;
                builder.ins().jump(merge_block, &then_args);
            }

            let mut else_values = values.clone();
            let mut else_headers = list_headers.clone();
            builder.switch_to_block(else_block);
            let else_falls = lower_insts(
                function_ids,
                data_ids,
                allocator_id,
                text_builder_new_id,
                text_builder_append_id,
                text_builder_append_codepoint_id,
                text_builder_finish_id,
                list_new_id,
                text_concat_id,
                text_slice_id,
                text_from_f64_fixed_id,
                parse_i32_id,
                parse_f64_id,
                arg_count_id,
                arg_text_id,
                stdin_text_id,
                stdout_write_id,
                text_eq_id,
                records,
                enums,
                value_kinds,
                module,
                function,
                builder,
                block_params,
                slot_vars,
                slot_types,
                &mut else_values,
                &mut else_headers,
                trusted_list_accesses,
                else_insts,
                backend,
            )?;
            if else_falls {
                let else_args = branch_jump_args(
                    &else_values,
                    *else_result,
                    dest_type,
                    function,
                    "else",
                    backend,
                )?;
                builder.ins().jump(merge_block, &else_args);
            }

            if !(then_falls || else_falls) {
                return Ok(false);
            }

            builder.seal_block(merge_block);
            builder.switch_to_block(merge_block);
            let repr = match builder.block_params(merge_block) {
                [] => NativeValueRepr::Unit,
                [value] => NativeValueRepr::Native(*value),
                _ => {
                    return Err(format!(
                        "{backend} does not support multi-value conditional merges in `{}`",
                        function.name
                    ));
                }
            };
            values.insert(*dest, repr);
            Ok(true)
        }
        Inst::Repeat {
            dest,
            count,
            index_slot,
            body_insts,
        } => {
            let initial_count = native_value(values, *count, function, "repeat count", backend)?;
            let loop_trusted_accesses = index_slot
                .map_or_else(TrustedListAccesses::default, |slot| {
                    collect_trusted_repeat_list_accesses(body_insts, slot)
                });
            if !loop_trusted_accesses.pairs.is_empty() {
                let zero = builder.ins().iconst(types::I64, 0);
                let has_positive_count =
                    builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThan, initial_count, zero);
                let effective_count = builder
                    .ins()
                    .select(has_positive_count, initial_count, zero);
                for vec in loop_trusted_accesses.unique_vecs() {
                    let vec_val =
                        native_value(values, vec, function, "repeat trusted f64 vec", backend)?;
                    let header = cached_list_header(
                        builder,
                        list_headers,
                        vec_val,
                        function,
                        "repeat trusted f64 vec",
                        backend,
                    )?;
                    let count_exceeds_len =
                        builder
                            .ins()
                            .icmp(IntCC::UnsignedGreaterThan, effective_count, header.len);
                    builder
                        .ins()
                        .trapnz(count_exceeds_len, TrapCode::HEAP_OUT_OF_BOUNDS);
                }
            }
            let header_block = builder.create_block();
            let body_block = builder.create_block();
            let exit_block = builder.create_block();
            builder.append_block_param(header_block, types::I64);
            builder.append_block_param(header_block, types::I64);
            let zero = builder.ins().iconst(types::I64, 0);
            let entry_args = [
                cranelift_codegen::ir::BlockArg::Value(initial_count),
                cranelift_codegen::ir::BlockArg::Value(zero),
            ];
            builder.ins().jump(header_block, &entry_args);

            builder.switch_to_block(header_block);
            let remaining = builder.block_params(header_block)[0];
            let current_index = builder.block_params(header_block)[1];
            let has_more = builder
                .ins()
                .icmp(IntCC::SignedGreaterThan, remaining, zero);
            builder
                .ins()
                .brif(has_more, body_block, &[], exit_block, &[]);
            builder.seal_block(body_block);
            builder.seal_block(exit_block);

            let mut body_values = values.clone();
            let mut body_headers = list_headers.clone();
            builder.switch_to_block(body_block);
            if let Some(slot) = index_slot {
                let var = slot_vars.get(slot).copied().ok_or_else(|| {
                    format!(
                        "{backend} lowering is missing loop index slot {} in `{}`",
                        slot.render(),
                        function.name
                    )
                })?;
                let expected = *slot_types.get(slot).ok_or_else(|| {
                    format!(
                        "{backend} loop index slot {} is missing a declared native type in `{}`",
                        slot.render(),
                        function.name
                    )
                })?;
                let current_index = coerce_var_value(builder, current_index, expected, function, backend)?;
                builder.def_var(var, current_index);
            }
            let body_falls = lower_insts(
                function_ids,
                data_ids,
                allocator_id,
                text_builder_new_id,
                text_builder_append_id,
                text_builder_append_codepoint_id,
                text_builder_finish_id,
                list_new_id,
                text_concat_id,
                text_slice_id,
                text_from_f64_fixed_id,
                parse_i32_id,
                parse_f64_id,
                arg_count_id,
                arg_text_id,
                stdin_text_id,
                stdout_write_id,
                text_eq_id,
                records,
                enums,
                value_kinds,
                module,
                function,
                builder,
                block_params,
                slot_vars,
                slot_types,
                &mut body_values,
                &mut body_headers,
                &loop_trusted_accesses,
                body_insts,
                backend,
            )?;
            if body_falls {
                let one = builder.ins().iconst(types::I64, 1);
                let next = builder.ins().isub(remaining, one);
                let next_index = builder.ins().iadd(current_index, one);
                let backedge_args = [
                    cranelift_codegen::ir::BlockArg::Value(next),
                    cranelift_codegen::ir::BlockArg::Value(next_index),
                ];
                builder.ins().jump(header_block, &backedge_args);
            }

            builder.seal_block(header_block);
            builder.switch_to_block(exit_block);
            values.insert(*dest, NativeValueRepr::Unit);
            Ok(true)
        }
        Inst::While {
            dest,
            condition_insts,
            condition,
            body_insts,
        } => {
            let condition_block = builder.create_block();
            let body_block = builder.create_block();
            let exit_block = builder.create_block();
            builder.ins().jump(condition_block, &[]);

            builder.switch_to_block(condition_block);
            let mut condition_values = values.clone();
            let mut condition_headers = list_headers.clone();
            let condition_falls = lower_insts(
                function_ids,
                data_ids,
                allocator_id,
                text_builder_new_id,
                text_builder_append_id,
                text_builder_append_codepoint_id,
                text_builder_finish_id,
                list_new_id,
                text_concat_id,
                text_slice_id,
                text_from_f64_fixed_id,
                parse_i32_id,
                parse_f64_id,
                arg_count_id,
                arg_text_id,
                stdin_text_id,
                stdout_write_id,
                text_eq_id,
                records,
                enums,
                value_kinds,
                module,
                function,
                builder,
                block_params,
                slot_vars,
                slot_types,
                &mut condition_values,
                &mut condition_headers,
                trusted_list_accesses,
                condition_insts,
                backend,
            )?;
            if !condition_falls {
                builder.seal_block(condition_block);
                values.insert(*dest, NativeValueRepr::Unit);
                return Ok(false);
            }
            let condition_value = native_value(
                &condition_values,
                *condition,
                function,
                "while condition",
                backend,
            )?;
            builder
                .ins()
                .brif(condition_value, body_block, &[], exit_block, &[]);
            builder.seal_block(body_block);
            builder.seal_block(exit_block);

            let mut body_values = values.clone();
            let mut body_headers = list_headers.clone();
            builder.switch_to_block(body_block);
            let body_falls = lower_insts(
                function_ids,
                data_ids,
                allocator_id,
                text_builder_new_id,
                text_builder_append_id,
                text_builder_append_codepoint_id,
                text_builder_finish_id,
                list_new_id,
                text_concat_id,
                text_slice_id,
                text_from_f64_fixed_id,
                parse_i32_id,
                parse_f64_id,
                arg_count_id,
                arg_text_id,
                stdin_text_id,
                stdout_write_id,
                text_eq_id,
                records,
                enums,
                value_kinds,
                module,
                function,
                builder,
                block_params,
                slot_vars,
                slot_types,
                &mut body_values,
                &mut body_headers,
                trusted_list_accesses,
                body_insts,
                backend,
            )?;
            if body_falls {
                builder.ins().jump(condition_block, &[]);
            }

            builder.seal_block(condition_block);
            builder.switch_to_block(exit_block);
            values.insert(*dest, NativeValueRepr::Unit);
            Ok(true)
        }
        Inst::Assert { condition, kind } => {
            let condition =
                native_value(values, *condition, function, "contract condition", backend)?;
            builder
                .ins()
                .trapz(condition, TrapCode::unwrap_user(contract_trap_code(*kind)));
            Ok(true)
        }
        Inst::Perform { .. } | Inst::Handle { .. } => Err(format!(
            "{backend} does not yet support effect handlers in `{}`",
            function.name
        )),
    }
}

pub fn branch_jump_args(
    values: &BTreeMap<ValueId, NativeValueRepr>,
    result: Option<ValueId>,
    dest_type: Option<types::Type>,
    function: &Function,
    branch_name: &str,
    backend: &str,
) -> Result<Vec<BlockArg>, String> {
    match (dest_type, result) {
        (None, None) => Ok(Vec::new()),
        (Some(_), Some(result)) => Ok(vec![BlockArg::Value(native_value(
            values,
            result,
            function,
            &format!("{branch_name} branch result"),
            backend,
        )?)]),
        (Some(_), None) | (None, Some(_)) => Err(format!(
            "{backend} conditional branches in `{}` do not agree on whether they produce a value",
            function.name
        )),
    }
}

fn collect_trusted_repeat_list_accesses(
    instructions: &[Inst],
    index_slot: crate::LocalSlotId,
) -> TrustedListAccesses {
    let mut body_defined_values = BTreeSet::new();
    collect_defined_values(instructions, &mut body_defined_values);
    let mut loop_index_values = BTreeSet::new();
    collect_loop_index_values(instructions, index_slot, &mut loop_index_values);
    let mut pairs = BTreeSet::new();
    collect_trusted_list_pairs(
        instructions,
        &body_defined_values,
        &loop_index_values,
        &mut pairs,
    );
    TrustedListAccesses { pairs }
}

fn collect_defined_values(instructions: &[Inst], defined: &mut BTreeSet<ValueId>) {
    for inst in instructions {
        match inst {
            Inst::TextBuilderNew { dest }
            | Inst::TextBuilderFinish { dest, .. }
            | Inst::ListNew { dest, .. }
            | Inst::ListLen { dest, .. }
            | Inst::ListGet { dest, .. }
            | Inst::F64FromI32 { dest, .. }
            | Inst::TextLen { dest, .. }
            | Inst::TextConcat { dest, .. }
            | Inst::TextSlice { dest, .. }
            | Inst::TextByte { dest, .. }
            | Inst::TextFromF64Fixed { dest, .. }
            | Inst::ArgCount { dest }
            | Inst::ArgText { dest, .. }
            | Inst::StdinText { dest }
            | Inst::ParseI32 { dest, .. }
            | Inst::ParseF64 { dest, .. }
            | Inst::LoadParam { dest, .. }
            | Inst::LoadLocal { dest, .. }
            | Inst::ConstInt { dest, .. }
            | Inst::ConstF64 { dest, .. }
            | Inst::ConstBool { dest, .. }
            | Inst::ConstText { dest, .. }
            | Inst::MakeEnum { dest, .. }
            | Inst::MakeRecord { dest, .. }
            | Inst::Field { dest, .. }
            | Inst::EnumTagEq { dest, .. }
            | Inst::EnumPayload { dest, .. }
            | Inst::If { dest, .. }
            | Inst::While { dest, .. }
            | Inst::Repeat { dest, .. }
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
            | Inst::Call { dest, .. } => {
                defined.insert(*dest);
            }
            Inst::TextBuilderAppend { dest, .. }
            | Inst::TextBuilderAppendCodepoint { dest, .. }
            | Inst::ListSet { dest, .. } => {
                defined.insert(*dest);
            }
            Inst::Perform { dest, .. } | Inst::Handle { dest, .. } => {
                defined.insert(*dest);
            }
            Inst::StoreLocal { .. } | Inst::StdoutWrite { .. } | Inst::Assert { .. } => {}
        }
        match inst {
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                collect_defined_values(then_insts, defined);
                collect_defined_values(else_insts, defined);
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                collect_defined_values(condition_insts, defined);
                collect_defined_values(body_insts, defined);
            }
            Inst::Repeat { body_insts, .. } => collect_defined_values(body_insts, defined),
            _ => {}
        }
    }
}

fn collect_loop_index_values(
    instructions: &[Inst],
    index_slot: crate::LocalSlotId,
    loop_index_values: &mut BTreeSet<ValueId>,
) {
    for inst in instructions {
        match inst {
            Inst::LoadLocal { dest, slot } if *slot == index_slot => {
                loop_index_values.insert(*dest);
            }
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                collect_loop_index_values(then_insts, index_slot, loop_index_values);
                collect_loop_index_values(else_insts, index_slot, loop_index_values);
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                collect_loop_index_values(condition_insts, index_slot, loop_index_values);
                collect_loop_index_values(body_insts, index_slot, loop_index_values);
            }
            Inst::Perform { .. } | Inst::Handle { .. } => {}
            _ => {}
        }
    }
}

fn collect_trusted_list_pairs(
    instructions: &[Inst],
    body_defined_values: &BTreeSet<ValueId>,
    loop_index_values: &BTreeSet<ValueId>,
    pairs: &mut BTreeSet<(ValueId, ValueId)>,
) {
    for inst in instructions {
        match inst {
            Inst::ListGet { list, index, .. } | Inst::ListSet { list, index, .. }
                if loop_index_values.contains(index) && !body_defined_values.contains(list) =>
            {
                pairs.insert((*list, *index));
            }
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                collect_trusted_list_pairs(
                    then_insts,
                    body_defined_values,
                    loop_index_values,
                    pairs,
                );
                collect_trusted_list_pairs(
                    else_insts,
                    body_defined_values,
                    loop_index_values,
                    pairs,
                );
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                collect_trusted_list_pairs(
                    condition_insts,
                    body_defined_values,
                    loop_index_values,
                    pairs,
                );
                collect_trusted_list_pairs(
                    body_insts,
                    body_defined_values,
                    loop_index_values,
                    pairs,
                );
            }
            Inst::Perform { .. } | Inst::Handle { .. } => {}
            _ => {}
        }
    }
}

fn cached_list_header(
    builder: &mut FunctionBuilder<'_>,
    headers: &mut BTreeMap<Value, ListHeader>,
    list: Value,
    function: &Function,
    context: &str,
    backend: &str,
) -> Result<ListHeader, String> {
    if let Some(header) = headers.get(&list).copied() {
        return Ok(header);
    }
    let _ = (function, context, backend);
    let len = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), list, LIST_LEN_OFFSET);
    let values_ptr = builder
        .ins()
        .load(types::I64, MemFlags::trusted(), list, LIST_VALUES_OFFSET);
    let header = ListHeader { len, values_ptr };
    headers.insert(list, header);
    Ok(header)
}

pub fn native_value(
    values: &BTreeMap<ValueId, NativeValueRepr>,
    value: ValueId,
    function: &Function,
    context: &str,
    backend: &str,
) -> Result<cranelift_codegen::ir::Value, String> {
    match value_repr(values, value, function, context, backend)? {
        NativeValueRepr::Native(value) => Ok(value),
        NativeValueRepr::Unit => Err(format!(
            "{backend} expected a native value for {context} in `{}`",
            function.name
        )),
    }
}

pub fn value_repr(
    values: &BTreeMap<ValueId, NativeValueRepr>,
    value: ValueId,
    function: &Function,
    context: &str,
    backend: &str,
) -> Result<NativeValueRepr, String> {
    values.get(&value).copied().ok_or_else(|| {
        format!(
            "{backend} could not resolve {} {} in `{}`",
            context,
            value.render(),
            function.name
        )
    })
}

fn branch_result_kind(
    kinds: &BTreeMap<ValueId, NativeValueKind>,
    result: Option<ValueId>,
    function: &Function,
    branch_name: &str,
) -> Result<Option<NativeValueKind>, String> {
    result.map_or(Ok(None), |result| {
        kinds.get(&result).cloned().map(Some).ok_or_else(|| {
            format!(
                "native {branch_name} branch result {} is unavailable in `{}`",
                result.render(),
                function.name
            )
        })
    })
}

pub fn native_type(
    name: &str,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) -> Result<types::Type, String> {
    match name {
        "Unit" => Ok(types::INVALID),
        other => {
            let kind = native_value_kind(other, records, enums)?;
            Ok(native_kind_type(&kind))
        }
    }
}

pub fn declare_record_allocator<M: Module>(
    module: &mut M,
    backend: &str,
) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_record_alloc", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} record allocator: {error}"))
}

pub fn declare_text_concat<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_concat", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} text concat helper: {error}"))
}

pub fn declare_list_new<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_list_new", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} list new helper: {error}"))
}

pub fn declare_text_builder_new<M: Module>(
    module: &mut M,
    backend: &str,
) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_builder_new", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} text builder new helper: {error}"))
}

pub fn declare_text_builder_append<M: Module>(
    module: &mut M,
    backend: &str,
) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_builder_append", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} text builder append helper: {error}"))
}

pub fn declare_text_builder_append_codepoint<M: Module>(
    module: &mut M,
    backend: &str,
) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function(
            "sarif_text_builder_append_codepoint",
            Linkage::Import,
            &signature,
        )
        .map_err(|error| {
            format!("failed to declare {backend} text builder append codepoint helper: {error}")
        })
}

pub fn declare_text_builder_finish<M: Module>(
    module: &mut M,
    backend: &str,
) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_builder_finish", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} text builder finish helper: {error}"))
}

pub fn declare_text_slice<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_slice", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} text slice helper: {error}"))
}

pub fn declare_text_from_f64_fixed<M: Module>(
    module: &mut M,
    backend: &str,
) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::F64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_from_f64_fixed", Linkage::Import, &signature)
        .map_err(|error| {
            format!("failed to declare {backend} fixed-decimal float text helper: {error}")
        })
}

pub fn declare_parse_i32<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_parse_i32", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} parse_i32 helper: {error}"))
}

pub fn declare_parse_f64<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::F64));
    module
        .declare_function("sarif_parse_f64", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} parse_f64 helper: {error}"))
}

pub fn declare_text_eq<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_text_eq", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} text equality helper: {error}"))
}

pub fn declare_arg_count<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_arg_count", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} arg count helper: {error}"))
}

pub fn declare_arg_text<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_arg_text", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} arg text helper: {error}"))
}

pub fn declare_stdin_text<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.returns.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_stdin_text", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} stdin text helper: {error}"))
}

pub fn declare_stdout_write<M: Module>(module: &mut M, backend: &str) -> Result<FuncId, String> {
    let mut signature = module.make_signature();
    signature.call_conv = CallConv::triple_default(module.isa().triple());
    signature.params.push(AbiParam::new(types::I64));
    module
        .declare_function("sarif_stdout_write", Linkage::Import, &signature)
        .map_err(|error| format!("failed to declare {backend} stdout write helper: {error}"))
}

pub fn declare_text_data_for_insts<M: Module>(
    module: &mut M,
    data_ids: &mut BTreeMap<String, DataId>,
    instructions: &[Inst],
    prefix: &str,
    next_index: &mut usize,
    backend: &str,
) -> Result<(), String> {
    for inst in instructions {
        match inst {
            Inst::ConstText { value, .. } => {
                if data_ids.contains_key(value) {
                    continue;
                }
                let name = format!("{prefix}_{}", *next_index);
                *next_index += 1;
                let id = module
                    .declare_data(&name, Linkage::Local, false, false)
                    .map_err(|error| {
                        format!("failed to declare {backend} text object `{name}`: {error}")
                    })?;
                data_ids.insert(value.clone(), id);
            }
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                declare_text_data_for_insts(
                    module, data_ids, then_insts, prefix, next_index, backend,
                )?;
                declare_text_data_for_insts(
                    module, data_ids, else_insts, prefix, next_index, backend,
                )?;
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                declare_text_data_for_insts(
                    module,
                    data_ids,
                    condition_insts,
                    prefix,
                    next_index,
                    backend,
                )?;
                declare_text_data_for_insts(
                    module, data_ids, body_insts, prefix, next_index, backend,
                )?;
            }
            Inst::Repeat { body_insts, .. } => {
                declare_text_data_for_insts(
                    module, data_ids, body_insts, prefix, next_index, backend,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn encode_text_blob(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of::<u64>() + value.len());
    let len = u64::try_from(value.len()).expect("stage-0 text length must fit in u64");
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    bytes
}

pub const fn contract_trap_code(kind: crate::ContractKind) -> u8 {
    match kind {
        crate::ContractKind::Requires => 1,
        crate::ContractKind::Ensures => 2,
        crate::ContractKind::Bounds => 3,
    }
}
