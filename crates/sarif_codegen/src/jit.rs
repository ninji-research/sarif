#![allow(unsafe_code, unsafe_op_in_unsafe_fn, unused_unsafe)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File as RustFile;
use std::io::{self, Read as RustRead, Seek as RustSeek, SeekFrom, Write as RustWrite};

use cranelift_codegen::ir::{AbiParam, InstBuilder, UserFuncName, types};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataId, FuncId, Linkage, Module, default_libcall_names};

use crate::native::{
    ListHeader, NativeEnum, NativeRecord, NativeValueRepr, RuntimeHelperIds, TrustedListAccesses,
    collect_native_enums, collect_native_records, declare_runtime_helpers, encode_text_blob,
    infer_value_kinds, lower_insts, native_type as shared_native_type,
};
use crate::{Function, Inst, Program, RuntimeError, RuntimeValue, ValueId};

// ---------------------------------------------------------------------------
// Arena allocator (thread-local stable-pointer bump arena with scope stack)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct ArenaState {
    chunk_index: usize,
    offset: usize,
}

struct ScopedArena {
    chunks: Vec<Box<[u8]>>,
    current_chunk_idx: usize,
    current_offset: usize,
}

impl ScopedArena {
    fn new() -> Self {
        Self {
            chunks: Vec::new(),
            current_chunk_idx: 0,
            current_offset: 0,
        }
    }

    fn alloc(&mut self, size: usize) -> *mut u8 {
        let aligned = arena_align_up(size);

        if self.current_chunk_idx < self.chunks.len()
            && self.current_offset + aligned <= self.chunks[self.current_chunk_idx].len()
        {
            let ptr = unsafe {
                self.chunks[self.current_chunk_idx]
                    .as_mut_ptr()
                    .add(self.current_offset)
            };
            self.current_offset += aligned;
            return ptr;
        }

        let chunk_size = 4 * 1024 * 1024; // 4 MB chunks
        let new_chunk_size = chunk_size.max(aligned);
        let mut new_chunk = vec![0u8; new_chunk_size].into_boxed_slice();
        let ptr = new_chunk.as_mut_ptr();

        if self.chunks.is_empty() {
            self.chunks.push(new_chunk);
            self.current_chunk_idx = 0;
        } else {
            self.chunks.truncate(self.current_chunk_idx + 1);
            self.chunks.push(new_chunk);
            self.current_chunk_idx = self.chunks.len() - 1;
        }
        self.current_offset = aligned;
        ptr
    }

    fn push(&mut self) -> ArenaState {
        ArenaState {
            chunk_index: self.current_chunk_idx,
            offset: self.current_offset,
        }
    }

    fn pop(&mut self, state: ArenaState) {
        self.current_chunk_idx = state.chunk_index;
        self.current_offset = state.offset;
    }
}

const ARENA_ALIGN: usize = 16;
static EMPTY_TEXT: [u8; 8] = [0u8; 8];

thread_local! {
    static ARENA: RefCell<ScopedArena> = RefCell::new(ScopedArena::new());
    static SCOPE_STACK: RefCell<Vec<ArenaState>> = const { RefCell::new(Vec::new()) };
    static PROGRAM_ARGS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    static STDIN_TEXT: RefCell<Option<String>> = const { RefCell::new(None) };
    static STDOUT_BUF: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    static TEXT_DATA_TABLE: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
}

