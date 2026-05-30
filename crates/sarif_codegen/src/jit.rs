#![allow(unsafe_code, unsafe_op_in_unsafe_fn, unused_unsafe)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use cranelift_codegen::ir::{AbiParam, InstBuilder, UserFuncName, types};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataId, FuncId, Linkage, Module, default_libcall_names};

use crate::native::{
    ListHeader, NativeEnum, NativeRecord, NativeValueRepr, TextIndexHelperIds, TrustedListAccesses,
    collect_native_enums, collect_native_records, declare_alloc_pop, declare_alloc_push,
    declare_arg_count, declare_arg_text, declare_bytes_slice, declare_bytes_to_text,
    declare_file_close, declare_file_exists, declare_file_is_valid, declare_file_open,
    declare_file_read, declare_file_read_to_end, declare_file_remove, declare_file_seek,
    declare_file_size, declare_file_write, declare_list_new, declare_list_push,
    declare_list_sort_by_text_field, declare_list_sort_text, declare_parse_f64, declare_parse_i32,
    declare_parse_i32_range, declare_record_allocator, declare_stdin_text, declare_stdout_write,
    declare_stdout_write_builder, declare_text_builder_append, declare_text_builder_append_ascii,
    declare_text_builder_append_codepoint, declare_text_builder_append_i32,
    declare_text_builder_append_slice, declare_text_builder_finish, declare_text_builder_new,
    declare_text_cmp, declare_text_concat, declare_text_eq, declare_text_eq_range,
    declare_text_field_end, declare_text_find_byte_range, declare_text_from_f64_fixed,
    declare_text_intern, declare_text_line_end, declare_text_next_field, declare_text_next_line,
    declare_text_slice, infer_value_kinds, lower_insts, native_type as shared_native_type,
};
use crate::{Function, Program, RuntimeError, RuntimeValue, ValueId};

// ---------------------------------------------------------------------------
// Arena allocator (thread-local bump arena with scope stack)
// ---------------------------------------------------------------------------

