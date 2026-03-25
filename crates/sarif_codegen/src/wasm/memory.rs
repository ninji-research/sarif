use std::collections::BTreeMap;

use wasmtime::{Memory, Store};

use super::{WasmEnum, WasmError, WasmRecord, WasmValueKind, enum_is_payload_free};
use crate::{RuntimeRecord, RuntimeValue};

const PAYLOAD_ENUM_SIZE: usize = 16;

fn enum_variant_index(enum_ty: &WasmEnum, variant_name: &str) -> Option<usize> {
    enum_ty.variants.iter().position(|v| v.name == variant_name)
}

pub(super) fn encode_enum_tag_wasm(
    value: &crate::RuntimeEnum,
    enums: &BTreeMap<String, WasmEnum>,
) -> Result<i64, WasmError> {
    let enum_ty = enums
        .get(&value.name)
        .ok_or_else(|| WasmError::new(format!("unknown enum type `{}`", value.name)))?;
    let index = enum_variant_index(enum_ty, &value.variant).ok_or_else(|| {
        WasmError::new(format!(
            "enum `{}` has no variant `{}`",
            value.name, value.variant
        ))
    })?;
    i64::try_from(index)
        .map_err(|_| WasmError::new(format!("enum `{}` exceeds stage-0 limits", value.name)))
}

pub(super) fn decode_payload_free_enum_tag(
    tag: i64,
    name: &str,
    enums: &BTreeMap<String, WasmEnum>,
) -> Result<RuntimeValue, WasmError> {
    let enum_ty = enums
        .get(name)
        .ok_or_else(|| WasmError::new(format!("unknown enum type `{name}`")))?;
    let index = usize::try_from(tag)
        .map_err(|_| WasmError::new(format!("enum `{name}` tag `{tag}` is out of range")))?;
    let variant = enum_ty
        .variants
        .get(index)
        .ok_or_else(|| WasmError::new(format!("enum `{name}` tag `{tag}` is out of range")))?;
    Ok(RuntimeValue::Enum(crate::RuntimeEnum {
        name: name.to_owned(),
        variant: variant.name.clone(),
        payload: None,
    }))
}

pub(super) fn pack_text_value(offset: u32, len: u32) -> i64 {
    (i64::from(len) << 32) | i64::from(offset)
}

pub(super) fn unpack_text_value(packed: i64) -> Result<(usize, usize), WasmError> {
    let raw = u64::try_from(packed)
        .map_err(|_| WasmError::new("wasm text value must not be negative"))?;
    let ptr = usize::try_from(raw & u64::from(u32::MAX))
        .map_err(|_| WasmError::new("wasm text pointer exceeds host limits"))?;
    let len = usize::try_from(raw >> 32)
        .map_err(|_| WasmError::new("wasm text length exceeds host limits"))?;
    Ok((ptr, len))
}

pub(super) fn record_offset(index: usize) -> Result<u32, WasmError> {
    let index = u32::try_from(index)
        .map_err(|_| WasmError::new("record field index exceeds 32-bit limits"))?;
    index
        .checked_mul(8)
        .ok_or_else(|| WasmError::new("record offset exceeds 32-bit limits"))
}

pub(super) fn record_size(record: &WasmRecord) -> u32 {
    u32::try_from(record.fields.len())
        .expect("record field count should fit in u32")
        .saturating_mul(8)
}

pub(super) fn runtime_value_to_wasm_arg(
    value: &RuntimeValue,
    ty: &str,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    memory: &Memory,
    store: &mut Store<()>,
    host_heap: &mut usize,
) -> Result<i64, WasmError> {
    match (value, ty) {
        (RuntimeValue::Int(value), "I32") => Ok(*value),
        (RuntimeValue::Bool(value), "Bool") => Ok(i64::from(*value)),
        (RuntimeValue::Text(value), "Text") => {
            let ptr = alloc_wasm_bytes(memory, store, host_heap, value.as_bytes())?;
            let len = u32::try_from(value.len())
                .map_err(|_| WasmError::new("wasm text argument exceeds 32-bit limits"))?;
            Ok(pack_text_value(ptr, len))
        }
        (RuntimeValue::Enum(value), expected) if enums.contains_key(expected) => {
            let enum_ty = enums.get(expected).ok_or_else(|| {
                WasmError::new(format!("missing wasm enum metadata for `{expected}`"))
            })?;
            if enum_is_payload_free(enum_ty) {
                encode_enum_tag_wasm(value, enums)
            } else {
                let ptr = write_enum_to_memory(
                    value, expected, records, enums, memory, store, host_heap,
                )?;
                Ok(i64::from(ptr))
            }
        }
        (RuntimeValue::Record(record), expected) if records.contains_key(expected) => {
            let ptr =
                write_record_to_memory(record, expected, records, enums, memory, store, host_heap)?;
            Ok(i64::from(ptr))
        }
        (RuntimeValue::Unit, _) => Err(WasmError::new(
            "wasm function arguments cannot contain `unit` in stage-0",
        )),
        (value, ty) => Err(WasmError::new(format!(
            "runtime value `{}` does not match wasm type `{ty}`",
            value.render()
        ))),
    }
}

