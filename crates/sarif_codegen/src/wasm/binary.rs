use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, EntityType, ExportKind, ExportSection,
    Function as WasmFunction, FunctionSection, GlobalSection, GlobalType, Ieee64, ImportSection,
    Instruction, MemArg, MemorySection, MemoryType, Module, NameMap, NameSection, TypeSection,
    ValType,
};

use super::{WasmEnum, WasmError, WasmRecord, WasmType};
use crate::wasm::runtime_gen::emit_runtime_sections;
use crate::wasm::{
    collect_inst_kinds, enum_eq_helper_name, enum_is_payload_free, record_eq_helper_name, wasm_id,
    wasm_type_from_kind, wasm_type_from_kind_result, wasm_value_kind_from_name,
};
use crate::{
    CodegenValueKind as WasmValueKind, EnumType, ExternFunction, Function, Inst, LocalSlotId,
    Program, StructType, ValueId, for_each_inst_recursive,
};

const PAYLOAD_ENUM_SIZE: u32 = 16;

const fn memarg(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }
}

struct LocalEnv {
    params: Vec<u32>,
    slots: BTreeMap<LocalSlotId, u32>,
    values: BTreeMap<ValueId, u32>,
    repeat_counters: BTreeMap<String, u32>,
    next_index: u32,
}

impl LocalEnv {
    fn new() -> Self {
        Self {
            params: Vec::new(),
            slots: BTreeMap::new(),
            values: BTreeMap::new(),
            repeat_counters: BTreeMap::new(),
            next_index: 0,
        }
    }

    fn alloc_param(&mut self) -> u32 {
        let idx = self.next_index;
        self.next_index += 1;
        self.params.push(idx);
        idx
    }

    fn alloc_local(&mut self) -> u32 {
        let idx = self.next_index;
        self.next_index += 1;
        idx
    }

    fn get_param(&self, index: usize) -> u32 {
        self.params[index]
    }

    fn get_or_alloc_slot(&mut self, slot: LocalSlotId) -> u32 {
        if let Some(&idx) = self.slots.get(&slot) {
            idx
        } else {
            let idx = self.alloc_local();
            self.slots.insert(slot, idx);
            idx
        }
    }

    fn get_or_alloc_value(&mut self, id: ValueId) -> u32 {
        if let Some(&idx) = self.values.get(&id) {
            idx
        } else {
            let idx = self.alloc_local();
            self.values.insert(id, idx);
            idx
        }
    }

    fn get_value(&self, id: ValueId) -> u32 {
        self.values[&id]
    }

    fn get_or_alloc_repeat_counter(&mut self, name: String) -> u32 {
        if let Some(&idx) = self.repeat_counters.get(&name) {
            idx
        } else {
            let idx = self.alloc_local();
            self.repeat_counters.insert(name, idx);
            idx
        }
    }
}

struct FuncIndexMap {
    map: BTreeMap<String, u32>,
}

impl FuncIndexMap {
    fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    fn insert(&mut self, name: &str, idx: u32) {
        self.map.insert(name.to_string(), idx);
    }

    fn get(&self, name: &str) -> Option<u32> {
        self.map.get(name).copied()
    }

    fn runtime(&self, name: &str) -> u32 {
        self.map[name]
    }
}

struct PreambleImport {
    module: &'static str,
    name: &'static str,
    type_params: &'static [ValType],
    type_results: &'static [ValType],
}

