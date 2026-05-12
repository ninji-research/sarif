use std::collections::BTreeMap;
use std::io::Write;

use wasmtime::{Caller, Engine, Extern, Instance, Linker, Memory, Module, Store, TypedFunc, Val};

use super::memory::{
    decode_enum_from_memory, decode_payload_free_enum_tag, decode_record_from_memory,
    read_bytes_from_memory, read_text_from_memory, runtime_value_to_wasm_arg, unpack_text_value,
};
use super::{WasmEmitter, WasmEnum, WasmError, WasmRecord, enum_is_payload_free};
use crate::{Program, RuntimeError, RuntimeValue, emit_wasm, run_function, run_main};

/// # Errors
///
/// Returns an error if Wasm emission fails, if the generated module cannot be
/// instantiated by Wasmtime, or if the stage-0 subset cannot represent the
/// program.
pub fn run_main_wasm(program: &Program) -> Result<RuntimeValue, WasmError> {
    run_main(program).map_err(|error| {
        let message = match error {
            RuntimeError::Message(m) => m,
            RuntimeError::EffectUnwind {
                effect, operation, ..
            } => format!("unhandled effect {effect}.{operation}"),
        };
        WasmError::new(format!(
            "interpreter preflight failed before wasm execution: {message}"
        ))
    })?;
    let wasm = emit_wasm(program)?;
    let main = program
        .functions
        .iter()
        .find(|function| function.name == "main")
        .ok_or_else(|| WasmError::new("missing `main` entrypoint"))?;
    if !main.params.is_empty() {
        return Err(WasmError::new("`main` must not take parameters"));
    }

    let emitter = WasmEmitter::new(program)?;
    let (mut store, instance) = instantiate_wasm_module(&wasm)?;
    decode_main_wasm_result(
        &emitter,
        main.return_type.as_deref().unwrap_or("Unit"),
        &instance,
        &mut store,
    )
}