thread_local! {
    static ARENA: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static SCOPE_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

const ARENA_ALIGN: usize = 16;
static EMPTY_TEXT: [u8; 8] = [0u8; 8];

fn arena_align_up(n: usize) -> usize {
    (n + ARENA_ALIGN - 1) & !(ARENA_ALIGN - 1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_record_alloc(size: i64) -> i64 {
    unsafe {
        let n = size.max(8) as usize;
        let aligned = arena_align_up(n);
        ARENA.with(|arena| {
            let mut arena = arena.borrow_mut();
            let pos = arena.len();
            arena.resize(pos + aligned, 0);
            (arena.as_ptr().add(pos)) as i64
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_alloc_push() {
    unsafe {
        ARENA.with(|arena| {
            SCOPE_STACK.with(|stack| {
                stack.borrow_mut().push(arena.borrow().len());
            });
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_alloc_pop() {
    unsafe {
        SCOPE_STACK.with(|stack| {
            if let Some(saved) = stack.borrow_mut().pop() {
                ARENA.with(|arena| {
                    arena.borrow_mut().truncate(saved);
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

unsafe fn text_len(ptr: i64) -> u64 {
    unsafe { std::ptr::read_unaligned(ptr as *const u64) }
}

unsafe fn text_data(ptr: i64) -> *const u8 {
    unsafe { (ptr as *const u8).add(8) }
}

unsafe fn text_data_mut(ptr: i64) -> *mut u8 {
    unsafe { (ptr as *mut u8).add(8) }
}

unsafe fn alloc_text(len: u64) -> i64 {
    unsafe {
        let blob_size = 8 + len as usize;
        let blob = sarif_record_alloc(blob_size as i64);
        std::ptr::write_unaligned(blob as *mut u64, len);
        blob
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_len(ptr: i64) -> i64 {
    unsafe {
        if ptr == 0 {
            return 0;
        }
        text_len(ptr) as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_concat(left: i64, right: i64) -> i64 {
    unsafe {
        if left == 0 {
            return right;
        }
        if right == 0 {
            return left;
        }
        let l_len = text_len(left);
        let r_len = text_len(right);
        let result = alloc_text(l_len + r_len);
        let data = text_data_mut(result);
        std::ptr::copy_nonoverlapping(text_data(left), data, l_len as usize);
        std::ptr::copy_nonoverlapping(text_data(right), data.add(l_len as usize), r_len as usize);
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_intern(ptr: i64) -> i64 {
    unsafe {
        if ptr == 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        if ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return ptr;
        }
        let len = text_len(ptr);
        let result = alloc_text(len);
        std::ptr::copy_nonoverlapping(text_data(ptr), text_data_mut(result), len as usize);
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_eq(left: i64, right: i64) -> i64 {
    unsafe {
        if left == right {
            return 1;
        }
        if left == 0 || right == 0 {
            return 0;
        }
        let l_len = text_len(left);
        let r_len = text_len(right);
        if l_len != r_len {
            return 0;
        }
        let l_data = text_data(left);
        let r_data = text_data(right);
        let mut i = 0;
        while i < l_len as usize {
            if std::ptr::read(l_data.add(i)) != std::ptr::read(r_data.add(i)) {
                return 0;
            }
            i += 1;
        }
        1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_cmp(left: i64, right: i64) -> i64 {
    unsafe {
        if left == right {
            return 0;
        }
        if left == 0 {
            return -1;
        }
        if right == 0 {
            return 1;
        }
        let l_len = text_len(left);
        let r_len = text_len(right);
        let min_len = l_len.min(r_len) as usize;
        let l_data = text_data(left);
        let r_data = text_data(right);
        let mut i = 0;
        while i < min_len {
            let lb = std::ptr::read(l_data.add(i));
            let rb = std::ptr::read(r_data.add(i));
            if lb < rb {
                return -1;
            }
            if lb > rb {
                return 1;
            }
            i += 1;
        }
        if l_len < r_len {
            -1
        } else if l_len > r_len {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_slice(ptr: i64, start: i64, end: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let len = text_len(ptr);
        let s = start.max(0).min(len as i64) as u64;
        let e = end.max(s as i64).min(len as i64) as u64;
        let slice_len = e - s;
        if slice_len == 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let result = alloc_text(slice_len);
        std::ptr::copy_nonoverlapping(
            text_data(ptr).add(s as usize),
            text_data_mut(result),
            slice_len as usize,
        );
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_slice(ptr: i64, start: i64, end: i64) -> i64 {
    unsafe { sarif_text_slice(ptr, start, end) }
}

// ---------------------------------------------------------------------------
// Stdout/stderr helpers
// ---------------------------------------------------------------------------

fn write_stdout(data: &[u8]) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(data);
    let _ = handle.flush();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_stdout_write(ptr: i64) {
    unsafe {
        if ptr == 0 {
            return;
        }
        let len = text_len(ptr) as usize;
        let data = std::slice::from_raw_parts(text_data(ptr), len);
        write_stdout(data);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_stdout_write_builder(builder: i64) -> i64 {
    unsafe {
        let buf = builder as *mut SarifTextBuilder;
        let len = unsafe { (*buf).len } as usize;
        let bytes = unsafe { std::slice::from_raw_parts((*buf).bytes, len) };
        write_stdout(bytes);
        unsafe {
            (*buf).len = 0;
        }
        builder
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_stdin_text() -> i64 {
    unsafe {
        let mut buf = Vec::new();
        let _ = io::stdin().read_to_end(&mut buf);
        let len = buf.len() as u64;
        let result = alloc_text(len);
        std::ptr::copy_nonoverlapping(buf.as_ptr(), text_data_mut(result), len as usize);
        result
    }
}

// ---------------------------------------------------------------------------
// List helpers
// ---------------------------------------------------------------------------

#[repr(C)]
struct SarifList {
    len: u64,
    values: *mut u64,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_new(len: i64, fill: i64) -> i64 {
    unsafe {
        let cap = len.max(0) as usize;
        let alloc_cap = cap.max(4);
        let mut values = vec![fill as u64; alloc_cap];
        let list = Box::new(SarifList {
            len: alloc_cap as u64,
            values: values.as_mut_ptr(),
        });
        std::mem::forget(values);
        Box::into_raw(list) as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_push(list: i64, logical_len: i64, value: i64) -> i64 {
    unsafe {
        let list = list as *mut SarifList;
        let used = logical_len as usize;
        let cap = unsafe { (*list).len } as usize;
        if used >= cap {
            let new_cap = cap.max(4) * 2;
            let old_ptr = unsafe { (*list).values };
            let mut new_values = Vec::from_raw_parts(old_ptr, cap, cap);
            new_values.resize(new_cap, 0);
            unsafe {
                (*list).values = new_values.as_mut_ptr();
                (*list).len = new_cap as u64;
            }
            std::mem::forget(new_values);
        }
        unsafe {
            std::ptr::write((*list).values.add(used), value as u64);
        }
        list as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_get(list: i64, index: i64) -> i64 {
    unsafe {
        if list == 0 {
            return 0;
        }
        let list = list as *mut SarifList;
        let idx = index as usize;
        let cap = unsafe { (*list).len } as usize;
        if idx >= cap {
            return 0;
        }
        unsafe { std::ptr::read((*list).values.add(idx)) as i64 }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_len(list: i64) -> i64 {
    unsafe {
        if list == 0 {
            return 0;
        }
        unsafe { (*(list as *mut SarifList)).len as i64 }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_sort_text(list: i64, logical_len: i64) -> i64 {
    unsafe {
        if list == 0 {
            return 0;
        }
        let list = list as *mut SarifList;
        let n = logical_len as usize;
        if n <= 1 {
            return list as i64;
        }
        let slice = unsafe { std::slice::from_raw_parts_mut((*list).values, n) };
        slice.sort_by(|a, b| {
            let cmp = unsafe { sarif_text_cmp(*a as i64, *b as i64) };
            cmp.cmp(&0)
        });
        list as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_sort_by_text_field(
    list: i64,
    logical_len: i64,
    _field_idx: i64,
) -> i64 {
    unsafe { sarif_list_sort_text(list, logical_len) }
}

// ---------------------------------------------------------------------------
// TextBuilder helpers
// ---------------------------------------------------------------------------

#[repr(C)]
struct SarifTextBuilder {
    len: u64,
    cap: u64,
    bytes: *mut u8,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_new() -> i64 {
    unsafe {
        let cap = 128u64;
        let mut buf = vec![0u8; cap as usize];
        let builder = Box::new(SarifTextBuilder {
            len: 0,
            cap,
            bytes: buf.as_mut_ptr(),
        });
        std::mem::forget(buf);
        Box::into_raw(builder) as i64
    }
}

unsafe fn builder_grow(builder: *mut SarifTextBuilder, needed: usize) {
    unsafe {
        let cap = (*builder).cap as usize;
        let mut new_cap = cap.max(128) * 2;
        while new_cap < needed {
            new_cap *= 2;
        }
        let old_ptr = (*builder).bytes;
        let _old_len = (*builder).len as usize;
        let mut new_buf = Vec::from_raw_parts(old_ptr, cap, cap);
        new_buf.resize(new_cap, 0);
        (*builder).bytes = new_buf.as_mut_ptr();
        (*builder).cap = new_cap as u64;
        std::mem::forget(new_buf);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_append(builder: i64, text: i64) -> i64 {
    unsafe {
        if text == 0 {
            return builder;
        }
        let builder = builder as *mut SarifTextBuilder;
        let tlen = text_len(text) as usize;
        let cur = (*builder).len as usize;
        let needed = cur + tlen;
        let cap = (*builder).cap as usize;
        if needed > cap {
            builder_grow(builder, needed);
        }
        std::ptr::copy_nonoverlapping(text_data(text), (*builder).bytes.add(cur), tlen);
        (*builder).len = needed as u64;
        builder as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_append_codepoint(builder: i64, cp: i64) -> i64 {
    unsafe {
        let builder = builder as *mut SarifTextBuilder;
        let mut buf = [0u8; 4];
        let s = char::from_u32(cp as u32).map(|c| c.encode_utf8(&mut buf));
        if let Some(encoded) = s {
            let len = encoded.len();
            let cur = (*builder).len as usize;
            let cap = (*builder).cap as usize;
            if cur + len > cap {
                builder_grow(builder, cur + len);
            }
            std::ptr::copy_nonoverlapping(buf.as_ptr(), (*builder).bytes.add(cur), len);
            (*builder).len = (cur + len) as u64;
        }
        builder as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_append_ascii(builder: i64, byte: i64) -> i64 {
    unsafe {
        let builder = builder as *mut SarifTextBuilder;
        let b = byte as u8;
        let cur = (*builder).len as usize;
        let cap = (*builder).cap as usize;
        if cur + 1 > cap {
            builder_grow(builder, cur + 1);
        }
        std::ptr::write((*builder).bytes.add(cur), b);
        (*builder).len = (cur + 1) as u64;
        builder as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_append_slice(
    builder: i64,
    text: i64,
    start: i64,
    end: i64,
) -> i64 {
    unsafe {
        let builder = builder as *mut SarifTextBuilder;
        if text != 0 {
            let tlen = text_len(text) as i64;
            let s = start.max(0).min(tlen) as usize;
            let e = end.max(s as i64).min(tlen) as usize;
            let slen = e - s;
            if slen > 0 {
                let cur = (*builder).len as usize;
                let cap = (*builder).cap as usize;
                if cur + slen > cap {
                    builder_grow(builder, cur + slen);
                }
                std::ptr::copy_nonoverlapping(
                    text_data(text).add(s),
                    (*builder).bytes.add(cur),
                    slen,
                );
                (*builder).len = (cur + slen) as u64;
            }
        }
        builder as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_append_i32(builder: i64, value: i64) -> i64 {
    unsafe {
        let builder = builder as *mut SarifTextBuilder;
        let s = value.to_string();
        let s_bytes = s.as_bytes();
        let cur = (*builder).len as usize;
        let cap = (*builder).cap as usize;
        if cur + s_bytes.len() > cap {
            builder_grow(builder, cur + s_bytes.len());
        }
        std::ptr::copy_nonoverlapping(s_bytes.as_ptr(), (*builder).bytes.add(cur), s_bytes.len());
        (*builder).len = (cur + s_bytes.len()) as u64;
        builder as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_builder_finish(builder: i64) -> i64 {
    unsafe {
        let builder = builder as *mut SarifTextBuilder;
        let len = (*builder).len;
        let result = alloc_text(len);
        std::ptr::copy_nonoverlapping((*builder).bytes, text_data_mut(result), len as usize);
        drop(Box::from_raw(builder));
        result
    }
}

// ---------------------------------------------------------------------------
// TextIndex helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
#[repr(C)]
struct SarifTextIndexEntry {
    key: u64,
    value: i64,
    hash: u32,
    occupied: u8,
}

#[repr(C)]
struct SarifTextIndex {
    len: u64,
    cap: u64,
    entries: *mut SarifTextIndexEntry,
}

fn text_index_hash(key: u64) -> u32 {
    // FNV-1a 32-bit hash
    let mut hash: u32 = 2166136261;
    let bytes = key.to_le_bytes();
    for &b in &bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    hash
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_index_new() -> i64 {
    unsafe {
        let cap = 8u64;
        let entries = vec![
            SarifTextIndexEntry {
                key: 0,
                value: 0,
                hash: 0,
                occupied: 0,
            };
            cap as usize
        ];
        let mut index = Box::new(SarifTextIndex {
            len: 0,
            cap,
            entries: std::ptr::null_mut(),
        });
        index.entries = entries.as_ptr() as *mut SarifTextIndexEntry;
        std::mem::forget(entries);
        Box::into_raw(index) as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_index_get(index: i64, key: i64) -> i64 {
    unsafe {
        if index == 0 {
            return 0;
        }
        let idx = index as *mut SarifTextIndex;
        let cap = (*idx).cap as usize;
        let h = text_index_hash(key as u64);
        let mask = cap - 1;
        let mut i = (h as usize) & mask;
        for _ in 0..cap {
            let entry = &*((*idx).entries.add(i));
            if entry.occupied == 0 {
                return 0;
            }
            if entry.hash == h && entry.key == key as u64 {
                return entry.value;
            }
            i = (i + 1) & mask;
        }
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_index_contains(index: i64, key: i64) -> i64 {
    unsafe {
        if index == 0 {
            return 0;
        }
        let idx = index as *mut SarifTextIndex;
        let cap = (*idx).cap as usize;
        let h = text_index_hash(key as u64);
        let mask = cap - 1;
        let mut i = (h as usize) & mask;
        for _ in 0..cap {
            let entry = &*((*idx).entries.add(i));
            if entry.occupied == 0 {
                return 0;
            }
            if entry.hash == h && entry.key == key as u64 {
                return 1;
            }
            i = (i + 1) & mask;
        }
        0
    }
}

unsafe fn index_grow(idx: *mut SarifTextIndex) {
    unsafe {
        let old_cap = (*idx).cap as usize;
        let new_cap = old_cap * 2;
        let old_entries = Vec::from_raw_parts((*idx).entries, old_cap, old_cap);
        let mut new_entries = vec![
            SarifTextIndexEntry {
                key: 0,
                value: 0,
                hash: 0,
                occupied: 0,
            };
            new_cap
        ];
        for old_entry in &*old_entries {
            if old_entry.occupied != 0 {
                let h = old_entry.hash;
                let mask = new_cap - 1;
                let mut i = (h as usize) & mask;
                while new_entries[i].occupied != 0 {
                    i = (i + 1) & (new_cap - 1);
                }
                new_entries[i] = old_entry.clone();
            }
        }
        (*idx).entries = new_entries.as_mut_ptr();
        (*idx).cap = new_cap as u64;
        std::mem::forget(new_entries);
        drop(old_entries);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_index_get_or_insert(
    index: i64,
    key: i64,
    default_value: i64,
) -> i64 {
    unsafe {
        if index == 0 {
            return 0;
        }
        let idx = index as *mut SarifTextIndex;
        loop {
            let cap = (*idx).cap as usize;
            let h = text_index_hash(key as u64);
            let mask = cap - 1;
            let mut i = (h as usize) & mask;
            let mut first_empty = None;
            for _ in 0..cap {
                let entry = &*((*idx).entries.add(i));
                if entry.occupied == 0 {
                    first_empty = Some(i);
                    break;
                }
                if entry.hash == h && entry.key == key as u64 {
                    return entry.value;
                }
                i = (i + 1) & mask;
            }
            // Key not found; check if we need to grow
            let load = (*idx).len as f64 / (*idx).cap as f64;
            if load > 0.75 {
                index_grow(idx);
                continue;
            }
            if let Some(slot) = first_empty {
                std::ptr::write(
                    (*idx).entries.add(slot),
                    SarifTextIndexEntry {
                        key: key as u64,
                        value: default_value,
                        hash: h,
                        occupied: 1,
                    },
                );
                (*idx).len += 1;
                return default_value;
            }
            index_grow(idx);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_index_set(index: i64, key: i64, value: i64) -> i64 {
    unsafe {
        if index == 0 {
            return index;
        }
        let idx = index as *mut SarifTextIndex;
        loop {
            let cap = (*idx).cap as usize;
            let h = text_index_hash(key as u64);
            let mask = cap - 1;
            let mut i = (h as usize) & mask;
            let mut first_empty = None;
            for _ in 0..cap {
                let entry = &mut *((*idx).entries.add(i));
                if entry.occupied == 0 {
                    first_empty = Some(i);
                    break;
                }
                if entry.hash == h && entry.key == key as u64 {
                    entry.value = value;
                    return index;
                }
                i = (i + 1) & mask;
            }
            let load = (*idx).len as f64 / (*idx).cap as f64;
            if load > 0.75 {
                index_grow(idx);
                continue;
            }
            if let Some(slot) = first_empty {
                std::ptr::write(
                    (*idx).entries.add(slot),
                    SarifTextIndexEntry {
                        key: key as u64,
                        value,
                        hash: h,
                        occupied: 1,
                    },
                );
                (*idx).len += 1;
                return index;
            }
            index_grow(idx);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_index_keys(index: i64) -> i64 {
    unsafe {
        if index == 0 {
            return 0;
        }
        let idx = index as *mut SarifTextIndex;
        let n = (*idx).len as usize;
        let mut list = sarif_list_new(n as i64, 0);
        let cap = (*idx).cap as usize;
        let mut logical = 0i64;
        for i in 0..cap {
            let entry = &*((*idx).entries.add(i));
            if entry.occupied != 0 {
                list = sarif_list_push(list, logical, entry.key as i64);
                logical += 1;
            }
        }
        list
    }
}

// ---------------------------------------------------------------------------
// Text eq_range helpers
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_eq_range(left: i64, right: i64) -> i64 {
    unsafe { sarif_text_eq(left, right) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_find_byte_range(_text: i64, _byte: i64) -> i64 {
    unsafe { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_line_end(_text: i64, _offset: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_next_line(_text: i64, _offset: i64) -> i64 {
    unsafe { -1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_field_end(_text: i64, _offset: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_next_field(_text: i64, _offset: i64) -> i64 {
    unsafe { -1 }
}

// ---------------------------------------------------------------------------
// F64 helpers
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_from_f64_fixed(value: f64, digits: i64) -> i64 {
    unsafe {
        let d = digits.clamp(0, 100) as usize;
        if value.is_nan() || value.is_infinite() {
            let s = if value.is_nan() {
                "nan"
            } else if value.is_sign_negative() {
                "-inf"
            } else {
                "inf"
            };
            let len = s.len() as u64;
            let result = alloc_text(len);
            std::ptr::copy_nonoverlapping(s.as_ptr(), text_data_mut(result), len as usize);
            return result;
        }
        let s = if d == 0 {
            format!("{:.0}", value)
        } else {
            format!("{:.d$}", value, d = d)
        };
        let s_bytes = s.as_bytes();
        let len = s_bytes.len() as u64;
        let result = alloc_text(len);
        std::ptr::copy_nonoverlapping(s_bytes.as_ptr(), text_data_mut(result), len as usize);
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_parse_i32(text: i64) -> i64 {
    unsafe {
        if text == 0 {
            return 0;
        }
        let len = text_len(text) as usize;
        if len == 0 {
            return 0;
        }
        let data = std::slice::from_raw_parts(text_data(text), len);
        let s = std::str::from_utf8(data).unwrap_or("0");
        s.parse::<i32>().unwrap_or(0) as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_parse_i32_range(_text: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_parse_f64(text: i64) -> f64 {
    unsafe {
        if text == 0 {
            return 0.0;
        }
        let len = text_len(text) as usize;
        if len == 0 {
            return 0.0;
        }
        let data = unsafe { std::slice::from_raw_parts(text_data(text), len) };
        let s = std::str::from_utf8(data).unwrap_or("0");
        s.parse::<f64>().unwrap_or(0.0)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_arg_count() -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_arg_text(_index: i64) -> i64 {
    unsafe { EMPTY_TEXT.as_ptr() as i64 }
}

// ---------------------------------------------------------------------------
// File I/O stubs
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_open(_path: i64, _mode: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_close(_handle: i64) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_read(_handle: i64, _count: i64) -> i64 {
    unsafe { EMPTY_TEXT.as_ptr() as i64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_read_to_end(_handle: i64) -> i64 {
    unsafe { EMPTY_TEXT.as_ptr() as i64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_write(_handle: i64, _data: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_seek(_handle: i64, _offset: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_size(_handle: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_exists(_path: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_remove(_path: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_is_valid(_handle: i64) -> i64 {
    unsafe { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_to_text(_bytes: i64) -> i64 {
    unsafe { EMPTY_TEXT.as_ptr() as i64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_data_for_insts() {}

// ---------------------------------------------------------------------------
// Register all runtime helpers with the JIT builder
// ---------------------------------------------------------------------------

fn register_runtime_helpers(builder: &mut JITBuilder) {
    let helpers: &[(&str, *const u8)] = &[
        ("sarif_record_alloc", sarif_record_alloc as *const u8),
        ("sarif_alloc_push", sarif_alloc_push as *const u8),
        ("sarif_alloc_pop", sarif_alloc_pop as *const u8),
        ("sarif_text_len", sarif_text_len as *const u8),
        ("sarif_text_concat", sarif_text_concat as *const u8),
        ("sarif_text_intern", sarif_text_intern as *const u8),
        ("sarif_text_eq", sarif_text_eq as *const u8),
        ("sarif_text_cmp", sarif_text_cmp as *const u8),
        ("sarif_text_slice", sarif_text_slice as *const u8),
        ("sarif_bytes_slice", sarif_bytes_slice as *const u8),
        ("sarif_text_eq_range", sarif_text_eq_range as *const u8),
        (
            "sarif_text_find_byte_range",
            sarif_text_find_byte_range as *const u8,
        ),
        ("sarif_text_line_end", sarif_text_line_end as *const u8),
        ("sarif_text_next_line", sarif_text_next_line as *const u8),
        ("sarif_text_field_end", sarif_text_field_end as *const u8),
        ("sarif_text_next_field", sarif_text_next_field as *const u8),
        (
            "sarif_text_from_f64_fixed",
            sarif_text_from_f64_fixed as *const u8,
        ),
        (
            "sarif_text_builder_new",
            sarif_text_builder_new as *const u8,
        ),
        (
            "sarif_text_builder_append",
            sarif_text_builder_append as *const u8,
        ),
        (
            "sarif_text_builder_append_codepoint",
            sarif_text_builder_append_codepoint as *const u8,
        ),
        (
            "sarif_text_builder_append_ascii",
            sarif_text_builder_append_ascii as *const u8,
        ),
        (
            "sarif_text_builder_append_slice",
            sarif_text_builder_append_slice as *const u8,
        ),
        (
            "sarif_text_builder_append_i32",
            sarif_text_builder_append_i32 as *const u8,
        ),
        (
            "sarif_text_builder_finish",
            sarif_text_builder_finish as *const u8,
        ),
        ("sarif_stdout_write", sarif_stdout_write as *const u8),
        (
            "sarif_stdout_write_builder",
            sarif_stdout_write_builder as *const u8,
        ),
        ("sarif_stdin_text", sarif_stdin_text as *const u8),
        ("sarif_list_new", sarif_list_new as *const u8),
        ("sarif_list_push", sarif_list_push as *const u8),
        ("sarif_list_get", sarif_list_get as *const u8),
        ("sarif_list_len", sarif_list_len as *const u8),
        ("sarif_list_sort_text", sarif_list_sort_text as *const u8),
        (
            "sarif_list_sort_by_text_field",
            sarif_list_sort_by_text_field as *const u8,
        ),
        ("sarif_text_index_new", sarif_text_index_new as *const u8),
        ("sarif_text_index_get", sarif_text_index_get as *const u8),
        (
            "sarif_text_index_contains",
            sarif_text_index_contains as *const u8,
        ),
        (
            "sarif_text_index_get_or_insert",
            sarif_text_index_get_or_insert as *const u8,
        ),
        ("sarif_text_index_set", sarif_text_index_set as *const u8),
        ("sarif_text_index_keys", sarif_text_index_keys as *const u8),
        ("sarif_parse_i32", sarif_parse_i32 as *const u8),
        ("sarif_parse_i32_range", sarif_parse_i32_range as *const u8),
        ("sarif_parse_f64", sarif_parse_f64 as *const u8),
        ("sarif_arg_count", sarif_arg_count as *const u8),
        ("sarif_arg_text", sarif_arg_text as *const u8),
        ("sarif_file_open", sarif_file_open as *const u8),
        ("sarif_file_close", sarif_file_close as *const u8),
        ("sarif_file_read", sarif_file_read as *const u8),
        (
            "sarif_file_read_to_end",
            sarif_file_read_to_end as *const u8,
        ),
        ("sarif_file_write", sarif_file_write as *const u8),
        ("sarif_file_seek", sarif_file_seek as *const u8),
        ("sarif_file_size", sarif_file_size as *const u8),
        ("sarif_file_exists", sarif_file_exists as *const u8),
        ("sarif_file_remove", sarif_file_remove as *const u8),
        ("sarif_file_is_valid", sarif_file_is_valid as *const u8),
        ("sarif_bytes_to_text", sarif_bytes_to_text as *const u8),
        (
            "sarif_text_data_for_insts",
            sarif_text_data_for_insts as *const u8,
        ),
    ];
    for &(name, ptr) in helpers {
        builder.symbol(name, ptr);
    }
}

// ---------------------------------------------------------------------------
// JitBackend: mirrors ObjectBackend's structure using JITModule
// ---------------------------------------------------------------------------

struct JitBackend<'a> {
    program: &'a Program,
    module: JITModule,
    function_ids: BTreeMap<String, FuncId>,
    allocator_id: FuncId,
    alloc_push_id: FuncId,
    alloc_pop_id: FuncId,
    text_builder_new_id: Option<FuncId>,
    text_builder_append_id: Option<FuncId>,
    text_builder_append_codepoint_id: Option<FuncId>,
    text_builder_append_ascii_id: Option<FuncId>,
    text_builder_append_slice_id: Option<FuncId>,
    text_builder_append_i32_id: Option<FuncId>,
    text_builder_finish_id: Option<FuncId>,
    stdout_write_builder_id: Option<FuncId>,
    list_new_id: FuncId,
    list_push_id: FuncId,
    list_sort_text_id: Option<FuncId>,
    list_sort_by_text_field_id: Option<FuncId>,
    text_concat_id: FuncId,
    text_slice_id: FuncId,
    bytes_slice_id: FuncId,
    text_eq_range_id: FuncId,
    text_find_byte_range_id: FuncId,
    text_line_end_id: FuncId,
    text_next_line_id: FuncId,
    text_field_end_id: FuncId,
    text_next_field_id: FuncId,
    text_from_f64_fixed_id: FuncId,
    parse_i32_id: FuncId,
    parse_i32_range_id: FuncId,
    parse_f64_id: FuncId,
    arg_count_id: FuncId,
    arg_text_id: FuncId,
    stdin_text_id: FuncId,
    stdout_write_id: FuncId,
    file_open_id: FuncId,
    file_close_id: FuncId,
    file_read_id: FuncId,
    file_read_to_end_id: FuncId,
    file_write_id: FuncId,
    file_seek_id: FuncId,
    file_size_id: FuncId,
    file_exists_id: FuncId,
    file_remove_id: FuncId,
    file_is_valid_id: FuncId,
    bytes_to_text_id: FuncId,
    text_eq_id: FuncId,
    text_cmp_id: FuncId,
    text_intern_id: FuncId,
    records: BTreeMap<String, NativeRecord>,
    native_enums: BTreeMap<String, NativeEnum>,
}

impl<'a> JitBackend<'a> {
    fn new(program: &'a Program) -> Result<Self, String> {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| e.to_string())?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| e.to_string())?;
        let isa_builder =
            cranelift_native::builder().map_err(|e| format!("host ISA not supported: {e}"))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| e.to_string())?;

        let mut jit_builder = JITBuilder::with_isa(isa, default_libcall_names());
        register_runtime_helpers(&mut jit_builder);

        let mut module = JITModule::new(jit_builder);
        let records = collect_native_records(program)?;
        let native_enums = collect_native_enums(program);

        let mut declare = |name: &str,
                           f: fn(&mut JITModule, &str) -> Result<FuncId, String>|
         -> Result<FuncId, String> {
            f(&mut module, "jit").map_err(|e| format!("failed to declare {name}: {e}"))
        };

        let allocator_id = declare("record_allocator", declare_record_allocator)?;
        let alloc_push_id = declare("alloc_push", declare_alloc_push)?;
        let alloc_pop_id = declare("alloc_pop", declare_alloc_pop)?;
        let list_new_id = declare("list_new", declare_list_new)?;
        let list_push_id = declare("list_push", declare_list_push)?;
        let text_concat_id = declare("text_concat", declare_text_concat)?;
        let text_slice_id = declare("text_slice", declare_text_slice)?;
        let bytes_slice_id = declare("bytes_slice", declare_bytes_slice)?;
        let text_eq_range_id = declare("text_eq_range", declare_text_eq_range)?;
        let text_find_byte_range_id =
            declare("text_find_byte_range", declare_text_find_byte_range)?;
        let text_line_end_id = declare("text_line_end", declare_text_line_end)?;
        let text_next_line_id = declare("text_next_line", declare_text_next_line)?;
        let text_field_end_id = declare("text_field_end", declare_text_field_end)?;
        let text_next_field_id = declare("text_next_field", declare_text_next_field)?;
        let text_from_f64_fixed_id = declare("text_from_f64_fixed", declare_text_from_f64_fixed)?;
        let parse_i32_id = declare("parse_i32", declare_parse_i32)?;
        let parse_i32_range_id = declare("parse_i32_range", declare_parse_i32_range)?;
        let parse_f64_id = declare("parse_f64", declare_parse_f64)?;
        let arg_count_id = declare("arg_count", declare_arg_count)?;
        let arg_text_id = declare("arg_text", declare_arg_text)?;
        let stdin_text_id = declare("stdin_text", declare_stdin_text)?;
        let stdout_write_id = declare("stdout_write", declare_stdout_write)?;
        let stdout_write_builder_id = Some(declare(
            "stdout_write_builder",
            declare_stdout_write_builder,
        )?);
        let text_intern_id = declare("text_intern", declare_text_intern)?;
        let text_eq_id = declare("text_eq", declare_text_eq)?;
        let text_cmp_id = declare("text_cmp", declare_text_cmp)?;
        let file_open_id = declare("file_open", declare_file_open)?;
        let file_close_id = declare("file_close", declare_file_close)?;
        let file_read_id = declare("file_read", declare_file_read)?;
        let file_read_to_end_id = declare("file_read_to_end", declare_file_read_to_end)?;
        let file_write_id = declare("file_write", declare_file_write)?;
        let file_seek_id = declare("file_seek", declare_file_seek)?;
        let file_size_id = declare("file_size", declare_file_size)?;
        let file_exists_id = declare("file_exists", declare_file_exists)?;
        let file_remove_id = declare("file_remove", declare_file_remove)?;
        let file_is_valid_id = declare("file_is_valid", declare_file_is_valid)?;
        let bytes_to_text_id = declare("bytes_to_text", declare_bytes_to_text)?;

        let text_builder_new_id = Some(declare("text_builder_new", declare_text_builder_new)?);
        let text_builder_append_id =
            Some(declare("text_builder_append", declare_text_builder_append)?);
        let text_builder_append_codepoint_id = Some(declare(
            "text_builder_append_codepoint",
            declare_text_builder_append_codepoint,
        )?);
        let text_builder_append_ascii_id = Some(declare(
            "text_builder_append_ascii",
            declare_text_builder_append_ascii,
        )?);
        let text_builder_append_slice_id = Some(declare(
            "text_builder_append_slice",
            declare_text_builder_append_slice,
        )?);
        let text_builder_append_i32_id = Some(declare(
            "text_builder_append_i32",
            declare_text_builder_append_i32,
        )?);
        let text_builder_finish_id =
            Some(declare("text_builder_finish", declare_text_builder_finish)?);

        let list_sort_text_id = Some(declare("list_sort_text", declare_list_sort_text)?);
        let list_sort_by_text_field_id = Some(declare(
            "list_sort_by_text_field",
            declare_list_sort_by_text_field,
        )?);

        let mut function_ids = BTreeMap::new();
        for function in &program.functions {
            let mut signature = module.make_signature();
            signature.call_conv = CallConv::triple_default(module.isa().triple());
            for param in &function.params {
                let ty = shared_native_type(&param.ty, &records, &native_enums)?;
                if ty != types::INVALID {
                    signature.params.push(AbiParam::new(ty));
                }
            }
            if let Some(return_type) = function.return_type.as_deref() {
                let ty = shared_native_type(return_type, &records, &native_enums)?;
                if ty != types::INVALID {
                    signature.returns.push(AbiParam::new(ty));
                }
            }
            let linkage = if function.name == "main" || function.name.ends_with("_main") {
                Linkage::Export
            } else {
                Linkage::Local
            };
            let id = module
                .declare_function(&function.name, linkage, &signature)
                .map_err(|e| format!("failed to declare function `{}`: {e}", function.name))?;
            function_ids.insert(function.name.clone(), id);
        }

        Ok(Self {
            program,
            module,
            function_ids,
            allocator_id,
            alloc_push_id,
            alloc_pop_id,
            text_builder_new_id,
            text_builder_append_id,
            text_builder_append_codepoint_id,
            text_builder_append_ascii_id,
            text_builder_append_slice_id,
            text_builder_append_i32_id,
            text_builder_finish_id,
            stdout_write_builder_id,
            list_new_id,
            list_push_id,
            list_sort_text_id,
            list_sort_by_text_field_id,
            text_concat_id,
            text_slice_id,
            bytes_slice_id,
            text_eq_range_id,
            text_find_byte_range_id,
            text_line_end_id,
            text_next_line_id,
            text_field_end_id,
            text_next_field_id,
            text_from_f64_fixed_id,
            parse_i32_id,
            parse_i32_range_id,
            parse_f64_id,
            arg_count_id,
            arg_text_id,
            stdin_text_id,
            stdout_write_id,
            file_open_id,
            file_close_id,
            file_read_id,
            file_read_to_end_id,
            file_write_id,
            file_seek_id,
            file_size_id,
            file_exists_id,
            file_remove_id,
            file_is_valid_id,
            bytes_to_text_id,
            text_eq_id,
            text_cmp_id,
            text_intern_id,
            records,
            native_enums,
        })
    }

    fn define_function(&mut self, function: &Function) -> Result<(), String> {
        let id = self.function_ids[&function.name];
        let mut ctx = self.module.make_context();
        ctx.func.name = UserFuncName::user(0, id.as_u32());

        let mut signature = self.module.make_signature();
        signature.call_conv = CallConv::triple_default(self.module.isa().triple());
        for param in &function.params {
            let ty = shared_native_type(&param.ty, &self.records, &self.native_enums)?;
            if ty != types::INVALID {
                signature.params.push(AbiParam::new(ty));
            }
        }
        if let Some(ref return_type) = function.return_type {
            let ty = shared_native_type(return_type, &self.records, &self.native_enums)?;
            if ty != types::INVALID {
                signature.returns.push(AbiParam::new(ty));
            }
        }
        ctx.func.signature = signature.clone();

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let block_params = builder.block_params(entry).to_vec();
        let mut values = BTreeMap::<ValueId, NativeValueRepr>::new();
        let mut slot_vars = BTreeMap::<crate::LocalSlotId, Variable>::new();
        let mut slot_types = BTreeMap::<crate::LocalSlotId, types::Type>::new();
        let value_kinds = infer_value_kinds(
            function,
            &self.records,
            &self.native_enums,
            &self.program.functions,
        )?;
        let mut list_headers = BTreeMap::<cranelift_codegen::ir::Value, ListHeader>::new();

        for local in &function.mutable_locals {
            let ty = shared_native_type(&local.ty, &self.records, &self.native_enums)?;
            if ty == types::INVALID {
                continue;
            }
            let var = builder.declare_var(ty);
            slot_vars.insert(local.slot, var);
            slot_types.insert(local.slot, ty);
        }

        let data_ids = BTreeMap::<String, DataId>::new();
        let text_index_helpers = TextIndexHelperIds {
            new_id: None,
            get_id: None,
            contains_id: None,
            get_or_insert_id: None,
            set_id: None,
            keys_id: None,
        };

        let falls_through = lower_insts(
            &self.function_ids,
            &data_ids,
            self.allocator_id,
            self.alloc_push_id,
            self.alloc_pop_id,
            self.text_builder_new_id,
            self.text_builder_append_id,
            self.text_builder_append_codepoint_id,
            self.text_builder_append_ascii_id,
            self.text_builder_append_slice_id,
            self.text_builder_append_i32_id,
            self.text_builder_finish_id,
            self.stdout_write_builder_id,
            &text_index_helpers,
            self.list_new_id,
            self.list_push_id,
            self.list_sort_text_id,
            self.list_sort_by_text_field_id,
            self.text_concat_id,
            self.text_intern_id,
            self.text_slice_id,
            self.bytes_slice_id,
            self.text_eq_range_id,
            self.text_find_byte_range_id,
            self.text_line_end_id,
            self.text_next_line_id,
            self.text_field_end_id,
            self.text_next_field_id,
            self.text_from_f64_fixed_id,
            self.parse_i32_id,
            self.parse_i32_range_id,
            self.parse_f64_id,
            self.arg_count_id,
            self.arg_text_id,
            self.stdin_text_id,
            self.stdout_write_id,
            self.file_open_id,
            self.file_close_id,
            self.file_read_id,
            self.file_read_to_end_id,
            self.file_write_id,
            self.file_seek_id,
            self.file_size_id,
            self.file_exists_id,
            self.file_remove_id,
            self.file_is_valid_id,
            self.bytes_to_text_id,
            self.text_eq_id,
            self.text_cmp_id,
            &self.records,
            &self.native_enums,
            &value_kinds,
            &mut self.module,
            function,
            &mut builder,
            &block_params,
            &slot_vars,
            &slot_types,
            &mut values,
            &mut list_headers,
            &TrustedListAccesses::default(),
            &function.instructions,
            "jit",
        )
        .map_err(|e| format!("failed to lower insts for `{}`: {e}", function.name))?;

        if falls_through {
            if let Some(result_val) = function.result {
                match values.get(&result_val) {
                    Some(NativeValueRepr::Native(val)) if !signature.returns.is_empty() => {
                        builder.ins().return_(&[*val]);
                    }
                    _ => {
                        builder.ins().return_(&[]);
                    }
                }
            } else {
                builder.ins().return_(&[]);
            }
        }

        builder.finalize();
        self.module
            .define_function(id, &mut ctx)
            .map_err(|e| format!("failed to define function `{}`: {e}", function.name))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Main entry point: run_function_native
// ---------------------------------------------------------------------------

/// Run a named function from a MIR program using Cranelift JIT.
///
/// This compiles the program to native code via Cranelift's JITModule,
/// executes the requested function, and returns the result.
pub fn run_function_native(
    program: &Program,
    name: &str,
    args: &[RuntimeValue],
) -> Result<RuntimeValue, RuntimeError> {
    // Preflight via interpreter to validate args and get expected result
    let _interpreter_result = crate::run_function(program, name, args)
        .map_err(|e| RuntimeError::Message(format!("preflight error: {e:?}")))?;

    let function = program
        .functions
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| RuntimeError::Message(format!("function `{name}` not found")))?;

    // Build JIT module
    let mut backend = JitBackend::new(program).map_err(RuntimeError::Message)?;

    // Define all functions (not just the target)
    for func in &program.functions {
        backend
            .define_function(func)
            .map_err(RuntimeError::Message)?;
    }

    // Finalize
    backend
        .module
        .finalize_definitions()
        .map_err(|e| RuntimeError::Message(format!("finalize error: {e:?}")))?;

    // Get function pointer
    let id = backend.function_ids[name];
    let code_ptr = backend.module.get_finalized_function(id);

    // Convert RuntimeValue args to native i64/f64 values
    let records = &backend.records;
    let native_enums = &backend.native_enums;
    let mut native_args: Vec<NativeArg> = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if i < function.params.len() {
            let param_ty = &function.params[i].ty;
            native_args.push(runtime_value_to_native(
                arg,
                param_ty,
                records,
                native_enums,
            ));
        }
    }

    // Determine return type
    let return_type = function.return_type.as_deref();

    // Call the function
    let native_result =
        unsafe { call_native_fn(code_ptr, &native_args, return_type, records, native_enums) };

    // Decode result
    let decoded = native_to_runtime_value(&native_result, return_type, records, native_enums);

    Ok(decoded)
}

// ---------------------------------------------------------------------------
// Argument encoding/decoding helpers
// ---------------------------------------------------------------------------

enum NativeArg {
    I64(i64),
    F64(f64),
    Void,
}

fn runtime_value_to_native(
    value: &RuntimeValue,
    _param_ty: &str,
    _records: &BTreeMap<String, NativeRecord>,
    _native_enums: &BTreeMap<String, NativeEnum>,
) -> NativeArg {
    match value {
        RuntimeValue::Unit => NativeArg::Void,
        RuntimeValue::Int(v) => NativeArg::I64(*v),
        RuntimeValue::F64(v) => NativeArg::F64(*v),
        RuntimeValue::Bool(v) => NativeArg::I64(if *v { 1 } else { 0 }),
        RuntimeValue::Text(v) => {
            if v.is_empty() {
                NativeArg::I64(EMPTY_TEXT.as_ptr() as i64)
            } else {
                let len = v.len() as u64;
                let blob = unsafe { alloc_text(len) };
                unsafe {
                    std::ptr::copy_nonoverlapping(v.as_ptr(), text_data_mut(blob), len as usize);
                }
                NativeArg::I64(blob)
            }
        }
        RuntimeValue::Bytes(v) => {
            let len = v.len() as u64;
            let blob = unsafe { alloc_text(len) };
            unsafe {
                std::ptr::copy_nonoverlapping(v.as_ptr(), text_data_mut(blob), len as usize);
            }
            NativeArg::I64(blob)
        }
        _ => NativeArg::I64(0),
    }
}

enum NativeResult {
    I64(i64),
    F64(f64),
    Void,
}

unsafe fn call_native_fn(
    code_ptr: *const u8,
    args: &[NativeArg],
    return_type: Option<&str>,
    _records: &BTreeMap<String, NativeRecord>,
    _native_enums: &BTreeMap<String, NativeEnum>,
) -> NativeResult {
    unsafe {
        let has_f64_return = return_type == Some("F64");

        match args.len() {
            0 => {
                if has_f64_return {
                    let f: unsafe extern "C" fn() -> f64 = std::mem::transmute(code_ptr);
                    NativeResult::F64(f())
                } else {
                    let f: unsafe extern "C" fn() -> i64 = std::mem::transmute(code_ptr);
                    NativeResult::I64(f())
                }
            }
            1 => {
                let a0 = &args[0];
                match (a0, has_f64_return) {
                    (NativeArg::I64(v0), false) => {
                        let f: unsafe extern "C" fn(i64) -> i64 = std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0))
                    }
                    (NativeArg::I64(v0), true) => {
                        let f: unsafe extern "C" fn(i64) -> f64 = std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0))
                    }
                    (NativeArg::F64(v0), false) => {
                        let f: unsafe extern "C" fn(f64) -> i64 = std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0))
                    }
                    (NativeArg::F64(v0), true) => {
                        let f: unsafe extern "C" fn(f64) -> f64 = std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0))
                    }
                    (NativeArg::Void, _) => NativeResult::Void,
                }
            }
            2 => {
                let a0 = &args[0];
                let a1 = &args[1];
                let v0 = match a0 {
                    NativeArg::I64(v) => *v,
                    NativeArg::F64(v) => v.to_bits() as i64,
                    NativeArg::Void => 0,
                };
                let v1 = match a1 {
                    NativeArg::I64(v) => *v,
                    NativeArg::F64(v) => v.to_bits() as i64,
                    NativeArg::Void => 0,
                };
                if has_f64_return {
                    let f: unsafe extern "C" fn(i64, i64) -> f64 = std::mem::transmute(code_ptr);
                    NativeResult::F64(f(v0, v1))
                } else {
                    let f: unsafe extern "C" fn(i64, i64) -> i64 = std::mem::transmute(code_ptr);
                    NativeResult::I64(f(v0, v1))
                }
            }
            3 => {
                let a0 = &args[0];
                let a1 = &args[1];
                let a2 = &args[2];
                let v0 = match a0 {
                    NativeArg::I64(v) => *v,
                    NativeArg::F64(v) => v.to_bits() as i64,
                    NativeArg::Void => 0,
                };
                let v1 = match a1 {
                    NativeArg::I64(v) => *v,
                    NativeArg::F64(v) => v.to_bits() as i64,
                    NativeArg::Void => 0,
                };
                let v2 = match a2 {
                    NativeArg::I64(v) => *v,
                    NativeArg::F64(v) => v.to_bits() as i64,
                    NativeArg::Void => 0,
                };
                if has_f64_return {
                    let f: unsafe extern "C" fn(i64, i64, i64) -> f64 =
                        std::mem::transmute(code_ptr);
                    NativeResult::F64(f(v0, v1, v2))
                } else {
                    let f: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                        std::mem::transmute(code_ptr);
                    NativeResult::I64(f(v0, v1, v2))
                }
            }
            _ => NativeResult::I64(0),
        }
    }
}

fn native_to_runtime_value(
    result: &NativeResult,
    return_type: Option<&str>,
    _records: &BTreeMap<String, NativeRecord>,
    _native_enums: &BTreeMap<String, NativeEnum>,
) -> RuntimeValue {
    match (result, return_type) {
        (NativeResult::Void, _) => RuntimeValue::Unit,
        (NativeResult::I64(_v), None | Some("Unit")) => RuntimeValue::Unit,
        (NativeResult::I64(v), Some("I32")) => RuntimeValue::Int(*v as i32 as i64),
        (NativeResult::I64(v), Some("Bool")) => RuntimeValue::Bool(*v != 0),
        (NativeResult::I64(v), Some("F64")) => RuntimeValue::F64(f64::from_bits(*v as u64)),
        (NativeResult::F64(v), Some("F64")) => RuntimeValue::F64(*v),
        (NativeResult::I64(v), Some("Text")) => {
            let ptr = *v as *const u8;
            if ptr.is_null() || ptr == EMPTY_TEXT.as_ptr() {
                RuntimeValue::Text(String::new())
            } else {
                let len = unsafe { std::ptr::read_unaligned(ptr as *const u64) } as usize;
                let data = unsafe { std::slice::from_raw_parts(ptr.add(8), len) };
                RuntimeValue::Text(String::from_utf8_lossy(data).into_owned())
            }
        }
        (NativeResult::I64(v), Some("Bytes")) => {
            let ptr = *v as *const u8;
            if ptr.is_null() || ptr == EMPTY_TEXT.as_ptr() {
                RuntimeValue::Bytes(Vec::new())
            } else {
                let len = unsafe { std::ptr::read_unaligned(ptr as *const u64) } as usize;
                let data = unsafe { std::slice::from_raw_parts(ptr.add(8), len) };
                RuntimeValue::Bytes(data.to_vec())
            }
        }
        (NativeResult::F64(v), _) => RuntimeValue::F64(*v),
        (NativeResult::I64(_), _) => RuntimeValue::Unit,
    }
}