pub(super) fn read_text_from_memory(
    memory: &Memory,
    store: &mut Store<()>,
    ptr: usize,
    len: usize,
) -> Result<Vec<u8>, WasmError> {
    let end = ptr
        .checked_add(len)
        .ok_or_else(|| WasmError::new("wasm text range overflows"))?;
    if end > memory.data_size(&store) {
        return Err(WasmError::new(
            "wasm text result points outside linear memory",
        ));
    }
    let mut bytes = vec![0u8; len];
    memory
        .read(store, ptr, &mut bytes)
        .map_err(|error| WasmError::new(format!("failed to read wasm text result: {error}")))?;
    Ok(bytes)
}

pub(super) fn decode_record_from_memory(
    memory: &Memory,
    store: &mut Store<()>,
    ptr: usize,
    name: &str,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
) -> Result<RuntimeValue, WasmError> {
    let record = records
        .get(name)
        .ok_or_else(|| WasmError::new(format!("missing wasm record metadata for `{name}`")))?;
    let mut fields = Vec::with_capacity(record.fields.len());
    for (index, field) in record.fields.iter().enumerate() {
        let field_ptr = ptr
            .checked_add(
                usize::try_from(record_offset(index)?)
                    .map_err(|_| WasmError::new("wasm record offset exceeds host limits"))?,
            )
            .ok_or_else(|| WasmError::new("wasm record field pointer overflows"))?;
        let raw = read_i64_from_memory(memory, store, field_ptr)?;
        let value = match &field.kind {
            WasmValueKind::I32 => RuntimeValue::Int(raw),
            WasmValueKind::F64 => RuntimeValue::F64(f64::from_bits(raw as u64)),
            WasmValueKind::Bool => RuntimeValue::Bool(raw != 0),
            WasmValueKind::TextBuilder => {
                return Err(WasmError::new(
                    "wasm backend does not yet support text builder values in stage-0",
                ));
            }
            WasmValueKind::List => {
                return Err(WasmError::new(
                    "wasm backend does not yet support List values in stage-0",
                ));
            }
            WasmValueKind::Text => {
                let (text_ptr, text_len) = unpack_text_value(raw)?;
                let bytes = read_text_from_memory(memory, store, text_ptr, text_len)?;
                let text = String::from_utf8(bytes).map_err(|error| {
                    WasmError::new(format!("wasm record text is not utf-8: {error}"))
                })?;
                RuntimeValue::Text(text)
            }
            WasmValueKind::Enum(name) => {
                let enum_ty = enums.get(name).ok_or_else(|| {
                    WasmError::new(format!("missing wasm enum metadata for `{name}`"))
                })?;
                if enum_is_payload_free(enum_ty) {
                    decode_payload_free_enum_tag(raw, name, enums)?
                } else {
                    let enum_ptr = usize::try_from(raw)
                        .map_err(|_| WasmError::new("wasm enum pointer exceeds host limits"))?;
                    decode_enum_from_memory(memory, store, enum_ptr, name, records, enums)?
                }
            }
            WasmValueKind::Record(child) => {
                let child_ptr = usize::try_from(raw).map_err(|_| {
                    WasmError::new("wasm nested record pointer exceeds host limits")
                })?;
                decode_record_from_memory(memory, store, child_ptr, child, records, enums)?
            }
            WasmValueKind::Unit => RuntimeValue::Unit,
        };
        fields.push((field.name.clone(), value));
    }
    Ok(RuntimeValue::Record(RuntimeRecord {
        name: name.to_owned(),
        fields,
    }))
}