/// # Errors
///
/// Returns an error if Wasm emission fails, if the generated module cannot be
/// instantiated by Wasmtime, or if the stage-0 subset cannot represent the
/// named function or its arguments.
pub fn run_function_wasm(
    program: &Program,
    name: &str,
    args: &[RuntimeValue],
) -> Result<RuntimeValue, WasmError> {
    let interpreter_result = run_function(program, name, args).map_err(|error| {
        let message = match error {
            RuntimeError::Message(m) => m,
            RuntimeError::EffectUnwind {
                effect, operation, ..
            } => format!("unhandled effect {effect}.{operation}"),
        };
        WasmError::new(format!(
            "interpreter preflight failed before wasm execution: {message}"
        ))
    })?;
    let wasm = emit_wasm(program)?;
    let emitter = WasmEmitter::new(program)?;
    let function = program
        .functions
        .iter()
        .find(|function| function.name == name)
        .ok_or_else(|| WasmError::new(format!("missing `{name}` function")))?;
    if function.params.len() != args.len() {
        return Err(WasmError::new(format!(
            "function `{name}` expects {} arguments but got {}",
            function.params.len(),
            args.len()
        )));
    }

    let engine = Engine::default();
    let module = Module::new(&engine, wasm)
        .map_err(|error| WasmError::new(format!("failed to compile wasm module: {error}")))?;
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    link_fd_write(&mut linker)?;
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|error| WasmError::new(format!("failed to instantiate wasm module: {error}")))?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| WasmError::new("missing exported wasm memory"))?;
    let mut host_heap = memory.data_size(&store);
    let wasm_args = args
        .iter()
        .zip(&function.params)
        .map(|(value, param)| {
            runtime_value_to_wasm_arg(
                value,
                &param.ty,
                &emitter.records,
                &emitter.enums,
                &memory,
                &mut store,
                &mut host_heap,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let func = instance
        .get_func(&mut store, name)
        .ok_or_else(|| WasmError::new(format!("missing exported wasm `{name}`")))?;
    let params = wasm_args.into_iter().map(Val::I64).collect::<Vec<_>>();
    let mut results = if function.return_type.is_some() {
        vec![Val::I64(0)]
    } else {
        Vec::new()
    };
    func.call(&mut store, &params, &mut results)
        .map_err(|error| WasmError::new(format!("wasm call failed: {error}")))?;

    let Some(result_type) = function.return_type.as_deref() else {
        return Ok(RuntimeValue::Unit);
    };
    decode_wasm_result(
        result_type,
        if results.is_empty() {
            None
        } else {
            Some(&results[0])
        },
        &memory,
        &mut store,
        &emitter.records,
        &emitter.enums,
        &interpreter_result,
    )
}

fn link_fd_write(linker: &mut Linker<()>) -> Result<(), WasmError> {
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_write",
            |mut caller: Caller<'_, ()>,
             fd: i32,
             iovs: i32,
             iovs_len: i32,
             _nwritten_ptr: i32|
             -> i32 {
                if fd != 1 {
                    return 8;
                }
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 9;
                };
                let data = memory.data(&caller);
                let iov_len = u32::try_from(iovs_len).unwrap_or(0) as usize;
                let iov_base = u32::try_from(iovs).unwrap_or(0) as usize;
                for i in 0..iov_len {
                    let base = iov_base.wrapping_add(i.wrapping_mul(8));
                    if base.wrapping_add(8) > data.len() {
                        return 21;
                    }
                    let ptr =
                        i32::from_le_bytes(data[base..base.wrapping_add(4)].try_into().unwrap());
                    let len = i32::from_le_bytes(
                        data[base.wrapping_add(4)..base.wrapping_add(8)]
                            .try_into()
                            .unwrap(),
                    );
                    if len < 0 {
                        return 21;
                    }
                    let start = u32::try_from(ptr).unwrap_or(0) as usize;
                    let end = start.wrapping_add(u32::try_from(len).unwrap_or(0) as usize);
                    if end > data.len() {
                        return 21;
                    }
                    if std::io::stdout().write_all(&data[start..end]).is_err() {
                        return 5;
                    }
                }
                0
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link WASI fd_write: {error}")))?;
    Ok(())
}

fn instantiate_wasm_module(wasm: &[u8]) -> Result<(Store<()>, Instance), WasmError> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm)
        .map_err(|error| WasmError::new(format!("failed to compile wasm module: {error}")))?;
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    link_fd_write(&mut linker)?;
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|error| WasmError::new(format!("failed to instantiate wasm module: {error}")))?;
    Ok((store, instance))
}

fn call_main_i64(instance: &Instance, store: &mut Store<()>) -> Result<i64, WasmError> {
    let func: TypedFunc<(), i64> = instance
        .get_typed_func(&mut *store, "main")
        .map_err(|error| WasmError::new(format!("failed to load wasm `main`: {error}")))?;
    func.call(&mut *store, ())
        .map_err(|error| WasmError::new(format!("wasm call failed: {error}")))
}

