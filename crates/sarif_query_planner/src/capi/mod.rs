#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_double, c_long};
use std::ptr;

use crate::{SarifPlan, SarifQuery, SarifResult, open_database, prepare_query};

#[unsafe(no_mangle)]
pub extern "C" fn sarif_open(path: *const c_char) -> *mut SarifQuery {
    if path.is_null() {
        return ptr::null_mut();
    }
    let c_str = unsafe { CStr::from_ptr(path) };
    open_database(c_str.to_str().unwrap_or(""))
        .map_or(ptr::null_mut(), |query| Box::into_raw(Box::new(query)))
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_prepare(db: *mut SarifQuery, sql: *const c_char) -> *mut SarifPlan {
    if db.is_null() || sql.is_null() {
        return ptr::null_mut();
    }
    let query = unsafe { &*db };
    let c_str = unsafe { CStr::from_ptr(sql) };
    prepare_query(query, c_str.to_str().unwrap_or(""))
        .map_or(ptr::null_mut(), |plan| Box::into_raw(Box::new(plan)))
}

#[unsafe(no_mangle)]
#[allow(unused_variables)]
pub extern "C" fn sarif_execute(plan: *mut SarifPlan) -> *mut SarifResult {
    if plan.is_null() {
        return ptr::null_mut();
    }
    let result = SarifResult::default();
    Box::into_raw(Box::new(result))
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_step(result: *mut SarifResult) -> c_long {
    if result.is_null() {
        return 0;
    }
    let res = unsafe { &mut *result };
    c_long::from(res.step())
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_column_text(result: *mut SarifResult, col: c_long) -> *const c_char {
    if result.is_null() {
        return ptr::null();
    }
    let res = unsafe { &*result };
    let idx: usize = col.try_into().unwrap_or(0);
    res.column_text(idx).map_or(ptr::null(), |s| {
        CString::new(s.as_str()).unwrap().into_raw()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_column_int(result: *mut SarifResult, col: c_long) -> c_long {
    if result.is_null() {
        return 0;
    }
    let res = unsafe { &*result };
    let idx: usize = col.try_into().unwrap_or(0);
    res.column_int(idx).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_column_double(result: *mut SarifResult, col: c_long) -> c_double {
    if result.is_null() {
        return 0.0;
    }
    let res = unsafe { &*result };
    let idx: usize = col.try_into().unwrap_or(0);
    res.column_double(idx).unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_finalize(plan: *mut SarifPlan) {
    if plan.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(plan));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_close(db: *mut SarifQuery) {
    if db.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(db));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sarif_result_close(result: *mut SarifResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(result));
    }
}