pub(super) fn decode_enum_from_memory(
    memory: &Memory,
    store: &mut Store<()>,
    ptr: usize,
    name: &str,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
) -> Result<RuntimeValue, WasmError> {
    let enum_ty = enums
        .get(name)
        .ok_or_else(|| WasmError::new(format!("missing wasm enum metadata for `{name}`")))?;
    let tag = read_i64_from_memory(memory, store, ptr)?;
    let index = usize::try_from(tag)
        .map_err(|_| WasmError::new(format!("enum `{name}` tag `{tag}` is out of range")))?;
    let variant = enum_ty
        .variants
        .get(index)
        .ok_or_else(|| WasmError::new(format!("enum `{name}` tag `{tag}` is out of range")))?;
    let payload = match &variant.payload {
        None => None,
        Some(kind) => {
            let raw = read_i64_from_memory(memory, store, ptr + 8)?;
            let value = match kind {
                WasmValueKind::I32 => RuntimeValue::Int(raw),
                WasmValueKind::F64 => RuntimeValue::F64(f64::from_bits(raw as u64)),
                WasmValueKind::Bool => RuntimeValue::Bool(raw != 0),
                WasmValueKind::TextBuilder => {
                    return Err(WasmError::new(
                        "wasm backend does not yet support text builder values in stage-0",
                    ));
                }
                WasmValueKind::List => {
                    return Err(WasmError::new(
                        "wasm backend does not yet support List values in stage-0",
                    ));
                }
                WasmValueKind::Text => {
                    let (text_ptr, text_len) = unpack_text_value(raw)?;
                    let bytes = read_text_from_memory(memory, store, text_ptr, text_len)?;
                    let text = String::from_utf8(bytes).map_err(|error| {
                        WasmError::new(format!("wasm enum text is not utf-8: {error}"))
                    })?;
                    RuntimeValue::Text(text)
                }
                WasmValueKind::Enum(child) => {
                    let child_ty = enums.get(child).ok_or_else(|| {
                        WasmError::new(format!("missing wasm enum metadata for `{child}`"))
                    })?;
                    if enum_is_payload_free(child_ty) {
                        decode_payload_free_enum_tag(raw, child, enums)?
                    } else {
                        let child_ptr = usize::try_from(raw).map_err(|_| {
                            WasmError::new("wasm nested enum pointer exceeds host limits")
                        })?;
                        decode_enum_from_memory(memory, store, child_ptr, child, records, enums)?
                    }
                }
                WasmValueKind::Record(child) => {
                    let child_ptr = usize::try_from(raw).map_err(|_| {
                        WasmError::new("wasm nested record pointer exceeds host limits")
                    })?;
                    decode_record_from_memory(memory, store, child_ptr, child, records, enums)?
                }
                WasmValueKind::Unit => RuntimeValue::Unit,
            };
            Some(Box::new(value))
        }
    };
    Ok(RuntimeValue::Enum(crate::RuntimeEnum {
        name: name.to_owned(),
        variant: variant.name.clone(),
        payload,
    }))
}

fn alloc_wasm_bytes(
    memory: &Memory,
    store: &mut Store<()>,
    host_heap: &mut usize,
    bytes: &[u8],
) -> Result<u32, WasmError> {
    let aligned = align_usize(*host_heap, 8)?;
    let end = aligned
        .checked_add(bytes.len())
        .ok_or_else(|| WasmError::new("wasm host allocation overflows"))?;
    ensure_wasm_memory(memory, store, end)?;
    if !bytes.is_empty() {
        memory.write(store, aligned, bytes).map_err(|error| {
            WasmError::new(format!("failed to write wasm argument bytes: {error}"))
        })?;
    }
    *host_heap = end;
    u32::try_from(aligned).map_err(|_| WasmError::new("wasm host pointer exceeds 32-bit limits"))
}