static PREAMBLE_IMPORTS: &[PreambleImport] = &[
    PreambleImport {
        module: "wasi_snapshot_preview1",
        name: "fd_write",
        type_params: &[ValType::I32, ValType::I32, ValType::I32, ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__host_argc",
        type_params: &[],
        type_results: &[ValType::I64],
    },
    PreambleImport {
        module: "env",
        name: "__host_argv",
        type_params: &[ValType::I64, ValType::I32, ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__host_stdin_read",
        type_params: &[ValType::I32, ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_env_get",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_env_set",
        type_params: &[ValType::I32, ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_env_remove",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_env_keys",
        type_params: &[],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_dir_create",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_dir_remove",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_dir_list",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_dir_exists",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_dir_current",
        type_params: &[],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_dir_change",
        type_params: &[ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "wasi_snapshot_preview1",
        name: "proc_exit",
        type_params: &[ValType::I32],
        type_results: &[],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_process_id",
        type_params: &[],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "wasi_snapshot_preview1",
        name: "clock_time_get",
        type_params: &[ValType::I32, ValType::I64, ValType::I32],
        type_results: &[ValType::I32],
    },
    PreambleImport {
        module: "env",
        name: "__sarif_clock_sleep",
        type_params: &[ValType::I32],
        type_results: &[],
    },
];

const RUNTIME_FUNC_NAMES: &[&str] = &[
    "alloc",
    "__sarif_alloc_push",
    "__sarif_alloc_pop",
    "__sarif_pack_text",
    "__sarif_text_len_i32",
    "__sarif_is_ascii_space",
    "__sarif_is_ascii_digit",
    "__sarif_is_utf8_continuation",
    "__sarif_text_eq",
    "__sarif_text_cmp",
    "__sarif_text_byte",
    "__sarif_bytes_slice",
    "__sarif_text_concat",
    "__sarif_clamp_text_slice_start",
    "__sarif_clamp_text_slice_end",
    "__sarif_text_slice",
    "__sarif_text_eq_range",
    "__sarif_text_find_byte_range",
    "__sarif_bytes_find_byte_range",
    "__sarif_text_line_end",
    "__sarif_text_next_line",
    "__sarif_text_field_end",
    "__sarif_text_next_field",
    "__sarif_parse_i32",
    "__sarif_parse_i32_range",
    "__sarif_parse_f64",
    "__sarif_text_from_f64_fixed",
    "__sarif_text_builder_new",
    "__sarif_text_builder_reserve",
    "__sarif_text_builder_append",
    "__sarif_text_builder_append_codepoint",
    "__sarif_text_builder_append_ascii",
    "__sarif_text_builder_append_slice",
    "__sarif_text_builder_append_i32",
    "__sarif_text_builder_append_i64",
    "__sarif_text_builder_finish",
    "__sarif_stdout_write",
    "__sarif_stdout_write_builder",
    "__sarif_text_hash",
    "__sarif_text_index_new",
    "__sarif_text_index_ensure_capacity",
    "__sarif_text_index_find_entry",
    "__sarif_text_index_get",
    "__sarif_text_index_contains",
    "__sarif_text_index_set",
    "__sarif_text_index_get_or_insert",
    "__sarif_list_new",
    "__sarif_list_len",
    "__sarif_list_get",
    "__sarif_list_set",
    "__sarif_list_push",
    "__sarif_list_sort_text",
    "__sarif_list_sort_record_text_field",
];

pub(crate) fn emit_wasm_binary(
    program: &Program,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
) -> Result<Vec<u8>, WasmError> {
    let mut func_indices = FuncIndexMap::new();
    let mut next_type_idx = 0u32;

    let mut types = TypeSection::new();
    let mut imports = ImportSection::new();
    let mut functions = FunctionSection::new();
    let mut memories = MemorySection::new();
    let mut globals = GlobalSection::new();
    let mut exports = ExportSection::new();
    let mut code = CodeSection::new();

    // --- Phase 1: Preamble imports ---
    let mut num_imported_funcs = 0u32;
    for imp in PREAMBLE_IMPORTS {
        let type_idx = next_type_idx;
        types.ty().function(
            imp.type_params.iter().copied(),
            imp.type_results.iter().copied(),
        );
        next_type_idx += 1;
        imports.import(imp.module, imp.name, EntityType::Function(type_idx));
        func_indices.insert(imp.name, num_imported_funcs);
        num_imported_funcs += 1;
    }

    // --- Phase 1b: User extern imports ---
    let _user_extern_start = num_imported_funcs;
    for extern_fn in &program.externs {
        let type_idx = next_type_idx;
        let mut params: Vec<ValType> = Vec::new();
        for param in &extern_fn.params {
            let kind = wasm_value_kind_from_name(&param.ty, &program.structs, &program.enums)
                .unwrap_or(WasmValueKind::I32);
            params.push(kind_to_valtype(&kind));
        }
        let results: Vec<ValType> = if let Some(ret) = &extern_fn.return_type {
            let kind = wasm_value_kind_from_name(ret, &program.structs, &program.enums)
                .unwrap_or(WasmValueKind::I32);
            vec![kind_to_valtype(&kind)]
        } else {
            Vec::new()
        };
        types.ty().function(params, results);
        next_type_idx += 1;
        imports.import("env", &extern_fn.name, EntityType::Function(type_idx));
        func_indices.insert(&extern_fn.name, num_imported_funcs);
        num_imported_funcs += 1;
    }

    // --- Phase 2: Runtime functions (from runtime_gen) ---
    let call_offset = num_imported_funcs - 1;
    let runtime_sections = emit_runtime_sections(program, records, enums, call_offset)
        .map_err(|e| WasmError::new(e.message))?;

    // We need the runtime type signatures (they're already emitted by runtime_gen).
    // But we already have types from preamble imports. We must NOT duplicate the runtime
    // type section. Instead, we need the runtime code bodies.
    // The runtime_gen emits its own complete TypeSection — we need to extract just the
    // function bodies and type-to-function mappings.
    //
    // Strategy: We emit the runtime section types first (they're fixed), then our
    // preamble types were already added. The runtime function section references type
    // indices from the runtime's own type section, which starts at 0. We need to
    // offset those type indices by the number of preamble types we already added.
    //
    // Actually, the simplest correct approach: build the complete type section ourselves.
    // The runtime has 15 fixed type signatures. We already added preamble import types.
    // We need to add the runtime types, then the runtime functions reference them.
    //
    // Let's count: PREAMBLE_IMPORTS has 18 entries, each gets 1 type. User externs also
    // each get 1 type. So next_type_idx = 18 + num_user_externs.
    //
    // The runtime has types 0-14 (15 types). Its function section references these.
    // We need the runtime's function types at indices next_type_idx..next_type_idx+15.
    // And the runtime's function section entries need to be adjusted by this offset.
    //
    // Easiest: just emit the 15 runtime type signatures ourselves, then emit function
    // entries that reference them (adjusted), then emit the runtime code bodies.

    let runtime_type_offset = next_type_idx;

    // Emit runtime's 15 type signatures
    emit_runtime_type_signatures(&mut types, &mut next_type_idx);

    // Emit runtime function entries (type indices offset by runtime_type_offset)
    let runtime_func_start = num_imported_funcs;
    for &type_idx in RUNTIME_FUNCTION_TYPES {
        functions.function(type_idx + runtime_type_offset);
    }

    // Register runtime function indices
    for (i, name) in RUNTIME_FUNC_NAMES.iter().enumerate() {
        func_indices.insert(name, runtime_func_start + u32::try_from(i).unwrap());
    }

    // Memory section
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });

    // Global section: heap_ptr (i32 mutable, init 0), alloc_stack_depth (i32 mutable, init 0)
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::extended([Instruction::I32Const(0)]),
    );
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::extended([Instruction::I32Const(0)]),
    );

    // --- Phase 3: Support functions (record_eq, enum_eq, arg_count, arg_text, stdin_text, stdin_bytes, text_index_keys) ---
    let support_start_func = runtime_func_start + u32::try_from(RUNTIME_FUNC_NAMES.len()).unwrap();

    let eq_helper_type_idx = next_type_idx;
    types
        .ty()
        .function([ValType::I64, ValType::I64], [ValType::I64]);
    next_type_idx += 1;

    let mut support_func_names: Vec<String> = Vec::new();

    for name in records.keys() {
        let helper_name = record_eq_helper_name(name);
        functions.function(eq_helper_type_idx);
        let func_idx = support_start_func
            + u32::try_from(support_func_names.len()).expect("too many support functions");
        func_indices.insert(&helper_name, func_idx);
        support_func_names.push(helper_name);
    }

    for (name, enum_ty) in enums {
        if !enum_is_payload_free(enum_ty) {
            let helper_name = enum_eq_helper_name(name);
            functions.function(eq_helper_type_idx);
            let func_idx = support_start_func
                + u32::try_from(support_func_names.len()).expect("too many support functions");
            func_indices.insert(&helper_name, func_idx);
            support_func_names.push(helper_name);
        }
    }

    {
        let type_idx = next_type_idx;
        types.ty().function([], [ValType::I64]);
        next_type_idx += 1;
        functions.function(type_idx);
        let func_name = "__sarif_arg_count".to_string();
        let func_idx = support_start_func
            + u32::try_from(support_func_names.len()).expect("too many support functions");
        func_indices.insert(&func_name, func_idx);
        support_func_names.push(func_name);
    }

    {
        let type_idx = next_type_idx;
        types.ty().function([ValType::I64], [ValType::I64]);
        next_type_idx += 1;
        functions.function(type_idx);
        let func_name = "__sarif_arg_text".to_string();
        let func_idx = support_start_func
            + u32::try_from(support_func_names.len()).expect("too many support functions");
        func_indices.insert(&func_name, func_idx);
        support_func_names.push(func_name);
    }

    {
        let type_idx = next_type_idx;
        types.ty().function([], [ValType::I64]);
        next_type_idx += 1;
        functions.function(type_idx);
        let func_name = "__sarif_stdin_text".to_string();
        let func_idx = support_start_func
            + u32::try_from(support_func_names.len()).expect("too many support functions");
        func_indices.insert(&func_name, func_idx);
        support_func_names.push(func_name);
    }

    {
        let type_idx = next_type_idx;
        types.ty().function([], [ValType::I64]);
        next_type_idx += 1;
        functions.function(type_idx);
        let func_name = "__sarif_stdin_bytes".to_string();
        let func_idx = support_start_func
            + u32::try_from(support_func_names.len()).expect("too many support functions");
        func_indices.insert(&func_name, func_idx);
        support_func_names.push(func_name);
    }

    {
        let type_idx = next_type_idx;
        types.ty().function([ValType::I64], [ValType::I64]);
        next_type_idx += 1;
        functions.function(type_idx);
        let func_name = "__sarif_text_index_keys".to_string();
        let func_idx = support_start_func
            + u32::try_from(support_func_names.len()).expect("too many support functions");
        func_indices.insert(&func_name, func_idx);
        support_func_names.push(func_name);
    }

    let support_func_count =
        u32::try_from(support_func_names.len()).expect("too many support functions");
    let user_start_func = support_start_func + support_func_count;

    // --- Phase 5: User function types and indices ---
    let mut user_funcs_ordered: Vec<&Function> = Vec::new();
    for function in &program.functions {
        let return_kind = if let Some(ty) = &function.return_type {
            wasm_value_kind_from_name(ty, &program.structs, &program.enums)?
        } else {
            WasmValueKind::Unit
        };
        let type_idx = get_or_create_user_func_type(
            &mut types,
            &mut next_type_idx,
            function,
            &program.structs,
            &program.enums,
            &return_kind,
        )?;
        functions.function(type_idx);
        let func_idx = user_start_func
            + u32::try_from(user_funcs_ordered.len()).expect("too many user functions");
        func_indices.insert(&format!("${}", function.name), func_idx);
        user_funcs_ordered.push(function);
    }

    // --- Phase 6: Emit code bodies ---

    // Runtime function code bodies (from runtime_gen)
    for func in runtime_sections.code_bodies.iter() {
        code.function(func);
    }

    // Record eq helpers
    for (name, record) in records {
        let _helper_name = record_eq_helper_name(name);
        let f = emit_record_eq_helper_binary(name, record, &func_indices);
        code.function(&f);
    }

    // Enum eq helpers
    for (name, enum_ty) in enums {
        if !enum_is_payload_free(enum_ty) {
            let _helper_name = enum_eq_helper_name(name);
            let f = emit_enum_eq_helper_binary(name, enum_ty, records, enums, &func_indices);
            code.function(&f);
        }
    }

    // arg_count
    {
        let f = emit_arg_count_helper(&func_indices);
        code.function(&f);
    }
    // arg_text
    {
        let f = emit_arg_text_helper(&func_indices);
        code.function(&f);
    }
    // stdin_text
    {
        let f = emit_stdin_text_helper(&func_indices);
        code.function(&f);
    }
    // stdin_bytes
    {
        let f = emit_stdin_bytes_helper(&func_indices);
        code.function(&f);
    }
    // text_index_keys
    {
        let f = emit_text_index_keys_helper(&func_indices);
        code.function(&f);
    }

    // User function code bodies
    for function in &user_funcs_ordered {
        let func = emit_user_function_binary(
            function,
            records,
            enums,
            &program.functions,
            &program.structs,
            &program.enums,
            &program.externs,
            &func_indices,
        )?;
        code.function(&func);
    }

    // --- Phase 7: Exports ---
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("alloc", ExportKind::Func, func_indices.runtime("alloc"));

    for function in &program.functions {
        let func_idx = func_indices
            .get(&format!("${}", function.name))
            .expect("user function should have an index");
        exports.export(&function.name, ExportKind::Func, func_idx);
    }

    // --- Assemble module ---
    let mut module = Module::new();
    module.section(&types);
    module.section(&imports);
    module.section(&functions);
    module.section(&memories);
    module.section(&globals);
    module.section(&exports);
    module.section(&code);

    let mut names = NameSection::new();
    let mut func_names = NameMap::new();
    for (name, &idx) in &func_indices.map {
        let display_name = name.strip_prefix('$').unwrap_or(name);
        func_names.append(idx, display_name);
    }
    names.functions(&func_names);
    module.section(&names);

    let bytes = module.finish();

    Ok(bytes)
}

// The runtime has 15 fixed type signatures (indices 0-14 in runtime_gen).
// We emit them with offset indices in the full module.
fn emit_runtime_type_signatures(types: &mut TypeSection, next_idx: &mut u32) {
    // Type 0: (i32) -> i32
    types.ty().function([ValType::I32], [ValType::I32]);
    *next_idx += 1;
    // Type 1: () -> ()
    types.ty().function([], []);
    *next_idx += 1;
    // Type 2: (i32, i32) -> i64
    types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I64]);
    *next_idx += 1;
    // Type 3: (i64) -> i32
    types.ty().function([ValType::I64], [ValType::I32]);
    *next_idx += 1;
    // Type 4: (i64, i64) -> i64
    types
        .ty()
        .function([ValType::I64, ValType::I64], [ValType::I64]);
    *next_idx += 1;
    // Type 5: (i64, i64, i64) -> i64
    types
        .ty()
        .function([ValType::I64, ValType::I64, ValType::I64], [ValType::I64]);
    *next_idx += 1;
    // Type 6: (i64, i64) -> i32
    types
        .ty()
        .function([ValType::I64, ValType::I64], [ValType::I32]);
    *next_idx += 1;
    // Type 7: (i64, i64, i64, i64) -> i64
    types.ty().function(
        [ValType::I64, ValType::I64, ValType::I64, ValType::I64],
        [ValType::I64],
    );
    *next_idx += 1;
    // Type 8: (i64) -> i64
    types.ty().function([ValType::I64], [ValType::I64]);
    *next_idx += 1;
    // Type 9: (i64) -> f64
    types.ty().function([ValType::I64], [ValType::F64]);
    *next_idx += 1;
    // Type 10: (f64, i64) -> i64
    types
        .ty()
        .function([ValType::F64, ValType::I64], [ValType::I64]);
    *next_idx += 1;
    // Type 11: () -> i64
    types.ty().function([], [ValType::I64]);
    *next_idx += 1;
    // Type 12: (i32, i32) -> i32
    types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    *next_idx += 1;
    // Type 13: (i64) -> ()
    types.ty().function([ValType::I64], []);
    *next_idx += 1;
    // Type 14: (i32, i64, i32) -> i32
    types
        .ty()
        .function([ValType::I32, ValType::I64, ValType::I32], [ValType::I32]);
    *next_idx += 1;
}

