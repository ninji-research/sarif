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
    link_proc_exit(&mut linker)?;
    link_clock_time_get(&mut linker)?;
    link_env(&mut linker, &[])?;
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

fn read_cstring_from_memory(data: &[u8], offset: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut i = offset;
    while i < data.len() && data[i] != 0 {
        bytes.push(data[i]);
        i += 1;
    }
    bytes
}

fn allocate_in_memory<T: wasmtime::AsContextMut>(
    memory: &Memory,
    store: &mut T,
    len: usize,
) -> i32 {
    let data_len = memory.data_size(&store);
    let ptr = data_len as i32;
    if len == 0 {
        return ptr;
    }
    let grow = (len as u64).div_ceil(65536);
    if memory.grow(store, grow).is_err() {
        return -1;
    }
    ptr
}

fn link_proc_exit(linker: &mut Linker<()>) -> Result<(), WasmError> {
    linker
        .func_wrap("wasi_snapshot_preview1", "proc_exit", |_code: i32| -> () {
            std::process::exit(_code);
        })
        .map_err(|error| WasmError::new(format!("failed to link WASI proc_exit: {error}")))?;
    Ok(())
}

fn link_clock_time_get(linker: &mut Linker<()>) -> Result<(), WasmError> {
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "clock_time_get",
            |mut caller: Caller<'_, ()>, _clock_id: i32, _precision: i64, result_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return -1;
                };
                let nanos = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                {
                    Ok(d) => d.as_nanos() as i64,
                    Err(_) => 0,
                };
                if result_ptr < 0 {
                    return -1;
                }
                let bytes = nanos.to_le_bytes();
                if memory
                    .write(&mut caller, result_ptr as usize, &bytes)
                    .is_err()
                {
                    return -1;
                }
                0
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link WASI clock_time_get: {error}")))?;
    Ok(())
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