fn arena_align_up(n: usize) -> usize {
    (n + ARENA_ALIGN - 1) & !(ARENA_ALIGN - 1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_record_alloc(size: i64) -> i64 {
    unsafe {
        let n = size.max(8) as usize;
        ARENA.with(|arena| arena.borrow_mut().alloc(n) as i64)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_alloc_push() {
    unsafe {
        ARENA.with(|arena| {
            SCOPE_STACK.with(|stack| {
                let state = arena.borrow_mut().push();
                stack.borrow_mut().push(state);
            });
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_alloc_pop() {
    unsafe {
        SCOPE_STACK.with(|stack| {
            if let Some(state) = stack.borrow_mut().pop() {
                ARENA.with(|arena| {
                    arena.borrow_mut().pop(state);
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_len(ptr: i64) -> i64 {
    unsafe { text_len(ptr) as i64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_byte(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        if len == 0 {
            return 0;
        }
        let idx = index.max(0).min(len as i64 - 1) as usize;
        i64::from(*text_data(ptr).add(idx))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_materialize(ptr: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let len = text_len(ptr);
        let result = alloc_text(len);
        std::ptr::copy_nonoverlapping(text_data(ptr), text_data_mut(result), len as usize);
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_i32(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 4 > len as usize {
            return 0;
        }
        i64::from(std::ptr::read_unaligned(text_data(ptr).add(idx) as *const i32))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_store_i32(ptr: i64, index: i64, value: i64) {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 4 > len as usize {
            return;
        }
        std::ptr::write_unaligned(text_data_mut(ptr).add(idx) as *mut i32, value as i32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_i64(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 8 > len as usize {
            return 0;
        }
        std::ptr::read_unaligned(text_data(ptr).add(idx) as *const i64)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_store_i64(ptr: i64, index: i64, value: i64) {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 8 > len as usize {
            return;
        }
        std::ptr::write_unaligned(text_data_mut(ptr).add(idx) as *mut i64, value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_slice_i64(ptr: i64, start: i64, length: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() || start < 0 || length < 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let src_len = text_len(ptr);
        let s = (start as u64).min(src_len);
        let cl = (length as u64).min(src_len - s);
        if cl == 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let result = alloc_text(cl);
        std::ptr::copy_nonoverlapping(
            text_data(ptr).add(s as usize),
            text_data_mut(result),
            cl as usize,
        );
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_i32_i64(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 4 > len as usize {
            return 0;
        }
        std::ptr::read_unaligned(text_data(ptr).add(idx) as *const i32) as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_byte_i64(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        if index < 0 || index >= len as i64 {
            return 0;
        }
        let idx = index as usize;
        i64::from(*text_data(ptr).add(idx))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_i64_i64(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 8 > len as usize {
            return 0;
        }
        std::ptr::read_unaligned(text_data(ptr).add(idx) as *const i64)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_f32_i64(ptr: i64, index: i64) -> f64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0.0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 4 > len as usize {
            return 0.0;
        }
        let val = std::ptr::read_unaligned(text_data(ptr).add(idx) as *const f32);
        val as f64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_f64(ptr: i64, index: i64) -> f64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0.0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 8 > len as usize {
            return 0.0;
        }
        std::ptr::read_unaligned(text_data(ptr).add(idx) as *const f64)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_store_f64(ptr: i64, index: i64, value: f64) {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 8 > len as usize {
            return;
        }
        std::ptr::write_unaligned(text_data_mut(ptr).add(idx) as *mut f64, value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_load_bool(ptr: i64, index: i64) -> i64 {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return 0;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 1 > len as usize {
            return 0;
        }
        if std::ptr::read_unaligned(text_data(ptr).add(idx) as *const bool) {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_store_bool(ptr: i64, index: i64, value: i64) {
    unsafe {
        if ptr == 0 || ptr as *const u8 == EMPTY_TEXT.as_ptr() {
            return;
        }
        let len = text_len(ptr);
        let idx = index as usize;
        if idx + 1 > len as usize {
            return;
        }
        std::ptr::write_unaligned(text_data_mut(ptr).add(idx) as *mut bool, value != 0);
    }
}

// ---------------------------------------------------------------------------
// Stdout/stderr helpers
// ---------------------------------------------------------------------------

fn write_stdout(data: &[u8]) {
    STDOUT_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        if let Some(ref mut captured) = *buf {
            captured.extend_from_slice(data);
        } else {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            let _ = handle.write_all(data);
            let _ = handle.flush();
        }
    });
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
        let text = STDIN_TEXT.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(text) = s.take() {
                text
            } else {
                let mut buf = Vec::new();
                let _ = io::stdin().read_to_end(&mut buf);
                String::from_utf8(buf).unwrap_or_default()
            }
        });
        let len = text.len() as u64;
        let result = alloc_text(len);
        std::ptr::copy_nonoverlapping(text.as_ptr(), text_data_mut(result), len as usize);
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
    field_offset: i64,
) -> i64 {
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
        slice.sort_by(|&a, &b| {
            if a == b {
                return std::cmp::Ordering::Equal;
            }
            if a == 0 {
                return std::cmp::Ordering::Less;
            }
            if b == 0 {
                return std::cmp::Ordering::Greater;
            }
            let text_a = unsafe { *((a as *const u8).add(field_offset as usize) as *const u64) };
            let text_b = unsafe { *((b as *const u8).add(field_offset as usize) as *const u64) };
            let cmp = unsafe { sarif_text_cmp(text_a as i64, text_b as i64) };
            cmp.cmp(&0)
        });
        list as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_sort_by_i32_field(
    list: i64,
    logical_len: i64,
    field_offset: i64,
) -> i64 {
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
        slice.sort_by(|&a, &b| {
            if a == b {
                return std::cmp::Ordering::Equal;
            }
            if a == 0 {
                return std::cmp::Ordering::Less;
            }
            if b == 0 {
                return std::cmp::Ordering::Greater;
            }
            let val_a = unsafe { *((a as *const u8).add(field_offset as usize) as *const i64) };
            let val_b = unsafe { *((b as *const u8).add(field_offset as usize) as *const i64) };
            val_a.cmp(&val_b)
        });
        list as i64
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_list_sort_by_f64_field(
    list: i64,
    logical_len: i64,
    field_offset: i64,
) -> i64 {
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
        slice.sort_by(|&a, &b| {
            if a == b {
                return std::cmp::Ordering::Equal;
            }
            if a == 0 {
                return std::cmp::Ordering::Less;
            }
            if b == 0 {
                return std::cmp::Ordering::Greater;
            }
            let val_a = unsafe { *((a as *const u8).add(field_offset as usize) as *const f64) };
            let val_b = unsafe { *((b as *const u8).add(field_offset as usize) as *const f64) };
            val_a
                .partial_cmp(&val_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        list as i64
    }
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
        if cap == 0 || !cap.is_power_of_two() {
            return 0;
        }
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
        if cap == 0 || !cap.is_power_of_two() {
            return 0;
        }
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
            if cap == 0 || !cap.is_power_of_two() {
                return 0;
            }
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
            if cap == 0 || !cap.is_power_of_two() {
                return index;
            }
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
pub unsafe extern "C" fn sarif_text_eq_range(
    source: i64,
    start: i64,
    end: i64,
    expected: i64,
) -> i64 {
    unsafe {
        if source == 0 || expected == 0 {
            return 0;
        }
        let source_len = text_len(source);
        let expected_len = text_len(expected);
        let s = start.max(0).min(source_len as i64) as u64;
        let e = end.max(s as i64).min(source_len as i64) as u64;
        if (e - s) != expected_len {
            return 0;
        }
        if expected_len == 0 {
            return 1;
        }
        let src_slice =
            std::slice::from_raw_parts(text_data(source).add(s as usize), expected_len as usize);
        let exp_slice = std::slice::from_raw_parts(text_data(expected), expected_len as usize);
        if src_slice == exp_slice { 1 } else { 0 }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_find_byte_range(
    source: i64,
    start: i64,
    end: i64,
    byte: i64,
) -> i64 {
    unsafe {
        if source == 0 {
            return end;
        }
        let source_len = text_len(source);
        let s = start.max(0).min(source_len as i64) as usize;
        let e = end.max(s as i64).min(source_len as i64) as usize;
        if s >= e {
            return end;
        }
        let bytes = text_data(source);
        let data = std::slice::from_raw_parts(bytes, source_len as usize);
        let needle = byte as u8;
        if let Some(pos) = data[s..e].iter().position(|&b| b == needle) {
            (s + pos) as i64
        } else {
            end
        }
    }
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
pub unsafe extern "C" fn sarif_text_field_end(source: i64, start: i64, end: i64, byte: i64) -> i64 {
    unsafe { sarif_text_find_byte_range(source, start, end, byte) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_next_field(
    source: i64,
    start: i64,
    end: i64,
    byte: i64,
) -> i64 {
    unsafe {
        if source == 0 {
            return end;
        }
        let source_len = text_len(source) as i64;
        let field_end = sarif_text_find_byte_range(source, start, end, byte);
        if field_end < end && field_end < source_len {
            field_end + 1
        } else {
            field_end
        }
    }
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
            format!("{:.precision$}", value, precision = d)
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
pub unsafe extern "C" fn sarif_parse_i32_range(text: i64, start: i64, end: i64) -> i64 {
    unsafe {
        if text == 0 {
            return 0;
        }
        let len = text_len(text) as usize;
        let s = start.max(0).min(len as i64) as usize;
        let e = end.max(s as i64).min(len as i64) as usize;
        if s >= e {
            return 0;
        }
        let bytes = text_data(text);
        let data = std::slice::from_raw_parts(bytes, len);
        let mut idx = s;
        let mut last = e;
        while idx < last && data[idx] == b' ' {
            idx += 1;
        }
        while last > idx && data[last - 1] == b' ' {
            last -= 1;
        }
        if idx >= last {
            return 0;
        }
        let s = std::str::from_utf8(&data[idx..last]);
        match s {
            Ok(s) => s.parse::<i32>().unwrap_or(0) as i64,
            Err(_) => 0,
        }
    }
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
    PROGRAM_ARGS.with(|args| {
        let args = args.borrow();
        if let Some(ref program_args) = *args {
            program_args.len() as i64
        } else {
            std::env::args().len() as i64
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_arg_text(index: i64) -> i64 {
    unsafe {
        if index < 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let idx = index as usize;
        let text = PROGRAM_ARGS.with(|args| {
            let args = args.borrow();
            if let Some(ref program_args) = *args {
                if idx < program_args.len() {
                    Some(program_args[idx].clone())
                } else {
                    None
                }
            } else {
                let env_args: Vec<String> = std::env::args().collect();
                if idx < env_args.len() {
                    Some(env_args[idx].clone())
                } else {
                    None
                }
            }
        });
        match text {
            Some(s) => {
                let len = s.len() as u64;
                let blob = alloc_text(len);
                std::ptr::copy_nonoverlapping(s.as_ptr(), text_data_mut(blob), len as usize);
                blob
            }
            None => EMPTY_TEXT.as_ptr() as i64,
        }
    }
}

// ---------------------------------------------------------------------------
// File I/O (real POSIX I/O via Rust std::fs)
// ---------------------------------------------------------------------------

thread_local! {
    static FILE_TABLE: RefCell<Vec<Option<RustFile>>> = const { RefCell::new(Vec::new()) };
}

fn file_handle_to_id(f: RustFile) -> i64 {
    FILE_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        for (i, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(f);
                return (i as i64) + 1;
            }
        }
        let id = table.len() as i64 + 1;
        table.push(Some(f));
        id
    })
}

fn id_to_file(id: i64) -> Option<RustFile> {
    if id <= 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    FILE_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if idx < table.len() {
            table[idx].take()
        } else {
            None
        }
    })
}

fn with_file<F, R>(id: i64, f: F) -> Option<R>
where
    F: FnOnce(&mut RustFile) -> R,
{
    if id <= 0 {
        return None;
    }
    let idx = (id - 1) as usize;
    FILE_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if idx < table.len() {
            table[idx].as_mut().map(f)
        } else {
            None
        }
    })
}

unsafe fn text_to_str(ptr: i64) -> Option<String> {
    if ptr == 0 {
        return None;
    }
    let len = text_len(ptr) as usize;
    let data = std::slice::from_raw_parts(text_data(ptr), len);
    std::str::from_utf8(data).map(|s| s.to_owned()).ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_open(path: i64, mode: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        let Some(mode_str) = text_to_str(mode) else {
            return 0;
        };
        let rust_mode = match mode_str.as_str() {
            "r" | "rb" => "r",
            "w" | "wb" => "w",
            "a" | "ab" => "a",
            "r+" | "rb+" | "r+b" => "r+",
            "w+" | "wb+" | "w+b" => "w+",
            "a+" | "ab+" | "a+b" => "a+",
            _ => "r",
        };
        let file = std::fs::OpenOptions::new()
            .read(rust_mode.contains('r') || rust_mode.contains('+'))
            .write(rust_mode.contains('w') || rust_mode.contains('a') || rust_mode.contains('+'))
            .append(rust_mode.starts_with('a'))
            .create(rust_mode.contains('w') || rust_mode.contains('a'))
            .truncate(rust_mode.starts_with('w') && !rust_mode.contains('+'))
            .open(&path_str)
            .ok();
        match file {
            Some(f) => file_handle_to_id(f),
            None => 0,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_close(handle: i64) {
    unsafe {
        let _ = id_to_file(handle);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_read(handle: i64, count: i64) -> i64 {
    unsafe {
        if handle <= 0 || count < 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let count = count as usize;
        let result = with_file(handle, |file| {
            let mut buf = vec![0u8; count];
            match file.read(&mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    Some(buf)
                }
                Err(_) => None,
            }
        });
        let Some(buf) = result.flatten() else {
            return EMPTY_TEXT.as_ptr() as i64;
        };
        let len = buf.len() as u64;
        let blob = alloc_text(len);
        std::ptr::copy_nonoverlapping(buf.as_ptr(), text_data_mut(blob), len as usize);
        blob
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_read_to_end(handle: i64) -> i64 {
    unsafe {
        if handle <= 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let result = with_file(handle, |file| {
            let mut buf = Vec::new();
            match file.read_to_end(&mut buf) {
                Ok(_) => Some(buf),
                Err(_) => None,
            }
        });
        let Some(buf) = result.flatten() else {
            return EMPTY_TEXT.as_ptr() as i64;
        };
        let len = buf.len() as u64;
        let blob = alloc_text(len);
        std::ptr::copy_nonoverlapping(buf.as_ptr(), text_data_mut(blob), len as usize);
        blob
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_write(handle: i64, data: i64) -> i64 {
    unsafe {
        if handle <= 0 || data == 0 {
            return 0;
        }
        let len = text_len(data) as usize;
        let bytes = std::slice::from_raw_parts(text_data(data), len);
        let result = with_file(handle, |file| match file.write_all(bytes) {
            Ok(()) => {
                let _ = file.flush();
                len as i64
            }
            Err(_) => 0,
        });
        result.unwrap_or(0)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_seek(handle: i64, offset: i64) -> i64 {
    unsafe {
        if handle <= 0 {
            return -1;
        }
        let result = with_file(handle, |file| {
            file.seek(SeekFrom::Current(offset)).ok().map(|p| p as i64)
        });
        result.flatten().unwrap_or(-1)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_size(handle: i64) -> i64 {
    unsafe {
        if handle <= 0 {
            return -1;
        }
        let result = with_file(handle, |file| {
            let cur = file.stream_position().ok()?;
            let end = file.seek(SeekFrom::End(0)).ok()?;
            let _ = file.seek(SeekFrom::Start(cur));
            Some(end as i64)
        });
        result.flatten().unwrap_or(-1)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_exists(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        if std::path::Path::new(&path_str).exists() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_remove(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        if std::fs::remove_file(&path_str).is_ok() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_is_valid(handle: i64) -> i64 {
    if handle > 0 { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_env_get(key: i64) -> i64 {
    unsafe {
        let Some(key_str) = text_to_str(key) else {
            return EMPTY_TEXT.as_ptr() as i64;
        };
        match std::env::var(key_str.as_str()) {
            Ok(val) => {
                let val_bytes = val.as_bytes();
                let result = alloc_text(val_bytes.len() as u64);
                std::ptr::copy_nonoverlapping(
                    val_bytes.as_ptr(),
                    text_data_mut(result),
                    val_bytes.len(),
                );
                result
            }
            Err(_) => EMPTY_TEXT.as_ptr() as i64,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_env_set(key: i64, value: i64) -> i64 {
    unsafe {
        let Some(key_str) = text_to_str(key) else {
            return 0;
        };
        let Some(val_str) = text_to_str(value) else {
            return 0;
        };
        std::env::set_var(key_str.as_str(), val_str.as_str());
        1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_env_remove(key: i64) -> i64 {
    unsafe {
        let Some(key_str) = text_to_str(key) else {
            return 0;
        };
        std::env::remove_var(key_str.as_str());
        1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_env_keys() -> i64 {
    unsafe {
        let keys: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
        let joined = keys.join("\n");
        let bytes = joined.as_bytes();
        let result = alloc_text(bytes.len() as u64);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), text_data_mut(result), bytes.len());
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_dir_create(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        if std::fs::create_dir_all(&path_str).is_ok() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_dir_remove(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        if std::fs::remove_dir_all(&path_str).is_ok() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_dir_list(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return EMPTY_TEXT.as_ptr() as i64;
        };
        let entries = match std::fs::read_dir(&path_str) {
            Ok(rd) => rd,
            Err(_) => return EMPTY_TEXT.as_ptr() as i64,
        };
        let names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        let joined = names.join("\n");
        let bytes = joined.as_bytes();
        let result = alloc_text(bytes.len() as u64);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), text_data_mut(result), bytes.len());
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_dir_exists(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        if std::path::Path::new(&path_str).is_dir() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_dir_current() -> i64 {
    unsafe {
        match std::env::current_dir() {
            Ok(cwd) => {
                let Some(cwd_str) = cwd.to_str() else {
                    return EMPTY_TEXT.as_ptr() as i64;
                };
                let bytes = cwd_str.as_bytes();
                let result = alloc_text(bytes.len() as u64);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), text_data_mut(result), bytes.len());
                result
            }
            Err(_) => EMPTY_TEXT.as_ptr() as i64,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_dir_change(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return 0;
        };
        if std::env::set_current_dir(&path_str).is_ok() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_process_exit(code: i64) {
    std::process::exit(code as i32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_process_id() -> i64 {
    std::process::id() as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_clock_now() -> f64 {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1e9
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_clock_sleep(ms: i64) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_bytes_to_text(bytes: i64) -> i64 {
    unsafe {
        if bytes == 0 {
            return EMPTY_TEXT.as_ptr() as i64;
        }
        let len = text_len(bytes) as usize;
        let result = alloc_text(len as u64);
        std::ptr::copy_nonoverlapping(text_data(bytes), text_data_mut(result), len);
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_to_bytes(text: i64) -> i64 {
    unsafe {
        if text == 0 {
            return 0;
        }
        let len = text_len(text) as usize;
        let result = alloc_text(len as u64);
        std::ptr::copy_nonoverlapping(text_data(text), text_data_mut(result), len);
        result
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_text_data_for_insts(idx: i64) -> i64 {
    TEXT_DATA_TABLE.with(|table| {
        let table = table.borrow();
        if idx >= 0 && (idx as usize) < table.len() {
            table[idx as usize]
        } else {
            EMPTY_TEXT.as_ptr() as i64
        }
    })
}

/// Collect unique text strings from ConstText and EnumToText instructions
/// across all functions, preserving insertion order.
fn collect_text_data_strings(program: &Program) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for function in &program.functions {
        collect_inst_text_data(&function.instructions, &mut seen, &mut result);
    }
    result
}

fn collect_inst_text_data(
    insts: &[Inst],
    seen: &mut std::collections::BTreeSet<String>,
    result: &mut Vec<String>,
) {
    for inst in insts {
        match inst {
            Inst::ConstText { value, .. } if seen.insert(value.clone()) => {
                result.push(value.clone());
            }
            Inst::ConstText { .. } => {}
            Inst::EnumToText { variant_names, .. } => {
                for name in variant_names {
                    if seen.insert(name.clone()) {
                        result.push(name.clone());
                    }
                }
            }
            Inst::If {
                then_insts,
                else_insts,
                ..
            } => {
                collect_inst_text_data(then_insts, seen, result);
                collect_inst_text_data(else_insts, seen, result);
            }
            Inst::While {
                condition_insts,
                body_insts,
                ..
            } => {
                collect_inst_text_data(condition_insts, seen, result);
                collect_inst_text_data(body_insts, seen, result);
            }
            Inst::Repeat { body_insts, .. } => {
                collect_inst_text_data(body_insts, seen, result);
            }
            _ => {}
        }
    }
}

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
        ("sarif_bytes_len", sarif_bytes_len as *const u8),
        ("sarif_bytes_byte", sarif_bytes_byte as *const u8),
        (
            "sarif_bytes_materialize",
            sarif_bytes_materialize as *const u8,
        ),
        ("sarif_bytes_load_i32", sarif_bytes_load_i32 as *const u8),
        ("sarif_bytes_store_i32", sarif_bytes_store_i32 as *const u8),
        ("sarif_bytes_load_i64", sarif_bytes_load_i64 as *const u8),
        ("sarif_bytes_store_i64", sarif_bytes_store_i64 as *const u8),
("sarif_bytes_load_i32_i64", sarif_bytes_load_i32_i64 as *const u8),
("sarif_bytes_load_i64_i64", sarif_bytes_load_i64_i64 as *const u8),
("sarif_bytes_load_f32_i64", sarif_bytes_load_f32_i64 as *const u8),
("sarif_bytes_byte_i64", sarif_bytes_byte_i64 as *const u8),
        ("sarif_bytes_slice_i64", sarif_bytes_slice_i64 as *const u8),
        ("sarif_bytes_load_f64", sarif_bytes_load_f64 as *const u8),
        ("sarif_bytes_store_f64", sarif_bytes_store_f64 as *const u8),
        ("sarif_bytes_load_bool", sarif_bytes_load_bool as *const u8),
        ("sarif_bytes_store_bool", sarif_bytes_store_bool as *const u8),
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
        (
            "sarif_list_sort_by_i32_field",
            sarif_list_sort_by_i32_field as *const u8,
        ),
        (
            "sarif_list_sort_by_f64_field",
            sarif_list_sort_by_f64_field as *const u8,
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
        ("sarif_file_mmap", sarif_file_mmap as *const u8),
        ("sarif_file_write", sarif_file_write as *const u8),
        ("sarif_file_seek", sarif_file_seek as *const u8),
        ("sarif_file_size", sarif_file_size as *const u8),
        ("sarif_file_exists", sarif_file_exists as *const u8),
        ("sarif_file_remove", sarif_file_remove as *const u8),
        ("sarif_file_is_valid", sarif_file_is_valid as *const u8),
        ("sarif_env_get", sarif_env_get as *const u8),
        ("sarif_env_set", sarif_env_set as *const u8),
        ("sarif_env_remove", sarif_env_remove as *const u8),
        ("sarif_env_keys", sarif_env_keys as *const u8),
        ("sarif_dir_create", sarif_dir_create as *const u8),
        ("sarif_dir_remove", sarif_dir_remove as *const u8),
        ("sarif_dir_list", sarif_dir_list as *const u8),
        ("sarif_dir_exists", sarif_dir_exists as *const u8),
        ("sarif_dir_current", sarif_dir_current as *const u8),
        ("sarif_dir_change", sarif_dir_change as *const u8),
        ("sarif_process_exit", sarif_process_exit as *const u8),
        ("sarif_process_id", sarif_process_id as *const u8),
        ("sarif_clock_now", sarif_clock_now as *const u8),
        ("sarif_clock_sleep", sarif_clock_sleep as *const u8),
        ("sarif_bytes_to_text", sarif_bytes_to_text as *const u8),
        ("sarif_text_to_bytes", sarif_text_to_bytes as *const u8),
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
    helpers: RuntimeHelperIds,
    text_data_index: BTreeMap<String, usize>,
    text_data_func_id: FuncId,
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

        let helpers = declare_runtime_helpers(&mut module, "jit")
            .map_err(|e| format!("failed to declare runtime helpers: {e}"))?;

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

        // Collect text data strings for ConstText/EnumToText instructions
        let text_data_list = collect_text_data_strings(program);
        let text_data_index: BTreeMap<String, usize> = text_data_list
            .into_iter()
            .enumerate()
            .map(|(i, s)| (s, i))
            .collect();
        let mut text_data_sig = module.make_signature();
        text_data_sig.call_conv = CallConv::triple_default(module.isa().triple());
        text_data_sig.params.push(AbiParam::new(types::I64));
        text_data_sig.returns.push(AbiParam::new(types::I64));
        let text_data_func_id = module
            .declare_function("sarif_text_data_for_insts", Linkage::Import, &text_data_sig)
            .map_err(|e| format!("failed to declare text_data_for_insts: {e}"))?;

        Ok(Self {
            program,
            module,
            function_ids,
            helpers,
            text_data_index,
            text_data_func_id,
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
            &self.program.externs,
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

        let empty_data_ids = BTreeMap::<String, DataId>::new();
        let falls_through = lower_insts(
            &self.function_ids,
            &empty_data_ids,
            Some(self.text_data_func_id),
            &self.text_data_index,
            &self.helpers,
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
                    _ if !signature.returns.is_empty() => {
                        return Err(format!(
                            "function `{}` has return value in signature but no computed value for result",
                            function.name
                        ));
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

    fn define_data_objects(&mut self) -> Result<(), String> {
        TEXT_DATA_TABLE.with(|table| {
            let mut table = table.borrow_mut();
            table.clear();
            let index: BTreeMap<&String, &usize> = self.text_data_index.iter().collect();
            let mut entries: Vec<(&String, &usize)> = index.into_iter().collect();
            entries.sort_by_key(|(_, idx)| **idx);
            for (value, _) in &entries {
                let blob = encode_text_blob(value).into_boxed_slice();
                let ptr = Box::leak(blob).as_ptr() as i64;
                table.push(ptr);
            }
        });
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sarif_file_mmap(path: i64) -> i64 {
    unsafe {
        let Some(path_str) = text_to_str(path) else {
            return EMPTY_TEXT.as_ptr() as i64;
        };
        let Ok(buf) = std::fs::read(&*path_str) else {
            return EMPTY_TEXT.as_ptr() as i64;
        };
        let len = buf.len() as u64;
        let blob = alloc_text(len);
        std::ptr::copy_nonoverlapping(buf.as_ptr(), text_data_mut(blob), len as usize);
        blob
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
    // let _interpreter_result = crate::run_function(program, name, args)
    //     .map_err(|e| RuntimeError::Message(format!("preflight error: {e:?}")))?;

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

    // Define text data objects (constant strings used in instructions)
    backend
        .define_data_objects()
        .map_err(|e| RuntimeError::Message(format!("data define error: {e}")))?;

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

/// Run the program's `main` function using the Cranelift JIT backend,
/// with captured stdout and configurable program args / stdin.
///
/// This mirrors `run_main_with_io_capture` from the interpreter path.
pub fn run_main_native_with_io_capture(
    program: &Program,
    program_args: &[String],
    stdin_text: String,
) -> Result<(RuntimeValue, String), RuntimeError> {
    PROGRAM_ARGS.with(|a| *a.borrow_mut() = Some(program_args.to_vec()));
    STDIN_TEXT.with(|s| *s.borrow_mut() = Some(stdin_text));
    STDOUT_BUF.with(|b| *b.borrow_mut() = Some(Vec::new()));

    let result = run_function_native(program, "main", &[]);

    let stdout = STDOUT_BUF.with(|b| {
        let buf = b.borrow_mut().take().unwrap_or_default();
        String::from_utf8(buf).unwrap_or_default()
    });

    PROGRAM_ARGS.with(|a| *a.borrow_mut() = None);
    STDIN_TEXT.with(|s| *s.borrow_mut() = None);

    result.map(|v| (v, stdout))
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
                match (a0, a1, has_f64_return) {
                    (NativeArg::I64(v0), NativeArg::I64(v1), false) => {
                        let f: unsafe extern "C" fn(i64, i64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1))
                    }
                    (NativeArg::I64(v0), NativeArg::I64(v1), true) => {
                        let f: unsafe extern "C" fn(i64, i64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1))
                    }
                    (NativeArg::F64(v0), NativeArg::I64(v1), false) => {
                        let f: unsafe extern "C" fn(f64, i64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1))
                    }
                    (NativeArg::F64(v0), NativeArg::I64(v1), true) => {
                        let f: unsafe extern "C" fn(f64, i64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1))
                    }
                    (NativeArg::I64(v0), NativeArg::F64(v1), false) => {
                        let f: unsafe extern "C" fn(i64, f64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1))
                    }
                    (NativeArg::I64(v0), NativeArg::F64(v1), true) => {
                        let f: unsafe extern "C" fn(i64, f64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1))
                    }
                    (NativeArg::F64(v0), NativeArg::F64(v1), false) => {
                        let f: unsafe extern "C" fn(f64, f64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1))
                    }
                    (NativeArg::F64(v0), NativeArg::F64(v1), true) => {
                        let f: unsafe extern "C" fn(f64, f64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1))
                    }
                    (NativeArg::Void, _, _) | (_, NativeArg::Void, _) => NativeResult::Void,
                }
            }
            3 => {
                let a0 = &args[0];
                let a1 = &args[1];
                let a2 = &args[2];
                match (a0, a1, a2, has_f64_return) {
                    (NativeArg::I64(v0), NativeArg::I64(v1), NativeArg::I64(v2), false) => {
                        let f: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::I64(v1), NativeArg::I64(v2), true) => {
                        let f: unsafe extern "C" fn(i64, i64, i64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::I64(v1), NativeArg::I64(v2), false) => {
                        let f: unsafe extern "C" fn(f64, i64, i64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::I64(v1), NativeArg::I64(v2), true) => {
                        let f: unsafe extern "C" fn(f64, i64, i64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::F64(v1), NativeArg::I64(v2), false) => {
                        let f: unsafe extern "C" fn(i64, f64, i64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::F64(v1), NativeArg::I64(v2), true) => {
                        let f: unsafe extern "C" fn(i64, f64, i64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::I64(v1), NativeArg::F64(v2), false) => {
                        let f: unsafe extern "C" fn(i64, i64, f64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::I64(v1), NativeArg::F64(v2), true) => {
                        let f: unsafe extern "C" fn(i64, i64, f64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::F64(v1), NativeArg::I64(v2), false) => {
                        let f: unsafe extern "C" fn(f64, f64, i64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::F64(v1), NativeArg::I64(v2), true) => {
                        let f: unsafe extern "C" fn(f64, f64, i64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::I64(v1), NativeArg::F64(v2), false) => {
                        let f: unsafe extern "C" fn(f64, i64, f64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::I64(v1), NativeArg::F64(v2), true) => {
                        let f: unsafe extern "C" fn(f64, i64, f64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::F64(v1), NativeArg::F64(v2), false) => {
                        let f: unsafe extern "C" fn(i64, f64, f64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::I64(v0), NativeArg::F64(v1), NativeArg::F64(v2), true) => {
                        let f: unsafe extern "C" fn(i64, f64, f64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::F64(v1), NativeArg::F64(v2), false) => {
                        let f: unsafe extern "C" fn(f64, f64, f64) -> i64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::I64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::F64(v0), NativeArg::F64(v1), NativeArg::F64(v2), true) => {
                        let f: unsafe extern "C" fn(f64, f64, f64) -> f64 =
                            std::mem::transmute(code_ptr);
                        NativeResult::F64(f(*v0, *v1, *v2))
                    }
                    (NativeArg::Void, _, _, _)
                    | (_, NativeArg::Void, _, _)
                    | (_, _, NativeArg::Void, _) => NativeResult::Void,
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