fn decode_main_wasm_result(
    emitter: &WasmEmitter<'_>,
    result_type: &str,
    instance: &Instance,
    store: &mut Store<()>,
) -> Result<RuntimeValue, WasmError> {
    match result_type {
        "I32" => {
            let value = call_main_i64(instance, store)?;
            Ok(RuntimeValue::Int(value))
        }
        "Bool" => {
            let value = call_main_i64(instance, store)?;
            Ok(RuntimeValue::Bool(value != 0))
        }
        "Text" => {
            let packed = call_main_i64(instance, store)?;
            let memory = instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| WasmError::new("missing exported wasm memory for text result"))?;
            let (ptr, len) = unpack_text_value(packed)?;
            let bytes = read_text_from_memory(&memory, store, ptr, len)?;
            let value = String::from_utf8(bytes).map_err(|error| {
                WasmError::new(format!("wasm text result is not utf-8: {error}"))
            })?;
            Ok(RuntimeValue::Text(value))
        }
        "Bytes" => {
            let packed = call_main_i64(instance, store)?;
            let memory = instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| WasmError::new("missing exported wasm memory for bytes result"))?;
            let (ptr, len) = unpack_text_value(packed)?;
            Ok(RuntimeValue::Bytes(read_bytes_from_memory(
                &memory, store, ptr, len,
            )?))
        }
        other if emitter.enums.contains_key(other) => {
            let raw = call_main_i64(instance, store)?;
            let memory = instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| WasmError::new("missing exported wasm memory for enum result"))?;
            let enum_ty = emitter.enums.get(other).ok_or_else(|| {
                WasmError::new(format!("missing wasm enum metadata for `{other}`"))
            })?;
            if enum_is_payload_free(enum_ty) {
                decode_payload_free_enum_tag(raw, other, &emitter.enums)
            } else {
                let ptr = usize::try_from(raw)
                    .map_err(|_| WasmError::new("wasm enum pointer exceeds host limits"))?;
                decode_enum_from_memory(
                    &memory,
                    store,
                    ptr,
                    other,
                    &emitter.records,
                    &emitter.enums,
                )
            }
        }
        other if emitter.records.contains_key(other) => {
            let packed = call_main_i64(instance, store)?;
            let memory = instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| WasmError::new("missing exported wasm memory for record result"))?;
            let ptr = usize::try_from(packed)
                .map_err(|_| WasmError::new("wasm record pointer exceeds host limits"))?;
            decode_record_from_memory(&memory, store, ptr, other, &emitter.records, &emitter.enums)
        }
        "Unit" => {
            let func: TypedFunc<(), ()> = instance
                .get_typed_func(&mut *store, "main")
                .map_err(|error| WasmError::new(format!("failed to load wasm `main`: {error}")))?;
            func.call(&mut *store, ())
                .map_err(|error| WasmError::new(format!("wasm call failed: {error}")))?;
            Ok(RuntimeValue::Unit)
        }
        other => Err(WasmError::new(format!(
            "wasm backend does not support `main` returning `{other}` in stage-0"
        ))),
    }
}