fn link_env(linker: &mut Linker<()>, args: &[String]) -> Result<(), WasmError> {
    let args: Vec<String> = args.to_vec();
    let args_for_argv = args.clone();
    let stdin: Vec<u8> = Vec::new();
    linker
        .func_wrap("env", "__host_argc", move || -> i64 {
            i64::try_from(args.len()).unwrap_or(0)
        })
        .map_err(|error| WasmError::new(format!("failed to link env __host_argc: {error}")))?;
    linker
        .func_wrap(
            "env",
            "__host_argv",
            move |mut caller: Caller<'_, ()>, index: i64, buf_ptr: i32, buf_len: i32| -> i32 {
                if index < 0 || buf_ptr < 0 || buf_len < 0 {
                    return -1;
                }
                let idx = index as usize;
                if idx >= args_for_argv.len() {
                    return -1;
                }
                let arg_bytes = args_for_argv[idx].as_bytes();
                let to_copy = arg_bytes.len().min(buf_len as usize);
                if to_copy == 0 {
                    return 0;
                }
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return -1;
                };
                if memory
                    .write(&mut caller, buf_ptr as usize, &arg_bytes[..to_copy])
                    .is_err()
                {
                    return -1;
                }
                i32::try_from(to_copy).unwrap_or(-1)
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link env __host_argv: {error}")))?;
    linker
        .func_wrap(
            "env",
            "__host_stdin_read",
            move |mut caller: Caller<'_, ()>, buf_ptr: i32, buf_len: i32| -> i32 {
                if buf_ptr < 0 || buf_len < 0 {
                    return -1;
                }
                let to_copy = stdin.len().min(buf_len as usize);
                if to_copy == 0 {
                    return 0;
                }
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return -1;
                };
                if memory
                    .write(&mut caller, buf_ptr as usize, &stdin[..to_copy])
                    .is_err()
                {
                    return -1;
                }
                i32::try_from(to_copy).unwrap_or(-1)
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __host_stdin_read: {error}"))
        })?;
    linker
        .func_wrap(
            "env",
            "__sarif_env_get",
            |mut caller: Caller<'_, ()>, key_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if key_ptr < 0 {
                    return 0;
                }
                let key_bytes = read_cstring_from_memory(data, key_ptr as usize);
                let key = match String::from_utf8(key_bytes) {
                    Ok(k) => k,
                    Err(_) => return 0,
                };
                let value = match std::env::var(&key) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let value_bytes = value.as_bytes();
                let buf_ptr = allocate_in_memory(&memory, &mut caller, value_bytes.len());
                if buf_ptr < 0 {
                    return 0;
                }
                if memory
                    .write(&mut caller, buf_ptr as usize, value_bytes)
                    .is_err()
                {
                    return 0;
                }
                buf_ptr
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link env __sarif_env_get: {error}")))?;
    linker
        .func_wrap(
            "env",
            "__sarif_env_set",
            |mut caller: Caller<'_, ()>, key_ptr: i32, value_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if key_ptr < 0 || value_ptr < 0 {
                    return 0;
                }
                let key_bytes = read_cstring_from_memory(data, key_ptr as usize);
                let value_bytes = read_cstring_from_memory(data, value_ptr as usize);
                let key = match String::from_utf8(key_bytes) {
                    Ok(k) => k,
                    Err(_) => return 0,
                };
                let value = match String::from_utf8(value_bytes) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                unsafe {
                    std::env::set_var(&key, &value);
                }
                1
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link env __sarif_env_set: {error}")))?;
    linker
        .func_wrap(
            "env",
            "__sarif_env_remove",
            |mut caller: Caller<'_, ()>, key_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if key_ptr < 0 {
                    return 0;
                }
                let key_bytes = read_cstring_from_memory(data, key_ptr as usize);
                let key = match String::from_utf8(key_bytes) {
                    Ok(k) => k,
                    Err(_) => return 0,
                };
                unsafe {
                    std::env::remove_var(&key);
                }
                1
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_env_remove: {error}"))
        })?;
    linker
        .func_wrap(
            "env",
            "__sarif_env_keys",
            |mut caller: Caller<'_, ()>| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let keys: String = std::env::vars()
                    .map(|(k, _)| k)
                    .collect::<Vec<_>>()
                    .join("\n");
                let bytes = keys.as_bytes();
                let buf_ptr = allocate_in_memory(&memory, &mut caller, bytes.len());
                if buf_ptr < 0 {
                    return 0;
                }
                if memory.write(&mut caller, buf_ptr as usize, bytes).is_err() {
                    return 0;
                }
                buf_ptr
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link env __sarif_env_keys: {error}")))?;
    linker
        .func_wrap(
            "env",
            "__sarif_dir_create",
            |mut caller: Caller<'_, ()>, path_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if path_ptr < 0 {
                    return 0;
                }
                let path_bytes = read_cstring_from_memory(data, path_ptr as usize);
                let path = match String::from_utf8(path_bytes) {
                    Ok(p) => p,
                    Err(_) => return 0,
                };
                if std::fs::create_dir(&path).is_err() {
                    0
                } else {
                    1
                }
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_dir_create: {error}"))
        })?;
    linker
        .func_wrap(
            "env",
            "__sarif_dir_remove",
            |mut caller: Caller<'_, ()>, path_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if path_ptr < 0 {
                    return 0;
                }
                let path_bytes = read_cstring_from_memory(data, path_ptr as usize);
                let path = match String::from_utf8(path_bytes) {
                    Ok(p) => p,
                    Err(_) => return 0,
                };
                if std::fs::remove_dir(&path).is_err() {
                    0
                } else {
                    1
                }
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_dir_remove: {error}"))
        })?;
    linker
        .func_wrap(
            "env",
            "__sarif_dir_list",
            |mut caller: Caller<'_, ()>, path_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if path_ptr < 0 {
                    return 0;
                }
                let path_bytes = read_cstring_from_memory(data, path_ptr as usize);
                let path = match String::from_utf8(path_bytes) {
                    Ok(p) => p,
                    Err(_) => return 0,
                };
                let entries = match std::fs::read_dir(&path) {
                    Ok(rd) => rd,
                    Err(_) => return 0,
                };
                let names: String = entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect::<Vec<_>>()
                    .join("\n");
                let bytes = names.as_bytes();
                let buf_ptr = allocate_in_memory(&memory, &mut caller, bytes.len());
                if buf_ptr < 0 {
                    return 0;
                }
                if memory.write(&mut caller, buf_ptr as usize, bytes).is_err() {
                    return 0;
                }
                buf_ptr
            },
        )
        .map_err(|error| WasmError::new(format!("failed to link env __sarif_dir_list: {error}")))?;
    linker
        .func_wrap(
            "env",
            "__sarif_dir_exists",
            |mut caller: Caller<'_, ()>, path_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if path_ptr < 0 {
                    return 0;
                }
                let path_bytes = read_cstring_from_memory(data, path_ptr as usize);
                let path = match String::from_utf8(path_bytes) {
                    Ok(p) => p,
                    Err(_) => return 0,
                };
                if std::path::Path::new(&path).is_dir() {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_dir_exists: {error}"))
        })?;
    linker
        .func_wrap(
            "env",
            "__sarif_dir_current",
            |mut caller: Caller<'_, ()>| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let cwd = match std::env::current_dir() {
                    Ok(d) => d,
                    Err(_) => return 0,
                };
                let cwd_str = match cwd.to_str() {
                    Some(s) => s.to_owned(),
                    None => return 0,
                };
                let bytes = cwd_str.as_bytes();
                let buf_ptr = allocate_in_memory(&memory, &mut caller, bytes.len());
                if buf_ptr < 0 {
                    return 0;
                }
                if memory.write(&mut caller, buf_ptr as usize, bytes).is_err() {
                    return 0;
                }
                buf_ptr
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_dir_current: {error}"))
        })?;
    linker
        .func_wrap(
            "env",
            "__sarif_dir_change",
            |mut caller: Caller<'_, ()>, path_ptr: i32| -> i32 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let data = memory.data(&caller);
                if path_ptr < 0 {
                    return 0;
                }
                let path_bytes = read_cstring_from_memory(data, path_ptr as usize);
                let path = match String::from_utf8(path_bytes) {
                    Ok(p) => p,
                    Err(_) => return 0,
                };
                if std::env::set_current_dir(&path).is_err() {
                    0
                } else {
                    1
                }
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_dir_change: {error}"))
        })?;
    linker
        .func_wrap("env", "__sarif_process_id", || -> i32 {
            std::process::id() as i32
        })
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_process_id: {error}"))
        })?;
    linker
        .func_wrap("env", "__sarif_clock_sleep", |ms: i32| {
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            }
        })
        .map_err(|error| {
            WasmError::new(format!("failed to link env __sarif_clock_sleep: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_i32",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> i64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let idx = index as usize;
                if idx + 4 > len {
                    return 0;
                }
                let mut buf = [0u8; 4];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0;
                }
                i64::from(i32::from_le_bytes(buf))
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_i32: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_store_i32",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64, value: i64| {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let idx = index as usize;
                if idx + 4 > len {
                    return;
                }
                let _ = memory.write(&mut caller, ptr + idx, &(value as i32).to_le_bytes());
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_store_i32: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_i64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> i64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let idx = index as usize;
                if idx + 8 > len {
                    return 0;
                }
                let mut buf = [0u8; 8];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0;
                }
                i64::from_le_bytes(buf)
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_i64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_store_i64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64, value: i64| {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let idx = index as usize;
                if idx + 8 > len {
                    return;
                }
                let _ = memory.write(&mut caller, ptr + idx, &value.to_le_bytes());
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_store_i64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_slice_i64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, start: i64, length: i64| -> i64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let (bytes_ptr, bytes_len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                if start < 0 || length < 0 || bytes_ptr == 0 { return 0; }
                let cs = if (start as u64) < (bytes_len as u64) { start as u64 } else { bytes_len as u64 };
                let cl = if (length as u64) < ((bytes_len as u64) - cs) { length as u64 } else { (bytes_len as u64) - cs };
                if cl == 0 { return 0; }
                let alloc_size = 8 + cl;
                if memory.grow(&mut caller, ((alloc_size + 65535) / 65536) as u64).is_err() { return 0; }
                let ptr = (memory.size(&caller) as u64) * 65536 - alloc_size;
                let _ = memory.write(&mut caller, ptr as usize, &cl.to_le_bytes());
                let src_start = if (bytes_ptr as u64 & 1) != 0 {
                    let mut buf = [0u8; 16];
                    if memory.read(&caller, bytes_ptr + 8, &mut buf).is_err() { return 0; }
                    let pp = u64::from_le_bytes(buf[0..8].try_into().unwrap());
                    let off = u64::from_le_bytes(buf[8..16].try_into().unwrap());
                    (pp + off + cs) as usize
                } else {
                    bytes_ptr + 8 + (cs as usize)
                };
                let mut buf = vec![0u8; cl as usize];
                if memory.read(&caller, src_start, &mut buf).is_err() { return 0; }
                let _ = memory.write(&mut caller, ptr as usize + 8, &buf);
                (alloc_size as i64) << 32 | (ptr as i64)
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_slice_i64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_i32_i64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> i64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let idx = index as usize;
                if idx + 4 > len {
                    return 0;
                }
                let mut buf = [0u8; 4];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0;
                }
                i64::from(i32::from_le_bytes(buf))
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_i32_i64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_i64_i64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> i64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let idx = index as usize;
                if idx + 8 > len {
                    return 0;
                }
                let mut buf = [0u8; 8];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0;
                }
                i64::from_le_bytes(buf)
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_i64_i64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_f32_i64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> f64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0.0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0.0,
                };
                let idx = index as usize;
                if idx + 4 > len {
                    return 0.0;
                }
                let mut buf = [0u8; 4];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0.0;
                }
                f32::from_le_bytes(buf) as f64
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_f32_i64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_f64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> f64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0.0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0.0,
                };
                let idx = index as usize;
                if idx + 8 > len {
                    return 0.0;
                }
                let mut buf = [0u8; 8];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0.0;
                }
                f64::from_le_bytes(buf)
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_f64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_store_f64",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64, value: f64| {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let idx = index as usize;
                if idx + 8 > len {
                    return;
                }
                let _ = memory.write(&mut caller, ptr + idx, &value.to_le_bytes());
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_store_f64: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_load_bool",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64| -> i64 {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return 0;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let idx = index as usize;
                if idx + 1 > len {
                    return 0;
                }
                let mut buf = [0u8; 1];
                if memory.read(&caller, ptr + idx, &mut buf).is_err() {
                    return 0;
                }
                if buf[0] != 0 {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_load_bool: {error}"))
        })?;

    linker
        .func_wrap(
            "env",
            "sarif_bytes_store_bool",
            |mut caller: Caller<'_, ()>, bytes_packed: i64, index: i64, value: i64| {
                let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                    return;
                };
                let (ptr, len) = match unpack_text_value(bytes_packed) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let idx = index as usize;
                if idx + 1 > len {
                    return;
                }
                let byte = if value != 0 { [1u8] } else { [0u8] };
                let _ = memory.write(&mut caller, ptr + idx, &byte);
            },
        )
        .map_err(|error| {
            WasmError::new(format!("failed to link env sarif_bytes_store_bool: {error}"))
        })?;

    Ok(())
}