fn write_record_to_memory(
    record: &RuntimeRecord,
    expected: &str,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    memory: &Memory,
    store: &mut Store<()>,
    host_heap: &mut usize,
) -> Result<u32, WasmError> {
    let layout = records
        .get(expected)
        .ok_or_else(|| WasmError::new(format!("missing wasm record metadata for `{expected}`")))?;
    let size = usize::try_from(record_size(layout))
        .map_err(|_| WasmError::new("wasm record size exceeds host limits"))?;
    let ptr = alloc_wasm_zeroed(memory, store, host_heap, size)?;
    for (index, field) in layout.fields.iter().enumerate() {
        let store = &mut *store;
        let (_, value) = record
            .fields
            .iter()
            .find(|(name, _)| name == &field.name)
            .ok_or_else(|| {
                WasmError::new(format!(
                    "runtime record `{}` is missing field `{}` for wasm conversion",
                    record.name, field.name
                ))
            })?;
        let raw = match (&field.kind, value) {
            (WasmValueKind::I32, RuntimeValue::Int(value)) => *value,
            (WasmValueKind::Bool, RuntimeValue::Bool(value)) => i64::from(*value),
            (WasmValueKind::Text, RuntimeValue::Text(value)) => {
                let text_ptr = alloc_wasm_bytes(memory, store, host_heap, value.as_bytes())?;
                let len = u32::try_from(value.len())
                    .map_err(|_| WasmError::new("wasm text argument exceeds 32-bit limits"))?;
                pack_text_value(text_ptr, len)
            }
            (WasmValueKind::Enum(name), RuntimeValue::Enum(value)) => {
                let child = enums.get(name).ok_or_else(|| {
                    WasmError::new(format!("missing wasm enum metadata for `{name}`"))
                })?;
                if enum_is_payload_free(child) {
                    encode_enum_tag_wasm(value, enums)?
                } else {
                    i64::from(write_enum_to_memory(
                        value, name, records, enums, memory, store, host_heap,
                    )?)
                }
            }
            (WasmValueKind::Record(name), RuntimeValue::Record(value)) => i64::from(
                write_record_to_memory(value, name, records, enums, memory, store, host_heap)?,
            ),
            (WasmValueKind::Unit, RuntimeValue::Unit) => 0,
            (kind, value) => {
                return Err(WasmError::new(format!(
                    "runtime record field `{}` value `{}` does not match wasm kind {:?}",
                    field.name,
                    value.render(),
                    kind
                )));
            }
        };
        memory
            .write(
                store,
                usize::try_from(ptr).expect("u32 fits in usize")
                    + usize::try_from(record_offset(index)?)
                        .map_err(|_| WasmError::new("wasm record offset exceeds host limits"))?,
                &raw.to_le_bytes(),
            )
            .map_err(|error| {
                WasmError::new(format!("failed to write wasm record field: {error}"))
            })?;
    }
    Ok(ptr)
}