fn decode_wasm_result(
    result_type: &str,
    result: Option<&Val>,
    memory: &Memory,
    store: &mut Store<()>,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    interpreter_result: &RuntimeValue,
) -> Result<RuntimeValue, WasmError> {
    match result_type {
        "Unit" => Ok(RuntimeValue::Unit),
        "I32" => match result {
            Some(Val::I64(value)) => Ok(RuntimeValue::Int(*value)),
            other => Err(WasmError::new(format!(
                "wasm backend expected i64 result for `I32` but observed {other:?}"
            ))),
        },
        "Bool" => match result {
            Some(Val::I64(value)) => Ok(RuntimeValue::Bool(*value != 0)),
            other => Err(WasmError::new(format!(
                "wasm backend expected i64 result for `Bool` but observed {other:?}"
            ))),
        },
        "Text" => match result {
            Some(Val::I64(packed)) => {
                let (ptr, len) = unpack_text_value(*packed)?;
                let bytes = read_text_from_memory(memory, store, ptr, len)?;
                let value = String::from_utf8(bytes).map_err(|error| {
                    WasmError::new(format!("wasm text result is not utf-8: {error}"))
                })?;
                Ok(RuntimeValue::Text(value))
            }
            other => Err(WasmError::new(format!(
                "wasm backend expected i64 result for `Text` but observed {other:?}"
            ))),
        },
        "Bytes" => match result {
            Some(Val::I64(packed)) => {
                let (ptr, len) = unpack_text_value(*packed)?;
                Ok(RuntimeValue::Bytes(read_bytes_from_memory(
                    memory, store, ptr, len,
                )?))
            }
            other => Err(WasmError::new(format!(
                "wasm backend expected i64 result for `Bytes` but observed {other:?}"
            ))),
        },
        other if enums.contains_key(other) => match result {
            Some(Val::I64(raw)) => {
                let enum_ty = enums.get(other).ok_or_else(|| {
                    WasmError::new(format!("missing wasm enum metadata for `{other}`"))
                })?;
                let value = if enum_is_payload_free(enum_ty) {
                    decode_payload_free_enum_tag(*raw, other, enums)?
                } else {
                    let ptr = usize::try_from(*raw)
                        .map_err(|_| WasmError::new("wasm enum pointer exceeds host limits"))?;
                    decode_enum_from_memory(memory, store, ptr, other, records, enums)?
                };
                if &value != interpreter_result {
                    return Err(WasmError::new(format!(
                        "wasm enum result for `{other}` diverged from interpreter"
                    )));
                }
                Ok(value)
            }
            observed => Err(WasmError::new(format!(
                "wasm backend expected i64 result for `{other}` but observed {observed:?}"
            ))),
        },
        other if records.contains_key(other) => match result {
            Some(Val::I64(packed)) => {
                let ptr = usize::try_from(*packed)
                    .map_err(|_| WasmError::new("wasm record pointer exceeds host limits"))?;
                let value = decode_record_from_memory(memory, store, ptr, other, records, enums)?;
                if &value != interpreter_result {
                    return Err(WasmError::new(format!(
                        "wasm record result for `{other}` diverged from interpreter"
                    )));
                }
                Ok(value)
            }
            observed => Err(WasmError::new(format!(
                "wasm backend expected i64 result for `{other}` but observed {observed:?}"
            ))),
        },
        other => Err(WasmError::new(format!(
            "wasm backend does not support `{other}` results in stage-0"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    use super::{call_main_i64, instantiate_wasm_module, run_function_wasm, run_main_wasm};
    use crate::{RuntimeValue, emit_wasm, lower};

    fn lower_program(source: &str) -> crate::Program {
        let lexed = lex(source);
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        lower(&hir.module).program
    }

    fn run_main_without_preflight(source: &str) -> Result<i64, String> {
        let program = lower_program(source);
        let wasm = emit_wasm(&program).map_err(|error| error.message)?;
        let (mut store, instance) =
            instantiate_wasm_module(&wasm).map_err(|error| error.message)?;
        call_main_i64(&instance, &mut store).map_err(|error| error.message)
    }

    #[test]
    fn direct_wasm_execution_traps_on_bounds_failures() {
        let error = run_main_without_preflight("fn main() -> I32 { let xs = [20, 22]; xs[2] }")
            .expect_err("wasm should trap on bounds failure");
        assert!(error.contains("wasm call failed"), "{error}");
        assert!(error.contains("!main"), "{error}");
    }

    #[test]
    fn direct_wasm_execution_traps_on_contract_failures() {
        let error = run_main_without_preflight(
            "fn broken(value: I32) -> I32 ensures result == value + 1 { value }\nfn main() -> I32 { broken(3) }",
        )
        .expect_err("wasm should trap on contract failure");
        assert!(error.contains("wasm call failed"), "{error}");
        assert!(error.contains("!broken"), "{error}");
    }

    #[test]
    fn wasm_function_execution_accepts_bytes_values() {
        let program = lower_program(
            "fn probe(xs: Bytes) -> I32 { if bytes_len(xs) == 4 and bytes_byte(xs, 0) == 115 and bytes_find_byte_range(xs, 0, bytes_len(xs), 105) == 3 and bytes_len(bytes_slice(xs, 1, 3)) == 2 { 42 } else { 0 } }",
        );
        let result = run_function_wasm(&program, "probe", &[RuntimeValue::Bytes(b"sari".to_vec())])
            .expect("wasm bytes function should run");
        assert_eq!(result, RuntimeValue::Int(42));
    }

    #[test]
    fn wasm_text_builder_new_and_finish() {
        let program = lower_program(
            "fn main() -> Text { let mut tb = text_builder_new(); text_builder_finish(tb) }",
        );
        let result = run_main_wasm(&program).expect("wasm text builder should run");
        assert_eq!(result, RuntimeValue::Text(String::new()));
    }

    #[test]
    fn wasm_text_builder_append_text() {
        let program = lower_program(
            "fn build(greeting: Text, name: Text) -> Text {\n  let mut tb = text_builder_new();\n  tb = text_builder_append(tb, greeting);\n  tb = text_builder_append(tb, name);\n  text_builder_finish(tb)\n}",
        );
        let result = run_function_wasm(
            &program,
            "build",
            &[
                RuntimeValue::Text("Hello, ".into()),
                RuntimeValue::Text("World!".into()),
            ],
        )
        .expect("wasm text builder append should run");
        assert_eq!(result, RuntimeValue::Text("Hello, World!".into()));
    }

    #[test]
    fn wasm_text_builder_append_ascii() {
        let program = lower_program(
            "fn main() -> Text {\n  let mut tb = text_builder_new();\n  tb = text_builder_append_ascii(tb, 72);\n  tb = text_builder_append_ascii(tb, 105);\n  text_builder_finish(tb)\n}",
        );
        let result = run_main_wasm(&program).expect("wasm text builder append_ascii should run");
        assert_eq!(result, RuntimeValue::Text("Hi".into()));
    }

    #[test]
    fn wasm_text_builder_append_i32() {
        let program = lower_program(
            "fn main() -> Text {\n  let mut tb = text_builder_new();\n  tb = text_builder_append_i32(tb, 42);\n  text_builder_finish(tb)\n}",
        );
        let result = run_main_wasm(&program).expect("wasm text builder append_i32 should run");
        assert_eq!(result, RuntimeValue::Text("42".into()));
    }

    #[test]
    fn wasm_text_builder_compound() {
        let program = lower_program(
            "fn main() -> Text {\n  let mut tb = text_builder_new();\n  tb = text_builder_append(tb, \"The answer is \");\n  tb = text_builder_append_i32(tb, 42);\n  tb = text_builder_append_ascii(tb, 46);\n  tb = text_builder_append_ascii(tb, 10);\n  text_builder_finish(tb)\n}",
        );
        let result = run_main_wasm(&program).expect("wasm text builder compound should run");
        assert_eq!(result, RuntimeValue::Text("The answer is 42.\n".into()));
    }

    #[test]
    fn wasm_text_index_new_and_get_missing() {
        let program = lower_program(
            "fn main() -> I32 effects [alloc] { let idx = text_index_new(); text_index_get(idx, \"missing\") }",
        );
        let result = run_main_wasm(&program).expect("wasm text index new/get should run");
        assert_eq!(result, RuntimeValue::Int(-1));
    }

    #[test]
    fn wasm_text_index_set_and_get() {
        let program = lower_program(
            "fn main() -> I32 effects [alloc] { let mut idx = text_index_new(); idx = text_index_set(idx, \"alpha\", 42); text_index_get(idx, \"alpha\") }",
        );
        let result = run_main_wasm(&program).expect("wasm text index set/get should run");
        assert_eq!(result, RuntimeValue::Int(42));
    }

    #[test]
    fn wasm_text_index_get_or_insert() {
        let program = lower_program(
            "fn main() -> I32 effects [alloc] { let idx = text_index_new(); text_index_get_or_insert(idx, \"key\", 7) }",
        );
        let result = run_main_wasm(&program).expect("wasm text index get_or_insert should run");
        assert_eq!(result, RuntimeValue::Int(7));
    }

    #[test]
    fn wasm_text_index_get_or_insert_existing() {
        let program = lower_program(
            "fn main() -> I32 effects [alloc] { let mut idx = text_index_new(); idx = text_index_set(idx, \"k\", 99); text_index_get_or_insert(idx, \"k\", 7) }",
        );
        let result =
            run_main_wasm(&program).expect("wasm text index get_or_insert existing should run");
        assert_eq!(result, RuntimeValue::Int(99));
    }

    #[test]
    fn wasm_text_index_multiple_ops() {
        let program = lower_program(
            "fn main() -> I32 effects [alloc] { let mut idx = text_index_new(); idx = text_index_set(idx, \"a\", 10); idx = text_index_set(idx, \"b\", 20); idx = text_index_set(idx, \"c\", 30); text_index_get(idx, \"b\") }",
        );
        let result = run_main_wasm(&program).expect("wasm text index multiple ops should run");
        assert_eq!(result, RuntimeValue::Int(20));
    }
}