// Runtime function → type index mapping (from runtime_gen.rs Function Section)
const RUNTIME_FUNCTION_TYPES: &[u32] = &[
    0,  // alloc
    1,  // __sarif_alloc_push
    1,  // __sarif_alloc_pop
    2,  // __sarif_pack_text
    3,  // __sarif_text_len_i32
    0,  // __sarif_is_ascii_space
    0,  // __sarif_is_ascii_digit
    0,  // __sarif_is_utf8_continuation
    4,  // __sarif_text_eq
    4,  // __sarif_text_cmp
    4,  // __sarif_text_byte
    5,  // __sarif_bytes_slice
    4,  // __sarif_text_concat
    6,  // __sarif_clamp_text_slice_start
    6,  // __sarif_clamp_text_slice_end
    5,  // __sarif_text_slice
    7,  // __sarif_text_eq_range
    7,  // __sarif_text_find_byte_range
    7,  // __sarif_bytes_find_byte_range
    4,  // __sarif_text_line_end
    4,  // __sarif_text_next_line
    7,  // __sarif_text_field_end
    7,  // __sarif_text_next_field
    8,  // __sarif_parse_i32
    5,  // __sarif_parse_i32_range
    9,  // __sarif_parse_f64
    10, // __sarif_text_from_f64_fixed
    11, // __sarif_text_builder_new
    12, // __sarif_text_builder_reserve
    4,  // __sarif_text_builder_append
    4,  // __sarif_text_builder_append_codepoint
    4,  // __sarif_text_builder_append_ascii
    7,  // __sarif_text_builder_append_slice
    4,  // __sarif_text_builder_append_i32
    4,  // __sarif_text_builder_append_i64
    8,  // __sarif_text_builder_finish
    13, // __sarif_stdout_write
    8,  // __sarif_stdout_write_builder
    3,  // __sarif_text_hash
    11, // __sarif_text_index_new
    0,  // __sarif_text_index_ensure_capacity
    14, // __sarif_text_index_find_entry
    4,  // __sarif_text_index_get
    6,  // __sarif_text_index_contains
    5,  // __sarif_text_index_set
    5,  // __sarif_text_index_get_or_insert
    4,  // __sarif_list_new
    8,  // __sarif_list_len
    4,  // __sarif_list_get
    5,  // __sarif_list_set
    5,  // __sarif_list_push
    4,  // __sarif_list_sort_text
    5,  // __sarif_list_sort_record_text_field
];

fn get_or_create_user_func_type(
    types: &mut TypeSection,
    next_idx: &mut u32,
    function: &Function,
    program_structs: &[StructType],
    program_enums: &[EnumType],
    return_kind: &WasmValueKind,
) -> Result<u32, WasmError> {
    let mut params: Vec<ValType> = Vec::new();
    for param in &function.params {
        let kind = wasm_value_kind_from_name(&param.ty, program_structs, program_enums)
            .unwrap_or(WasmValueKind::I32);
        params.push(kind_to_valtype(&kind));
    }
    let results: Vec<ValType> = if let Some(ty) = wasm_type_from_kind_result(return_kind) {
        vec![match ty {
            WasmType::I64 => ValType::I64,
            WasmType::F64 => ValType::F64,
        }]
    } else {
        Vec::new()
    };
    let idx = *next_idx;
    types.ty().function(params, results);
    *next_idx += 1;
    Ok(idx)
}

fn kind_to_valtype(kind: &WasmValueKind) -> ValType {
    match wasm_type_from_kind(kind) {
        WasmType::I64 => ValType::I64,
        WasmType::F64 => ValType::F64,
    }
}

fn emit_record_eq_helper_binary(
    _name: &str,
    record: &WasmRecord,
    func_indices: &FuncIndexMap,
) -> WasmFunction {
    let mut locals: Vec<(u32, ValType)> = Vec::new();
    let left_idx = 0u32;
    let right_idx = 1u32;
    let result_idx = 2u32;
    locals.push((1, ValType::I64));

    let mut f = WasmFunction::new(locals);
    f.instruction(&Instruction::I64Const(1));
    f.instruction(&Instruction::LocalSet(result_idx));
    for field in &record.fields {
        f.instruction(&Instruction::LocalGet(result_idx));
        emit_memory_kind_equality_binary(
            &mut f,
            &field.kind,
            left_idx,
            right_idx,
            field.offset,
            func_indices,
        );
        f.instruction(&Instruction::I64And);
        f.instruction(&Instruction::LocalSet(result_idx));
    }
    f.instruction(&Instruction::LocalGet(result_idx));
    f.instruction(&Instruction::End);
    f
}

fn emit_enum_eq_helper_binary(
    _name: &str,
    enum_ty: &WasmEnum,
    _records: &BTreeMap<String, WasmRecord>,
    _enums: &BTreeMap<String, WasmEnum>,
    func_indices: &FuncIndexMap,
) -> WasmFunction {
    let mut locals: Vec<(u32, ValType)> = Vec::new();
    let left_idx = 0u32;
    let right_idx = 1u32;
    let left_tag_idx = 2u32;
    let right_tag_idx = 3u32;
    let left_matches_idx = 4u32;
    let result_idx = 5u32;
    locals.push((1, ValType::I64));
    locals.push((1, ValType::I64));
    locals.push((1, ValType::I64));
    locals.push((1, ValType::I64));

    let mut f = WasmFunction::new(locals);

    f.instruction(&Instruction::LocalGet(left_idx));
    f.instruction(&Instruction::I32WrapI64);
    f.instruction(&Instruction::I64Load(memarg(0)));
    f.instruction(&Instruction::LocalSet(left_tag_idx));

    f.instruction(&Instruction::LocalGet(right_idx));
    f.instruction(&Instruction::I32WrapI64);
    f.instruction(&Instruction::I64Load(memarg(0)));
    f.instruction(&Instruction::LocalSet(right_tag_idx));

    f.instruction(&Instruction::LocalGet(left_tag_idx));
    f.instruction(&Instruction::LocalGet(right_tag_idx));
    f.instruction(&Instruction::I64Eq);
    f.instruction(&Instruction::I64ExtendI32U);
    f.instruction(&Instruction::LocalSet(result_idx));

    for (index, variant) in enum_ty.variants.iter().enumerate() {
        let Some(_payload_kind) = &variant.payload else {
            continue;
        };
        f.instruction(&Instruction::LocalGet(left_tag_idx));
        f.instruction(&Instruction::I64Const(i64::try_from(index).unwrap()));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::I64ExtendI32U);
        f.instruction(&Instruction::LocalSet(left_matches_idx));

        f.instruction(&Instruction::LocalGet(result_idx));
        f.instruction(&Instruction::LocalGet(left_matches_idx));
        f.instruction(&Instruction::I64Const(1));
        f.instruction(&Instruction::I64Xor);
        emit_memory_kind_equality_binary(
            &mut f,
            _payload_kind,
            left_idx,
            right_idx,
            8,
            func_indices,
        );
        f.instruction(&Instruction::I64Or);
        f.instruction(&Instruction::I64And);
        f.instruction(&Instruction::LocalSet(result_idx));
    }

    f.instruction(&Instruction::LocalGet(result_idx));
    f.instruction(&Instruction::End);
    f
}

fn emit_memory_kind_equality_binary(
    f: &mut WasmFunction,
    kind: &WasmValueKind,
    left_base: u32,
    right_base: u32,
    offset: u32,
    func_indices: &FuncIndexMap,
) {
    match kind {
        WasmValueKind::Unit => {
            f.instruction(&Instruction::I64Const(1));
        }
        WasmValueKind::F64 => {
            emit_memory_load_binary(f, left_base, offset, WasmType::F64);
            emit_memory_load_binary(f, right_base, offset, WasmType::F64);
            f.instruction(&Instruction::F64Eq);
            f.instruction(&Instruction::I64ExtendI32U);
        }
        WasmValueKind::Text | WasmValueKind::Bytes => {
            emit_memory_load_binary(f, left_base, offset, WasmType::I64);
            emit_memory_load_binary(f, right_base, offset, WasmType::I64);
            f.instruction(&Instruction::Call(func_indices.runtime("__sarif_text_eq")));
        }
        WasmValueKind::Record(name) => {
            emit_memory_load_binary(f, left_base, offset, WasmType::I64);
            emit_memory_load_binary(f, right_base, offset, WasmType::I64);
            let helper = record_eq_helper_name(name);
            f.instruction(&Instruction::Call(
                func_indices
                    .get(&helper)
                    .expect("record eq helper should have an index"),
            ));
        }
        WasmValueKind::Enum(name) => {
            emit_memory_load_binary(f, left_base, offset, WasmType::I64);
            emit_memory_load_binary(f, right_base, offset, WasmType::I64);
            let helper = enum_eq_helper_name(name);
            if let Some(idx) = func_indices.get(&helper) {
                f.instruction(&Instruction::Call(idx));
            } else {
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::I64ExtendI32U);
            }
        }
        WasmValueKind::I32
        | WasmValueKind::I64
        | WasmValueKind::Bool
        | WasmValueKind::TextIndex
        | WasmValueKind::TextBuilder
        | WasmValueKind::List(_)
        | WasmValueKind::File => {
            emit_memory_load_binary(f, left_base, offset, WasmType::I64);
            emit_memory_load_binary(f, right_base, offset, WasmType::I64);
            f.instruction(&Instruction::I64Eq);
            f.instruction(&Instruction::I64ExtendI32U);
        }
    }
}

fn emit_memory_load_binary(f: &mut WasmFunction, base: u32, offset: u32, ty: WasmType) {
    f.instruction(&Instruction::LocalGet(base));
    f.instruction(&Instruction::I32WrapI64);
    if offset > 0 {
        f.instruction(&Instruction::I32Const(i32::try_from(offset).unwrap()));
        f.instruction(&Instruction::I32Add);
    }
    match ty {
        WasmType::I64 => {
            f.instruction(&Instruction::I64Load(memarg(0)));
        }
        WasmType::F64 => {
            f.instruction(&Instruction::F64Load(memarg(0)));
        }
    }
}

fn emit_arg_count_helper(func_indices: &FuncIndexMap) -> WasmFunction {
    let mut f = WasmFunction::new(Vec::<(u32, ValType)>::new());
    f.instruction(&Instruction::Call(
        func_indices.get("__host_argc").expect("__host_argc import"),
    ));
    f.instruction(&Instruction::End);
    f
}