fn instantiate_wasm_module(wasm: &[u8]) -> Result<(Store<()>, Instance), WasmError> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm)
        .map_err(|error| WasmError::new(format!("failed to compile wasm module: {error}")))?;
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    link_fd_write(&mut linker)?;
    link_proc_exit(&mut linker)?;
    link_clock_time_get(&mut linker)?;
    link_env(&mut linker, &[])?;
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|error| WasmError::new(format!("failed to instantiate wasm module: {error}")))?;
    Ok((store, instance))
}

#[cfg(test)]
fn instantiate_wasm_module_with_args(
    wasm: &[u8],
    args: &[String],
) -> Result<(Store<()>, Instance), WasmError> {
    let engine = Engine::default();
    let module = Module::new(&engine, wasm)
        .map_err(|error| WasmError::new(format!("failed to compile wasm module: {error}")))?;
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    link_fd_write(&mut linker)?;
    link_proc_exit(&mut linker)?;
    link_clock_time_get(&mut linker)?;
    link_env(&mut linker, args)?;
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

    use super::super::memory::{read_text_from_memory, unpack_text_value};
    use super::{
        call_main_i64, instantiate_wasm_module, instantiate_wasm_module_with_args,
        link_clock_time_get, link_env, link_fd_write, link_proc_exit, run_function_wasm,
        run_main_wasm,
    };
    use crate::{RuntimeValue, emit_wasm, lower};
    use wasmtime::{Engine, Linker, Module, Store};

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

    #[test]
    fn wasm_arg_count_with_args() {
        let program =
            lower_program("fn main() -> I32 effects [alloc] { perform SystemIO.arg_count() }");
        let wasm = emit_wasm(&program).expect("wasm arg_count program should emit");
        let (mut store, instance) =
            instantiate_wasm_module_with_args(&wasm, &["hello".into(), "world".into()])
                .expect("wasm arg_count module should instantiate");
        let result = call_main_i64(&instance, &mut store).expect("wasm arg_count should return");
        assert_eq!(result, 2);
    }

    #[test]
    fn wasm_arg_text_with_args() {
        let program =
            lower_program("fn main() -> Text effects [alloc] { perform SystemIO.arg_text(1) }");
        let wasm = emit_wasm(&program).expect("wasm arg_text program should emit");
        let (mut store, instance) =
            instantiate_wasm_module_with_args(&wasm, &["hello".into(), "world".into()])
                .expect("wasm arg_text module should instantiate");
        let memory = instance
            .get_memory(&mut store, "memory")
            .expect("missing exported wasm memory for arg_text result");
        let packed = call_main_i64(&instance, &mut store).expect("wasm arg_text should return");
        let (ptr, len) =
            unpack_text_value(packed).expect("wasm arg_text packed value should be valid");
        let bytes = read_text_from_memory(&memory, &mut store, ptr, len)
            .expect("wasm arg_text should read from memory");
        let text = String::from_utf8(bytes).expect("wasm arg_text result should be valid utf-8");
        assert_eq!(text, "world");
    }

    #[test]
    fn wasm_binary_roundtrip_validate() {
        let program = lower_program("fn main() -> I32 { 42 }");
        let wasm = emit_wasm(&program).expect("wasm should emit");
        std::fs::write("/tmp/test_binary.wasm", &wasm).expect("write wasm");

        use wasmparser::{Parser, Validator, WasmFeatures};
        let mut validator =
            Validator::new_with_features(WasmFeatures::default() | WasmFeatures::REFERENCE_TYPES);
        for payload in Parser::new(0).parse_all(&wasm) {
            match payload {
                Ok(p) => {
                    if let Err(e) = validator.payload(&p) {
                        panic!("wasmparser validation error: {e:?}");
                    }
                }
                Err(e) => {
                    panic!("wasmparser parse error: {e:?}");
                }
            }
        }

        let engine = Engine::default();
        let module = Module::new(&engine, wasm).expect("emitted wasm should be valid");
        let mut store = Store::new(&engine, ());
        let mut linker = Linker::new(&engine);
        link_fd_write(&mut linker).expect("fd_write should link");
        link_proc_exit(&mut linker).expect("proc_exit should link");
        link_clock_time_get(&mut linker).expect("clock_time_get should link");
        link_env(&mut linker, &[]).expect("env should link");
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("wasm should instantiate");
        let result = call_main_i64(&instance, &mut store).expect("wasm main should return");
        assert_eq!(result, 42);
    }
}