fn write_enum_to_memory(
    value: &crate::RuntimeEnum,
    expected: &str,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    memory: &Memory,
    store: &mut Store<()>,
    host_heap: &mut usize,
) -> Result<u32, WasmError> {
    let enum_ty = enums
        .get(expected)
        .ok_or_else(|| WasmError::new(format!("missing wasm enum metadata for `{expected}`")))?;
    let variant = enum_ty
        .variants
        .iter()
        .find(|variant| variant.name == value.variant)
        .ok_or_else(|| {
            WasmError::new(format!(
                "enum `{expected}` has no variant `{}`",
                value.variant
            ))
        })?;
    let tag = encode_enum_tag_wasm(value, enums)?;
    let ptr = alloc_wasm_zeroed(memory, store, host_heap, PAYLOAD_ENUM_SIZE)?;
    memory
        .write(
            &mut *store,
            usize::try_from(ptr).expect("u32 fits in usize"),
            &tag.to_le_bytes(),
        )
        .map_err(|error| WasmError::new(format!("failed to write wasm enum tag: {error}")))?;
    let payload_raw = match (&variant.payload, &value.payload) {
        (None, None) => 0,
        (Some(payload_kind), Some(payload)) => match (payload_kind, payload.as_ref()) {
            (WasmValueKind::I32, RuntimeValue::Int(value)) => *value,
            (WasmValueKind::Bool, RuntimeValue::Bool(value)) => i64::from(*value),
            (WasmValueKind::Text, RuntimeValue::Text(value)) => {
                let text_ptr = alloc_wasm_bytes(memory, store, host_heap, value.as_bytes())?;
                let len = u32::try_from(value.len())
                    .map_err(|_| WasmError::new("wasm text argument exceeds 32-bit limits"))?;
                pack_text_value(text_ptr, len)
            }
            (WasmValueKind::Enum(name), RuntimeValue::Enum(value)) => {
                let child = enums.get(name).ok_or_else(|| {
                    WasmError::new(format!("missing wasm enum metadata for `{name}`"))
                })?;
                if enum_is_payload_free(child) {
                    encode_enum_tag_wasm(value, enums)?
                } else {
                    i64::from(write_enum_to_memory(
                        value,
                        name,
                        records,
                        enums,
                        memory,
                        &mut *store,
                        host_heap,
                    )?)
                }
            }
            (WasmValueKind::Record(name), RuntimeValue::Record(value)) => {
                i64::from(write_record_to_memory(
                    value,
                    name,
                    records,
                    enums,
                    memory,
                    &mut *store,
                    host_heap,
                )?)
            }
            (WasmValueKind::Unit, RuntimeValue::Unit) => 0,
            (kind, payload) => {
                return Err(WasmError::new(format!(
                    "runtime enum payload `{}` does not match wasm kind {:?}",
                    payload.render(),
                    kind
                )));
            }
        },
        (Some(_), None) => {
            return Err(WasmError::new(format!(
                "enum `{expected}.{}` is missing its payload",
                value.variant
            )));
        }
        (None, Some(_)) => {
            return Err(WasmError::new(format!(
                "enum `{expected}.{}` does not accept a payload",
                value.variant
            )));
        }
    };
    memory
        .write(
            &mut *store,
            usize::try_from(ptr).expect("u32 fits in usize") + 8,
            &payload_raw.to_le_bytes(),
        )
        .map_err(|error| WasmError::new(format!("failed to write wasm enum payload: {error}")))?;
    Ok(ptr)
}

fn alloc_wasm_zeroed(
    memory: &Memory,
    store: &mut Store<()>,
    host_heap: &mut usize,
    len: usize,
) -> Result<u32, WasmError> {
    let aligned = align_usize(*host_heap, 8)?;
    let end = aligned
        .checked_add(len)
        .ok_or_else(|| WasmError::new("wasm host allocation overflows"))?;
    ensure_wasm_memory(memory, store, end)?;
    if len > 0 {
        memory
            .write(store, aligned, &vec![0u8; len])
            .map_err(|error| {
                WasmError::new(format!("failed to zero wasm record storage: {error}"))
            })?;
    }
    *host_heap = end;
    u32::try_from(aligned).map_err(|_| WasmError::new("wasm host pointer exceeds 32-bit limits"))
}

fn ensure_wasm_memory(memory: &Memory, store: &mut Store<()>, end: usize) -> Result<(), WasmError> {
    if end <= memory.data_size(&store) {
        return Ok(());
    }
    let needed = end - memory.data_size(&store);
    let pages = u64::try_from(needed.div_ceil(65_536))
        .map_err(|_| WasmError::new("wasm host memory growth exceeds limits"))?;
    memory
        .grow(store, pages)
        .map(|_| ())
        .map_err(|error| WasmError::new(format!("failed to grow wasm memory: {error}")))
}

fn align_usize(value: usize, align: usize) -> Result<usize, WasmError> {
    let addend = align
        .checked_sub(1)
        .ok_or_else(|| WasmError::new("wasm alignment underflow"))?;
    let aligned = value
        .checked_add(addend)
        .ok_or_else(|| WasmError::new("wasm host alignment overflows"))?;
    Ok(aligned & !addend)
}

fn read_i64_from_memory(
    memory: &Memory,
    store: &mut Store<()>,
    ptr: usize,
) -> Result<i64, WasmError> {
    let end = ptr
        .checked_add(8)
        .ok_or_else(|| WasmError::new("wasm record field read overflows"))?;
    if end > memory.data_size(&store) {
        return Err(WasmError::new(
            "wasm record field points outside linear memory",
        ));
    }
    let mut bytes = [0u8; 8];
    memory
        .read(store, ptr, &mut bytes)
        .map_err(|error| WasmError::new(format!("failed to read wasm record field: {error}")))?;
    Ok(i64::from_le_bytes(bytes))
}