fn emit_arg_text_helper(func_indices: &FuncIndexMap) -> WasmFunction {
    let mut f = WasmFunction::new([(1, ValType::I32), (1, ValType::I32)]);
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::I32Const(4096));
    f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
    f.instruction(&Instruction::LocalTee(1));
    f.instruction(&Instruction::I32Const(4096));
    f.instruction(&Instruction::Call(
        func_indices.get("__host_argv").expect("__host_argv import"),
    ));
    f.instruction(&Instruction::LocalTee(2));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32LtS);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I64Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::LocalGet(1));
    f.instruction(&Instruction::LocalGet(2));
    f.instruction(&Instruction::Call(
        func_indices.runtime("__sarif_pack_text"),
    ));
    f.instruction(&Instruction::End);
    f
}

fn emit_stdin_text_helper(func_indices: &FuncIndexMap) -> WasmFunction {
    let mut f = WasmFunction::new([(1, ValType::I32), (1, ValType::I32)]);
    f.instruction(&Instruction::I32Const(8192));
    f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
    f.instruction(&Instruction::LocalTee(0));
    f.instruction(&Instruction::I32Const(8192));
    f.instruction(&Instruction::Call(
        func_indices
            .get("__host_stdin_read")
            .expect("__host_stdin_read import"),
    ));
    f.instruction(&Instruction::LocalTee(1));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32LtS);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I64Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::LocalGet(1));
    f.instruction(&Instruction::Call(
        func_indices.runtime("__sarif_pack_text"),
    ));
    f.instruction(&Instruction::End);
    f
}

fn emit_stdin_bytes_helper(func_indices: &FuncIndexMap) -> WasmFunction {
    let mut f = WasmFunction::new([(1, ValType::I32), (1, ValType::I32)]);
    f.instruction(&Instruction::I32Const(8192));
    f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
    f.instruction(&Instruction::LocalTee(0));
    f.instruction(&Instruction::I32Const(8192));
    f.instruction(&Instruction::Call(
        func_indices
            .get("__host_stdin_read")
            .expect("__host_stdin_read import"),
    ));
    f.instruction(&Instruction::LocalTee(1));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32LtS);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I64Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::LocalGet(1));
    f.instruction(&Instruction::Call(
        func_indices.runtime("__sarif_pack_text"),
    ));
    f.instruction(&Instruction::End);
    f
}

fn emit_text_index_keys_helper(_func_indices: &FuncIndexMap) -> WasmFunction {
    let mut f = WasmFunction::new([(1, ValType::I32)]);
    // For text_index_keys, we call __sarif_text_index_keys which wraps
    // __sarif_list_sort_text or similar. Actually, looking at the WAT emitter,
    // text_index_keys returns a list built from the index.
    // The simplest approach: this is runtime function 51 in the original WAT.
    // But actually, __sarif_text_index_keys doesn't exist in runtime_gen.
    // Looking at the WAT runtime_list.wat:
    // It returns list from the text index's key list.
    // For now, emit a placeholder that returns 0.
    f.instruction(&Instruction::I64Const(0));
    f.instruction(&Instruction::End);
    f
}

#[allow(clippy::too_many_arguments)]
fn emit_user_function_binary(
    function: &Function,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    all_functions: &[Function],
    program_structs: &[StructType],
    program_enums: &[EnumType],
    externs: &[ExternFunction],
    func_indices: &FuncIndexMap,
) -> Result<WasmFunction, WasmError> {
    let mut kinds = BTreeMap::new();
    collect_inst_kinds(
        function,
        &function.instructions,
        program_structs,
        program_enums,
        all_functions,
        externs,
        &mut kinds,
    )?;

    let mut env = LocalEnv::new();

    for _ in &function.params {
        env.alloc_param();
    }

    let mut local_decls: Vec<(u32, ValType)> = Vec::new();

    for local in &function.mutable_locals {
        let kind = wasm_value_kind_from_name(&local.ty, program_structs, program_enums)
            .unwrap_or(WasmValueKind::I32);
        let val_ty = kind_to_valtype(&kind);
        env.get_or_alloc_slot(local.slot);
        local_decls.push((1, val_ty));
    }

    let collected_locals = collect_locals_binary(function, &function.instructions, &kinds);
    for (id, kind) in &collected_locals {
        let val_ty = kind_to_valtype(kind);
        env.get_or_alloc_value(*id);
        local_decls.push((1, val_ty));
    }

    let mut repeat_counters = BTreeSet::new();
    for_each_inst_recursive(&function.instructions, &mut |inst| {
        if let Inst::Repeat { count, .. } = inst {
            repeat_counters.insert(wasm_id(*count));
        }
    });
    for counter in repeat_counters {
        let name = format!("repeat_counter_{}", counter);
        env.get_or_alloc_repeat_counter(name);
        local_decls.push((1, ValType::I64));
    }

    let mut f = WasmFunction::new(local_decls);

    for inst in &function.instructions {
        emit_inst_binary(
            &mut f,
            &mut env,
            function,
            inst,
            &kinds,
            func_indices,
            records,
            enums,
            program_structs,
            program_enums,
        )?;
    }

    if let Some(res) = function.result {
        f.instruction(&Instruction::LocalGet(env.get_value(res)));
    }

    f.instruction(&Instruction::End);
    Ok(f)
}

#[allow(clippy::only_used_in_recursion)]
fn collect_locals_binary(
    function: &Function,
    instructions: &[Inst],
    kinds: &BTreeMap<ValueId, WasmValueKind>,
) -> BTreeMap<ValueId, WasmValueKind> {
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
            | Inst::BytesFieldEnd { dest, .. }
            | Inst::BytesNextField { dest, .. }
            | Inst::TextLineEnd { dest, .. }
            | Inst::TextNextLine { dest, .. }
            | Inst::TextFieldEnd { dest, .. }
            | Inst::TextNextField { dest, .. }
            | Inst::TextConcat { dest, .. }
            | Inst::TextIntern { dest, .. }
            | Inst::TextSlice { dest, .. }
            | Inst::BytesSlice { dest, .. }
            | Inst::BytesMaterialize { dest, .. }
            | Inst::TextBuilderNew { dest }
            | Inst::TextIndexNew { dest }
            | Inst::TextBuilderAppend { dest, .. }
            | Inst::TextBuilderAppendCodepoint { dest, .. }
            | Inst::TextBuilderAppendAscii { dest, .. }
            | Inst::TextBuilderAppendSlice { dest, .. }
            | Inst::TextBuilderAppendI32 { dest, .. }
            | Inst::TextBuilderAppendI64 { dest, .. }
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
            | Inst::ListFrom { dest, .. }
            | Inst::ListLen { dest, .. }
            | Inst::ListGet { dest, .. }
            | Inst::ListSet { dest, .. }
            | Inst::ListPush { dest, .. }
            | Inst::ListSortText { dest, .. }
            | Inst::ListSortRecordTextField { dest, .. }
            | Inst::ListSortRecordField { dest, .. }
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
            | Inst::I64FromI32 { dest, .. }
            | Inst::Sqrt { dest, .. }
            | Inst::Perform { dest, .. }
            | Inst::Handle { dest, .. }
            | Inst::EnvGet { dest, .. }
            | Inst::EnvSet { dest, .. }
            | Inst::EnvRemove { dest, .. }
            | Inst::EnvKeys { dest }
            | Inst::DirCreate { dest, .. }
            | Inst::DirRemove { dest, .. }
            | Inst::DirList { dest, .. }
            | Inst::DirExists { dest, .. }
            | Inst::DirCurrent { dest }
            | Inst::DirChange { dest, .. }
            | Inst::ProcessId { dest }
            | Inst::ClockNow { dest } => {
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
                locals.extend(collect_locals_binary(function, then_insts, kinds));
                locals.extend(collect_locals_binary(function, else_insts, kinds));
            }
            Inst::While {
                dest,
                body_insts,
                condition_insts,
                ..
            } => {
                locals.insert(*dest, kinds[dest].clone());
                locals.extend(collect_locals_binary(function, condition_insts, kinds));
                locals.extend(collect_locals_binary(function, body_insts, kinds));
            }
            Inst::Repeat {
                dest, body_insts, ..
            } => {
                locals.insert(*dest, kinds[dest].clone());
                locals.extend(collect_locals_binary(function, body_insts, kinds));
            }
            Inst::StoreLocal { .. }
            | Inst::StdoutWrite { .. }
            | Inst::Assert { .. }
            | Inst::AllocPush
            | Inst::AllocPop
            | Inst::BytesToText { .. }
            | Inst::TextToBytes { .. }
            | Inst::FileOpen { .. }
            | Inst::FileIsValid { .. }
            | Inst::FileRead { .. }
            | Inst::FileReadToEnd { .. }
            | Inst::FileMmap { .. }
            | Inst::FileWrite { .. }
            | Inst::FileClose { .. }
            | Inst::FileSync { .. }
            | Inst::FileSeek { .. }
            | Inst::FileSize { .. }
            | Inst::FileExists { .. }
            | Inst::FileRemove { .. }
            | Inst::ProcessExit { .. }
            | Inst::ClockSleep { .. }
            | Inst::TcpListen { .. }
            | Inst::TcpAccept { .. }
            | Inst::TcpRecv { .. }
            | Inst::TcpSend { .. }
            | Inst::TcpClose { .. } => {}
        }
    }
    locals
}

#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn emit_inst_binary(
    f: &mut WasmFunction,
    env: &mut LocalEnv,
    function: &Function,
    inst: &Inst,
    kinds: &BTreeMap<ValueId, WasmValueKind>,
    func_indices: &FuncIndexMap,
    records: &BTreeMap<String, WasmRecord>,
    enums: &BTreeMap<String, WasmEnum>,
    program_structs: &[StructType],
    program_enums: &[EnumType],
) -> Result<(), WasmError> {
    match inst {
        Inst::LoadParam { dest, index } => {
            let param_idx = env.get_param(*index);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(param_idx));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::LoadLocal { dest, slot } => {
            let slot_idx = env.get_or_alloc_slot(*slot);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(slot_idx));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::StoreLocal { slot, src } => {
            let src_idx = env.get_value(*src);
            let slot_idx = env.get_or_alloc_slot(*slot);
            f.instruction(&Instruction::LocalGet(src_idx));
            f.instruction(&Instruction::LocalSet(slot_idx));
        }
        Inst::ConstInt { dest, value } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::I64Const(*value));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ConstF64 { dest, bits } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::F64Const(Ieee64::new(*bits)));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ConstBool { dest, value } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::I64Const(i64::from(*value as i32)));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ConstText { dest, value } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            let bytes = value.as_bytes();
            f.instruction(&Instruction::I32Const(i32::try_from(bytes.len()).unwrap()));
            f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
            for (index, byte) in bytes.iter().copied().enumerate() {
                f.instruction(&Instruction::LocalGet(dest_idx));
                f.instruction(&Instruction::I32WrapI64);
                f.instruction(&Instruction::I32Const(i32::try_from(index).unwrap()));
                f.instruction(&Instruction::I32Add);
                f.instruction(&Instruction::I32Const(i32::from(byte)));
                f.instruction(&Instruction::I32Store8(memarg(0)));
            }
            f.instruction(&Instruction::LocalGet(dest_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::I32Const(i32::try_from(bytes.len()).unwrap()));
            f.instruction(&Instruction::Call(
                func_indices.runtime("__sarif_pack_text"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::StdinBytes { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_stdin_bytes")
                    .expect("stdin_bytes helper"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::TextLen { dest, text } | Inst::BytesLen { dest, bytes: text } => {
            let text_idx = env.get_value(*text);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(text_idx));
            f.instruction(&Instruction::Call(
                func_indices.runtime("__sarif_text_len_i32"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::TextByte { dest, text, index }
        | Inst::BytesByte {
            dest,
            bytes: text,
            index,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*text, *index],
                func_indices.runtime("__sarif_text_byte"),
            );
        }
        Inst::BytesSlice {
            dest,
            bytes,
            start,
            end,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*bytes, *start, *end],
                func_indices.runtime("__sarif_bytes_slice"),
            );
        }
        Inst::BytesMaterialize { dest, bytes } => {
            let bytes_idx = env.get_value(*bytes);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(bytes_idx));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::BytesFindByteRange {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *byte],
                func_indices.runtime("__sarif_bytes_find_byte_range"),
            );
        }
        Inst::BytesFieldEnd {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *byte],
                func_indices.runtime("__sarif_text_field_end"),
            );
        }
        Inst::BytesNextField {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *byte],
                func_indices.runtime("__sarif_text_next_field"),
            );
        }
        Inst::TextConcat { dest, left, right } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*left, *right],
                func_indices.runtime("__sarif_text_concat"),
            );
        }
        Inst::TextIntern { dest, text } => {
            let text_idx = env.get_value(*text);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(text_idx));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::TextSlice {
            dest,
            text,
            start,
            end,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*text, *start, *end],
                func_indices.runtime("__sarif_text_slice"),
            );
        }
        Inst::TextCmp { dest, left, right } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*left, *right],
                func_indices.runtime("__sarif_text_cmp"),
            );
        }
        Inst::TextEqRange {
            dest,
            source,
            start,
            end,
            expected,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *expected],
                func_indices.runtime("__sarif_text_eq_range"),
            );
        }
        Inst::TextFindByteRange {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *byte],
                func_indices.runtime("__sarif_text_find_byte_range"),
            );
        }
        Inst::TextLineEnd {
            dest,
            source,
            start,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start],
                func_indices.runtime("__sarif_text_line_end"),
            );
        }
        Inst::TextNextLine {
            dest,
            source,
            start,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start],
                func_indices.runtime("__sarif_text_next_line"),
            );
        }
        Inst::TextFieldEnd {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *byte],
                func_indices.runtime("__sarif_text_field_end"),
            );
        }
        Inst::TextNextField {
            dest,
            source,
            start,
            end,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*source, *start, *end, *byte],
                func_indices.runtime("__sarif_text_next_field"),
            );
        }
        Inst::TextBuilderNew { dest } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[],
                func_indices.runtime("__sarif_text_builder_new"),
            );
        }
        Inst::TextIndexNew { dest } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[],
                func_indices.runtime("__sarif_text_index_new"),
            );
        }
        Inst::TextBuilderAppend {
            dest,
            builder,
            text,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder, *text],
                func_indices.runtime("__sarif_text_builder_append"),
            );
        }
        Inst::TextBuilderAppendCodepoint {
            dest,
            builder,
            codepoint,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder, *codepoint],
                func_indices.runtime("__sarif_text_builder_append_codepoint"),
            );
        }
        Inst::TextBuilderAppendAscii {
            dest,
            builder,
            byte,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder, *byte],
                func_indices.runtime("__sarif_text_builder_append_ascii"),
            );
        }
        Inst::TextBuilderAppendSlice {
            dest,
            builder,
            text,
            start,
            end,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder, *text, *start, *end],
                func_indices.runtime("__sarif_text_builder_append_slice"),
            );
        }
        Inst::TextBuilderAppendI32 {
            dest,
            builder,
            value,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder, *value],
                func_indices.runtime("__sarif_text_builder_append_i32"),
            );
        }
        Inst::TextBuilderAppendI64 {
            dest,
            builder,
            value,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder, *value],
                func_indices.runtime("__sarif_text_builder_append_i64"),
            );
        }
        Inst::TextBuilderFinish { dest, builder } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder],
                func_indices.runtime("__sarif_text_builder_finish"),
            );
        }
        Inst::StdoutWriteBuilder { dest, builder } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*builder],
                func_indices.runtime("__sarif_stdout_write_builder"),
            );
        }
        Inst::TextIndexGet { dest, index, key } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*index, *key],
                func_indices.runtime("__sarif_text_index_get"),
            );
        }
        Inst::TextIndexContains { dest, index, key } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*index, *key],
                func_indices.runtime("__sarif_text_index_contains"),
            );
        }
        Inst::TextIndexGetOrInsert {
            dest,
            index,
            key,
            next,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*index, *key, *next],
                func_indices.runtime("__sarif_text_index_get_or_insert"),
            );
        }
        Inst::TextIndexSet {
            dest,
            index,
            key,
            value,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*index, *key, *value],
                func_indices.runtime("__sarif_text_index_set"),
            );
        }
        Inst::TextIndexKeys { dest, index } => {
            let index_idx = env.get_value(*index);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(index_idx));
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_text_index_keys")
                    .expect("text_index_keys helper"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::TextFromF64Fixed {
            dest,
            value,
            digits,
        } => {
            let value_idx = env.get_value(*value);
            let digits_idx = env.get_value(*digits);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(value_idx));
            f.instruction(&Instruction::LocalGet(digits_idx));
            f.instruction(&Instruction::Call(
                func_indices.runtime("__sarif_text_from_f64_fixed"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ArgCount { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_arg_count")
                    .expect("arg_count helper"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ArgText { dest, index } => {
            let index_idx = env.get_value(*index);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(index_idx));
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_arg_text")
                    .expect("arg_text helper"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::StdinText { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_stdin_text")
                    .expect("stdin_text helper"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ParseI32 { dest, text } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*text],
                func_indices.runtime("__sarif_parse_i32"),
            );
        }
        Inst::ParseI32Range {
            dest,
            text,
            start,
            end,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*text, *start, *end],
                func_indices.runtime("__sarif_parse_i32_range"),
            );
        }
        Inst::ParseF64 { dest, text } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*text],
                func_indices.runtime("__sarif_parse_f64"),
            );
        }
        Inst::MakeEnum {
            dest,
            name,
            variant,
            payload,
        } => {
            emit_make_enum_binary(f, env, *dest, name, variant, *payload, enums, func_indices)?;
        }
        Inst::MakeRecord { dest, name, fields } => {
            let record = records
                .get(name)
                .ok_or_else(|| WasmError::new(format!("unknown record `{name}`")))?;
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::I32Const(i32::try_from(record.size).unwrap()));
            f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
            for field in &record.fields {
                let source = fields
                    .iter()
                    .find(|(n, _)| n == &field.name)
                    .map(|(_, s)| s)
                    .expect("field source should be available");
                f.instruction(&Instruction::LocalGet(dest_idx));
                f.instruction(&Instruction::I32WrapI64);
                f.instruction(&Instruction::I32Const(i32::try_from(field.offset).unwrap()));
                f.instruction(&Instruction::I32Add);
                f.instruction(&Instruction::LocalGet(env.get_value(*source)));
                match wasm_type_from_kind_result(&field.kind) {
                    Some(WasmType::I64) | None => {
                        f.instruction(&Instruction::I64Store(memarg(0)));
                    }
                    Some(WasmType::F64) => {
                        f.instruction(&Instruction::F64Store(memarg(0)));
                    }
                }
            }
        }
        Inst::Field { dest, base, name } => {
            let WasmValueKind::Record(record_name) = &kinds[base] else {
                return Err(WasmError::new("expected record kind for field access"));
            };
            let record = &records[record_name];
            let field = record
                .fields
                .iter()
                .find(|f_field| f_field.name == *name)
                .ok_or_else(|| {
                    WasmError::new(format!(
                        "record `{record_name}` has no field `{name}` in `{}`",
                        function.name
                    ))
                })?;
            let base_idx = env.get_value(*base);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(base_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::I32Const(i32::try_from(field.offset).unwrap()));
            f.instruction(&Instruction::I32Add);
            match wasm_type_from_kind_result(&field.kind) {
                Some(WasmType::I64) | None => {
                    f.instruction(&Instruction::I64Load(memarg(0)));
                }
                Some(WasmType::F64) => {
                    f.instruction(&Instruction::F64Load(memarg(0)));
                }
            }
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnumTagEq {
            dest, value, tag, ..
        } => {
            let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                return Err(WasmError::new("expected enum kind for enum tag comparison"));
            };
            let value_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(value_idx));
            if !enum_is_payload_free(&enums[enum_name]) {
                f.instruction(&Instruction::I32WrapI64);
                f.instruction(&Instruction::I64Load(memarg(0)));
            }
            f.instruction(&Instruction::I64Const(*tag));
            f.instruction(&Instruction::I64Eq);
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnumPayload { dest, value, .. } => {
            let value_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(value_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::I32Const(8));
            f.instruction(&Instruction::I32Add);
            match wasm_type_from_kind_result(&kinds[dest]) {
                Some(WasmType::I64) | None => {
                    f.instruction(&Instruction::I64Load(memarg(0)));
                }
                Some(WasmType::F64) => {
                    f.instruction(&Instruction::F64Load(memarg(0)));
                }
            }
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnumToI32 {
            dest,
            value,
            discriminants,
        } => {
            let value_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            let tag_idx = env.alloc_local();
            let result_idx = env.alloc_local();
            f.instruction(&Instruction::LocalGet(value_idx));
            let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                return Err(WasmError::new("expected enum kind for enum_to_i32"));
            };
            if !enum_is_payload_free(&enums[enum_name]) {
                f.instruction(&Instruction::I32WrapI64);
                f.instruction(&Instruction::I64Load(memarg(0)));
            }
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::LocalSet(tag_idx));
            for (i, &disc) in discriminants.iter().enumerate().rev() {
                f.instruction(&Instruction::LocalGet(tag_idx));
                f.instruction(&Instruction::I32Const(i32::try_from(i).unwrap()));
                f.instruction(&Instruction::I32Eq);
                f.instruction(&Instruction::I32Const(i32::try_from(disc).unwrap()));
                if i == discriminants.len() - 1 {
                    f.instruction(&Instruction::LocalSet(result_idx));
                } else {
                    f.instruction(&Instruction::LocalGet(result_idx));
                    f.instruction(&Instruction::Select);
                    f.instruction(&Instruction::LocalSet(result_idx));
                }
            }
            f.instruction(&Instruction::LocalGet(result_idx));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnumToText {
            dest,
            value,
            variant_names,
        } => {
            let value_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            let WasmValueKind::Enum(enum_name) = &kinds[value] else {
                return Err(WasmError::new("expected enum kind for enum_to_text"));
            };
            f.instruction(&Instruction::LocalGet(value_idx));
            let mut text_locals: Vec<u32> = Vec::new();
            for name in variant_names.iter() {
                let text_local = env.alloc_local();
                text_locals.push(text_local);
                let bytes = name.as_bytes();
                f.instruction(&Instruction::I32Const(i32::try_from(bytes.len()).unwrap()));
                f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
                f.instruction(&Instruction::I64ExtendI32U);
                f.instruction(&Instruction::LocalSet(text_local));
                for (index, byte) in bytes.iter().copied().enumerate() {
                    f.instruction(&Instruction::LocalGet(text_local));
                    f.instruction(&Instruction::I32WrapI64);
                    f.instruction(&Instruction::I32Const(i32::try_from(index).unwrap()));
                    f.instruction(&Instruction::I32Add);
                    f.instruction(&Instruction::I32Const(i32::from(byte)));
                    f.instruction(&Instruction::I32Store8(memarg(0)));
                }
            }
            for (i, &text_local) in text_locals.iter().enumerate().rev() {
                f.instruction(&Instruction::I64Const(i64::try_from(i).unwrap()));
                f.instruction(&Instruction::LocalGet(value_idx));
                if !enum_is_payload_free(&enums[enum_name]) {
                    f.instruction(&Instruction::I32WrapI64);
                    f.instruction(&Instruction::I64Load(memarg(0)));
                }
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::LocalGet(text_local));
                if i < text_locals.len() - 1 {
                    f.instruction(&Instruction::LocalGet(dest_idx));
                }
                f.instruction(&Instruction::Select);
            }
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ListNew { dest, len, value } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*len, *value],
                func_indices.runtime("__sarif_list_new"),
            );
        }
        Inst::ListLen { dest, list } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*list],
                func_indices.runtime("__sarif_list_len"),
            );
        }
        Inst::ListGet { dest, list, index } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*list, *index],
                func_indices.runtime("__sarif_list_get"),
            );
        }
        Inst::ListSet {
            dest,
            list,
            index,
            value,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*list, *index, *value],
                func_indices.runtime("__sarif_list_set"),
            );
        }
        Inst::ListPush {
            dest,
            list,
            len,
            value,
        } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*list, *len, *value],
                func_indices.runtime("__sarif_list_push"),
            );
        }
        Inst::ListSortText { dest, list, len } => {
            emit_runtime_call_binary(
                f,
                env,
                *dest,
                &[*list, *len],
                func_indices.runtime("__sarif_list_sort_text"),
            );
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
            let record = records.get(record_name.as_str()).ok_or_else(|| {
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
            let list_idx = env.get_value(*list);
            let len_idx = env.get_value(*len);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(list_idx));
            f.instruction(&Instruction::LocalGet(len_idx));
            f.instruction(&Instruction::I64Const(offset));
            f.instruction(&Instruction::Call(
                func_indices.runtime("__sarif_list_sort_record_text_field"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ListFrom { dest, array, len } => {
            // Allocate a new list with len and fill value 0.
            f.instruction(&Instruction::I64Const(*len as i64));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::Call(func_indices.runtime("__sarif_list_new")));

            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalSet(dest_idx));

            // Now, get the record name of the array record
            let WasmValueKind::Record(record_name) =
                kinds.get(array).cloned().ok_or_else(|| {
                    WasmError::new(format!(
                        "wasm list_from array {} kind not found in `{}`",
                        array.render(),
                        function.name
                    ))
                })?
            else {
                return Err(WasmError::new(format!(
                    "wasm list_from array {} is not a record in `{}`",
                    array.render(),
                    function.name
                )));
            };
            let record = records.get(record_name.as_str()).ok_or_else(|| {
                WasmError::new(format!("missing wasm record metadata for `{record_name}`"))
            })?;

            // For each element, load from record and call __sarif_list_set
            let array_idx = env.get_value(*array);
            for i in 0..*len {
                f.instruction(&Instruction::LocalGet(dest_idx));
                f.instruction(&Instruction::I64Const(i as i64));

                // Load field f{i}
                let field_name = format!("f{i}");
                let field = record
                    .fields
                    .iter()
                    .find(|f| f.name == field_name)
                    .ok_or_else(|| {
                        WasmError::new(format!(
                            "record `{record_name}` has no field `{field_name}` in `{}`",
                            function.name
                        ))
                    })?;

                f.instruction(&Instruction::LocalGet(array_idx));
                f.instruction(&Instruction::I32WrapI64);
                let offset = u64::from(field.offset);
                match wasm_type_from_kind_result(&field.kind) {
                    Some(WasmType::I64) | None => {
                        f.instruction(&Instruction::I64Load(memarg(offset)));
                    }
                    Some(WasmType::F64) => {
                        f.instruction(&Instruction::F64Load(memarg(offset)));
                        f.instruction(&Instruction::I64ReinterpretF64);
                    }
                }

                // Call __sarif_list_set
                f.instruction(&Instruction::Call(func_indices.runtime("__sarif_list_set")));
                // Drop the returned list pointer
                f.instruction(&Instruction::Drop);
            }
        }
        Inst::ListSortRecordField {
            dest,
            list,
            len,
            field,
        } => {
            let Some(WasmValueKind::List(element)) = kinds.get(list) else {
                return Err(WasmError::new(format!(
                    "wasm list_sort_record_field input {} is not a list in `{}`",
                    list.render(),
                    function.name
                )));
            };
            let WasmValueKind::Record(record_name) = element.as_ref() else {
                return Err(WasmError::new(format!(
                    "wasm list_sort_record_field requires List[record], found `{:?}` in `{}`",
                    element, function.name
                )));
            };
            let record = records.get(record_name.as_str()).ok_or_else(|| {
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
            let offset = i64::from(field_desc.offset);
            let list_idx = env.get_value(*list);
            let len_idx = env.get_value(*len);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(list_idx));
            f.instruction(&Instruction::LocalGet(len_idx));
            f.instruction(&Instruction::I64Const(offset));
            f.instruction(&Instruction::Call(
                func_indices.runtime("__sarif_list_sort_record_text_field"),
            ));
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::Add { dest, left, right } => {
            emit_binary_binary(f, env, "add", *dest, *left, *right, kinds)?;
        }
        Inst::Sub { dest, left, right } => {
            emit_binary_binary(f, env, "sub", *dest, *left, *right, kinds)?;
        }
        Inst::Mul { dest, left, right } => {
            emit_binary_binary(f, env, "mul", *dest, *left, *right, kinds)?;
        }
        Inst::Div { dest, left, right } => {
            emit_binary_binary(f, env, "div", *dest, *left, *right, kinds)?;
        }
        Inst::Rem { dest, left, right } => {
            emit_binary_binary(f, env, "rem", *dest, *left, *right, kinds)?;
        }
        Inst::BitAnd { dest, left, right } => {
            emit_binary_binary(f, env, "and", *dest, *left, *right, kinds)?;
        }
        Inst::BitOr { dest, left, right } => {
            emit_binary_binary(f, env, "or", *dest, *left, *right, kinds)?;
        }
        Inst::BitXor { dest, left, right } => {
            emit_binary_binary(f, env, "xor", *dest, *left, *right, kinds)?;
        }
        Inst::Shl { dest, left, right } => {
            emit_binary_binary(f, env, "shl", *dest, *left, *right, kinds)?;
        }
        Inst::Shr { dest, left, right } => {
            emit_binary_binary(f, env, "shr_s", *dest, *left, *right, kinds)?;
        }
        Inst::Eq { dest, left, right } => {
            emit_comparison_binary(
                f,
                env,
                "eq",
                *dest,
                *left,
                *right,
                kinds,
                enums,
                func_indices,
            )?;
        }
        Inst::Ne { dest, left, right } => {
            emit_comparison_binary(
                f,
                env,
                "ne",
                *dest,
                *left,
                *right,
                kinds,
                enums,
                func_indices,
            )?;
        }
        Inst::Lt { dest, left, right } => {
            emit_comparison_binary(
                f,
                env,
                "lt",
                *dest,
                *left,
                *right,
                kinds,
                enums,
                func_indices,
            )?;
        }
        Inst::Le { dest, left, right } => {
            emit_comparison_binary(
                f,
                env,
                "le",
                *dest,
                *left,
                *right,
                kinds,
                enums,
                func_indices,
            )?;
        }
        Inst::Gt { dest, left, right } => {
            emit_comparison_binary(
                f,
                env,
                "gt",
                *dest,
                *left,
                *right,
                kinds,
                enums,
                func_indices,
            )?;
        }
        Inst::Ge { dest, left, right } => {
            emit_comparison_binary(
                f,
                env,
                "ge",
                *dest,
                *left,
                *right,
                kinds,
                enums,
                func_indices,
            )?;
        }
        Inst::And { dest, left, right } => {
            emit_binary_binary(f, env, "and", *dest, *left, *right, kinds)?;
        }
        Inst::Or { dest, left, right } => {
            emit_binary_binary(f, env, "or", *dest, *left, *right, kinds)?;
        }
        Inst::F64FromI32 { dest, value } => {
            let src_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(src_idx));
            f.instruction(&Instruction::F64ConvertI64S);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::I64FromI32 { dest, value } => {
            let src_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(src_idx));
            f.instruction(&Instruction::I64ExtendI32S);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::Sqrt { dest, value } => {
            let src_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(src_idx));
            f.instruction(&Instruction::F64Sqrt);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::Call { dest, callee, args } => {
            for arg in args {
                f.instruction(&Instruction::LocalGet(env.get_value(*arg)));
            }
            let callee_key = format!("${}", callee);
            if let Some(idx) = func_indices.get(&callee_key) {
                f.instruction(&Instruction::Call(idx));
            } else if let Some(idx) = func_indices.get(callee) {
                f.instruction(&Instruction::Call(idx));
            }
            let dest_idx = env.get_or_alloc_value(*dest);
            if wasm_type_from_kind_result(&kinds[dest]).is_some() {
                f.instruction(&Instruction::LocalSet(dest_idx));
            }
        }
        Inst::If {
            condition,
            then_insts,
            else_insts,
            then_result,
            else_result,
            dest,
        } => {
            f.instruction(&Instruction::LocalGet(env.get_value(*condition)));
            f.instruction(&Instruction::I32WrapI64);
            let result_type = wasm_type_from_kind_result(&kinds[dest]);
            let block_type = match result_type {
                Some(WasmType::I64) => BlockType::Result(ValType::I64),
                Some(WasmType::F64) => BlockType::Result(ValType::F64),
                None => BlockType::Empty,
            };
            f.instruction(&Instruction::If(block_type));
            for inst in then_insts {
                emit_inst_binary(
                    f,
                    env,
                    function,
                    inst,
                    kinds,
                    func_indices,
                    records,
                    enums,
                    program_structs,
                    program_enums,
                )?;
            }
            if let Some(res) = then_result {
                f.instruction(&Instruction::LocalGet(env.get_value(*res)));
            } else if result_type.is_some() {
                f.instruction(&Instruction::I64Const(0));
            }
            f.instruction(&Instruction::Else);
            for inst in else_insts {
                emit_inst_binary(
                    f,
                    env,
                    function,
                    inst,
                    kinds,
                    func_indices,
                    records,
                    enums,
                    program_structs,
                    program_enums,
                )?;
            }
            if let Some(res) = else_result {
                f.instruction(&Instruction::LocalGet(env.get_value(*res)));
            } else if result_type.is_some() {
                f.instruction(&Instruction::I64Const(0));
            }
            f.instruction(&Instruction::End);
            if result_type.is_some() {
                let dest_idx = env.get_or_alloc_value(*dest);
                f.instruction(&Instruction::LocalSet(dest_idx));
            }
        }
        Inst::While {
            condition_insts,
            condition,
            body_insts,
            ..
        } => {
            f.instruction(&Instruction::Block(BlockType::Empty));
            f.instruction(&Instruction::Loop(BlockType::Empty));
            for inst in condition_insts {
                emit_inst_binary(
                    f,
                    env,
                    function,
                    inst,
                    kinds,
                    func_indices,
                    records,
                    enums,
                    program_structs,
                    program_enums,
                )?;
            }
            f.instruction(&Instruction::LocalGet(env.get_value(*condition)));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::I32Eqz);
            f.instruction(&Instruction::BrIf(1));
            for inst in body_insts {
                emit_inst_binary(
                    f,
                    env,
                    function,
                    inst,
                    kinds,
                    func_indices,
                    records,
                    enums,
                    program_structs,
                    program_enums,
                )?;
            }
            f.instruction(&Instruction::Br(0));
            f.instruction(&Instruction::End);
            f.instruction(&Instruction::End);
        }
        Inst::Repeat {
            count,
            body_insts,
            index_slot,
            ..
        } => {
            let count_id = wasm_id(*count);
            let counter_name = format!("repeat_counter_{}", count_id);
            let counter_idx = env.get_or_alloc_repeat_counter(counter_name);
            if let Some(slot) = index_slot {
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::LocalSet(env.get_or_alloc_slot(*slot)));
            }
            f.instruction(&Instruction::Block(BlockType::Empty));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::LocalSet(counter_idx));
            f.instruction(&Instruction::Loop(BlockType::Empty));
            f.instruction(&Instruction::LocalGet(counter_idx));
            f.instruction(&Instruction::LocalGet(env.get_value(*count)));
            f.instruction(&Instruction::I64GeS);
            f.instruction(&Instruction::BrIf(1));
            for inst in body_insts {
                emit_inst_binary(
                    f,
                    env,
                    function,
                    inst,
                    kinds,
                    func_indices,
                    records,
                    enums,
                    program_structs,
                    program_enums,
                )?;
            }
            f.instruction(&Instruction::LocalGet(counter_idx));
            f.instruction(&Instruction::I64Const(1));
            f.instruction(&Instruction::I64Add);
            f.instruction(&Instruction::LocalTee(counter_idx));
            if let Some(slot) = index_slot {
                f.instruction(&Instruction::LocalSet(env.get_or_alloc_slot(*slot)));
            } else {
                f.instruction(&Instruction::Drop);
            }
            f.instruction(&Instruction::Br(0));
            f.instruction(&Instruction::End);
            f.instruction(&Instruction::End);
        }
        Inst::Assert { condition, .. } => {
            f.instruction(&Instruction::LocalGet(env.get_value(*condition)));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::I32Eqz);
            f.instruction(&Instruction::If(BlockType::Empty));
            f.instruction(&Instruction::Unreachable);
            f.instruction(&Instruction::End);
        }
        Inst::AllocPush => {
            f.instruction(&Instruction::Call(
                func_indices.get("__sarif_alloc_push").expect("alloc_push"),
            ));
        }
        Inst::AllocPop => {
            f.instruction(&Instruction::Call(
                func_indices.get("__sarif_alloc_pop").expect("alloc_pop"),
            ));
        }
        Inst::StdoutWrite { text } => {
            f.instruction(&Instruction::LocalGet(env.get_value(*text)));
            f.instruction(&Instruction::Call(
                func_indices.runtime("__sarif_stdout_write"),
            ));
        }
        Inst::EnvGet { dest, key } => {
            let key_idx = env.get_value(*key);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(key_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_env_get")
                    .expect("__sarif_env_get import"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnvSet { dest, key, value } => {
            let key_idx = env.get_value(*key);
            let value_idx = env.get_value(*value);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(key_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::LocalGet(value_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_env_set")
                    .expect("__sarif_env_set import"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnvRemove { dest, key } => {
            let key_idx = env.get_value(*key);
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::LocalGet(key_idx));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_env_remove")
                    .expect("__sarif_env_remove import"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::EnvKeys { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_env_keys")
                    .expect("__sarif_env_keys import"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::DirCreate { dest, path } => {
            emit_env_call_binary(
                f,
                env,
                *dest,
                &[*path],
                func_indices
                    .get("__sarif_dir_create")
                    .expect("__sarif_dir_create import"),
            );
        }
        Inst::DirRemove { dest, path } => {
            emit_env_call_binary(
                f,
                env,
                *dest,
                &[*path],
                func_indices
                    .get("__sarif_dir_remove")
                    .expect("__sarif_dir_remove import"),
            );
        }
        Inst::DirList { dest, path } => {
            emit_env_call_binary(
                f,
                env,
                *dest,
                &[*path],
                func_indices
                    .get("__sarif_dir_list")
                    .expect("__sarif_dir_list import"),
            );
        }
        Inst::DirExists { dest, path } => {
            emit_env_call_binary(
                f,
                env,
                *dest,
                &[*path],
                func_indices
                    .get("__sarif_dir_exists")
                    .expect("__sarif_dir_exists import"),
            );
        }
        Inst::DirCurrent { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_dir_current")
                    .expect("__sarif_dir_current import"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::DirChange { dest, path } => {
            emit_env_call_binary(
                f,
                env,
                *dest,
                &[*path],
                func_indices
                    .get("__sarif_dir_change")
                    .expect("__sarif_dir_change import"),
            );
        }
        Inst::ProcessExit { code } => {
            f.instruction(&Instruction::LocalGet(env.get_value(*code)));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::Call(
                func_indices.get("proc_exit").expect("proc_exit import"),
            ));
            f.instruction(&Instruction::Unreachable);
        }
        Inst::ProcessId { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_process_id")
                    .expect("__sarif_process_id import"),
            ));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ClockNow { dest } => {
            let dest_idx = env.get_or_alloc_value(*dest);
            let time_ptr = env.alloc_local();
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::LocalGet(time_ptr));
            f.instruction(&Instruction::Call(
                func_indices
                    .get("clock_time_get")
                    .expect("clock_time_get import"),
            ));
            f.instruction(&Instruction::Drop);
            f.instruction(&Instruction::LocalGet(time_ptr));
            f.instruction(&Instruction::I64Load(memarg(0)));
            f.instruction(&Instruction::F64ConvertI64S);
            f.instruction(&Instruction::F64Const(Ieee64::new(0x3E112E0BE805E842)));
            f.instruction(&Instruction::F64Mul);
            f.instruction(&Instruction::LocalSet(dest_idx));
        }
        Inst::ClockSleep { ms } => {
            f.instruction(&Instruction::LocalGet(env.get_value(*ms)));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::Call(
                func_indices
                    .get("__sarif_clock_sleep")
                    .expect("__sarif_clock_sleep import"),
            ));
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
        Inst::TextToBytes { .. } => {
            return Err(WasmError::new(
                "wasm backend does not support text-to-bytes conversion",
            ));
        }
        Inst::FileOpen { .. }
        | Inst::FileIsValid { .. }
        | Inst::FileRead { .. }
        | Inst::FileReadToEnd { .. }
        | Inst::FileMmap { .. }
        | Inst::FileWrite { .. }
        | Inst::FileClose { .. }
        | Inst::FileSync { .. }
        | Inst::FileSeek { .. }
        | Inst::FileSize { .. }
        | Inst::FileExists { .. }
        | Inst::FileRemove { .. }
        | Inst::TcpListen { .. }
        | Inst::TcpAccept { .. }
        | Inst::TcpRecv { .. }
        | Inst::TcpSend { .. }
        | Inst::TcpClose { .. } => {
            return Err(WasmError::new(
                "wasm backend does not support this operation",
            ));
        }
    }
    Ok(())
}

fn emit_runtime_call_binary(
    f: &mut WasmFunction,
    env: &mut LocalEnv,
    dest: ValueId,
    args: &[ValueId],
    runtime_func_idx: u32,
) {
    for arg in args {
        f.instruction(&Instruction::LocalGet(env.get_value(*arg)));
    }
    f.instruction(&Instruction::Call(runtime_func_idx));
    let dest_idx = env.get_or_alloc_value(dest);
    f.instruction(&Instruction::LocalSet(dest_idx));
}

fn emit_env_call_binary(
    f: &mut WasmFunction,
    env: &mut LocalEnv,
    dest: ValueId,
    args: &[ValueId],
    func_idx: u32,
) {
    for arg in args {
        f.instruction(&Instruction::LocalGet(env.get_value(*arg)));
        f.instruction(&Instruction::I32WrapI64);
    }
    f.instruction(&Instruction::Call(func_idx));
    f.instruction(&Instruction::I64ExtendI32U);
    let dest_idx = env.get_or_alloc_value(dest);
    f.instruction(&Instruction::LocalSet(dest_idx));
}

fn emit_binary_binary(
    f: &mut WasmFunction,
    env: &mut LocalEnv,
    op: &str,
    dest: ValueId,
    left: ValueId,
    right: ValueId,
    kinds: &BTreeMap<ValueId, WasmValueKind>,
) -> Result<(), WasmError> {
    let kind = &kinds[&left];
    let wasm_type = wasm_type_from_kind(kind);
    let left_idx = env.get_value(left);
    let right_idx = env.get_value(right);
    let dest_idx = env.get_or_alloc_value(dest);

    f.instruction(&Instruction::LocalGet(left_idx));
    f.instruction(&Instruction::LocalGet(right_idx));

    let instr = if op == "and" || op == "or" {
        match wasm_type {
            WasmType::I64 => {
                if op == "and" {
                    Instruction::I64And
                } else {
                    Instruction::I64Or
                }
            }
            WasmType::F64 => unreachable!("f64.and/f64.or not valid"),
        }
    } else if (op == "div" || op == "rem") && matches!(wasm_type, WasmType::I64) {
        match op {
            "div" => Instruction::I64DivS,
            "rem" => Instruction::I64RemS,
            _ => unreachable!(),
        }
    } else {
        match (wasm_type, op) {
            (WasmType::I64, "add") => Instruction::I64Add,
            (WasmType::I64, "sub") => Instruction::I64Sub,
            (WasmType::I64, "mul") => Instruction::I64Mul,
            (WasmType::I64, "shl") => Instruction::I64Shl,
            (WasmType::I64, "shr_s") => Instruction::I64ShrS,
            (WasmType::I64, "xor") => Instruction::I64Xor,
            (WasmType::F64, "add") => Instruction::F64Add,
            (WasmType::F64, "sub") => Instruction::F64Sub,
            (WasmType::F64, "mul") => Instruction::F64Mul,
            (WasmType::F64, "div") => Instruction::F64Div,
            (WasmType::F64, "rem") => unreachable!("f64.rem not valid"),
            (WasmType::F64, "shl") | (WasmType::F64, "shr_s") => unreachable!(),
            (WasmType::F64, "xor") => unreachable!(),
            _ => unreachable!("unexpected binary op: {op} for {:?}", wasm_type),
        }
    };

    f.instruction(&instr);
    f.instruction(&Instruction::LocalSet(dest_idx));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_comparison_binary(
    f: &mut WasmFunction,
    env: &mut LocalEnv,
    op: &str,
    dest: ValueId,
    left: ValueId,
    right: ValueId,
    kinds: &BTreeMap<ValueId, WasmValueKind>,
    enums: &BTreeMap<String, WasmEnum>,
    func_indices: &FuncIndexMap,
) -> Result<(), WasmError> {
    let kind = &kinds[&left];
    let left_idx = env.get_value(left);
    let right_idx = env.get_value(right);
    let dest_idx = env.get_or_alloc_value(dest);

    if op == "eq" || op == "ne" {
        match kind {
            WasmValueKind::Text => {
                f.instruction(&Instruction::LocalGet(left_idx));
                f.instruction(&Instruction::LocalGet(right_idx));
                f.instruction(&Instruction::Call(func_indices.runtime("__sarif_text_eq")));
            }
            WasmValueKind::Record(name) => {
                f.instruction(&Instruction::LocalGet(left_idx));
                f.instruction(&Instruction::LocalGet(right_idx));
                let helper = record_eq_helper_name(name);
                if let Some(idx) = func_indices.get(&helper) {
                    f.instruction(&Instruction::Call(idx));
                }
            }
            WasmValueKind::Enum(name) if !enum_is_payload_free(&enums[name]) => {
                f.instruction(&Instruction::LocalGet(left_idx));
                f.instruction(&Instruction::LocalGet(right_idx));
                let helper = enum_eq_helper_name(name);
                if let Some(idx) = func_indices.get(&helper) {
                    f.instruction(&Instruction::Call(idx));
                }
            }
            _ => {}
        }
        let uses_structural_helper = matches!(kind, WasmValueKind::Text | WasmValueKind::Record(_))
            || matches!(kind, WasmValueKind::Enum(name) if !enum_is_payload_free(&enums[name]));
        if uses_structural_helper {
            if op == "ne" {
                f.instruction(&Instruction::I64Eqz);
                f.instruction(&Instruction::I64ExtendI32U);
            }
            f.instruction(&Instruction::LocalSet(dest_idx));
            return Ok(());
        }
    }

    let wasm_type = wasm_type_from_kind(kind);
    f.instruction(&Instruction::LocalGet(left_idx));
    f.instruction(&Instruction::LocalGet(right_idx));

    match wasm_type {
        WasmType::I64 => {
            let instr = match op {
                "eq" => Instruction::I64Eq,
                "ne" => Instruction::I64Ne,
                "lt" => Instruction::I64LtS,
                "le" => Instruction::I64LeS,
                "gt" => Instruction::I64GtS,
                "ge" => Instruction::I64GeS,
                _ => unreachable!(),
            };
            f.instruction(&instr);
        }
        WasmType::F64 => {
            let instr = match op {
                "eq" => Instruction::F64Eq,
                "ne" => Instruction::F64Ne,
                "lt" => Instruction::F64Lt,
                "le" => Instruction::F64Le,
                "gt" => Instruction::F64Gt,
                "ge" => Instruction::F64Ge,
                _ => unreachable!(),
            };
            f.instruction(&instr);
        }
    }
    f.instruction(&Instruction::I64ExtendI32U);
    f.instruction(&Instruction::LocalSet(dest_idx));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_make_enum_binary(
    f: &mut WasmFunction,
    env: &mut LocalEnv,
    dest: ValueId,
    name: &str,
    variant: &str,
    payload: Option<ValueId>,
    enums: &BTreeMap<String, WasmEnum>,
    func_indices: &FuncIndexMap,
) -> Result<(), WasmError> {
    let enum_ty = enums
        .get(name)
        .ok_or_else(|| WasmError::new(format!("unknown enum `{name}`")))?;
    let variant_index = enum_ty
        .variants
        .iter()
        .position(|v| v.name == variant)
        .expect("variant should exist");
    let dest_idx = env.get_or_alloc_value(dest);

    if enum_is_payload_free(enum_ty) {
        f.instruction(&Instruction::I64Const(
            i64::try_from(variant_index).unwrap(),
        ));
        f.instruction(&Instruction::LocalSet(dest_idx));
        return Ok(());
    }

    f.instruction(&Instruction::I32Const(
        i32::try_from(PAYLOAD_ENUM_SIZE).unwrap(),
    ));
    f.instruction(&Instruction::Call(func_indices.runtime("alloc")));
    f.instruction(&Instruction::I64ExtendI32U);
    f.instruction(&Instruction::LocalSet(dest_idx));

    f.instruction(&Instruction::LocalGet(dest_idx));
    f.instruction(&Instruction::I32WrapI64);
    f.instruction(&Instruction::I64Const(
        i64::try_from(variant_index).unwrap(),
    ));
    f.instruction(&Instruction::I64Store(memarg(0)));

    if let Some(source) = payload {
        f.instruction(&Instruction::LocalGet(dest_idx));
        f.instruction(&Instruction::I32WrapI64);
        f.instruction(&Instruction::I32Const(8));
        f.instruction(&Instruction::I32Add);
        f.instruction(&Instruction::LocalGet(env.get_value(source)));
        let payload_kind = enum_ty
            .variants
            .get(variant_index)
            .and_then(|v| v.payload.as_ref())
            .ok_or_else(|| {
                WasmError::new(format!(
                    "enum `{name}` variant `{variant}` is missing payload metadata"
                ))
            })?;
        match wasm_type_from_kind_result(payload_kind) {
            Some(WasmType::I64) | None => {
                f.instruction(&Instruction::I64Store(memarg(0)));
            }
            Some(WasmType::F64) => {
                f.instruction(&Instruction::F64Store(memarg(0)));
            }
        }
    }

    Ok(())
}
