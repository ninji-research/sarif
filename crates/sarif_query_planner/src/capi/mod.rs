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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizer::OptimizedPlan;

    fn make_raw_result() -> *mut SarifResult {
        let result = SarifResult {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["2".to_string(), "bob".to_string()],
            ],
            current_row: 0,
        };
        Box::into_raw(Box::new(result))
    }

    fn make_raw_plan() -> *mut SarifPlan {
        let plan = SarifPlan {
            query: SarifQuery::default(),
            optimized: Some(OptimizedPlan::default()),
        };
        Box::into_raw(Box::new(plan))
    }

    #[test]
    fn test_sarif_execute_returns_result() {
        let plan_ptr = make_raw_plan();
        let result_ptr = sarif_execute(plan_ptr);
        assert!(!result_ptr.is_null());
        unsafe {
            let result = &*result_ptr;
            assert_eq!(result.column_count(), 0);
            assert_eq!(result.row_count(), 0);
            drop(Box::from_raw(result_ptr));
        }
        sarif_finalize(plan_ptr);
    }

    #[test]
    fn test_sarif_execute_null_plan() {
        let result_ptr = sarif_execute(ptr::null_mut());
        assert!(result_ptr.is_null());
    }

    #[test]
    fn test_sarif_step_null_result() {
        assert_eq!(sarif_step(ptr::null_mut()), 0);
    }

    #[test]
    fn test_sarif_step_through_result() {
        let ptr = make_raw_result();
        unsafe {
            assert_eq!(sarif_step(ptr), 1);
            assert_eq!(sarif_step(ptr), 1);
            assert_eq!(sarif_step(ptr), 0);
            drop(Box::from_raw(ptr));
        }
    }

    #[test]
    fn test_sarif_column_text() {
        let ptr = make_raw_result();
        unsafe {
            sarif_step(ptr);
            let text_ptr = sarif_column_text(ptr, 0);
            assert!(!text_ptr.is_null());
            let text = CStr::from_ptr(text_ptr);
            assert_eq!(text.to_str().unwrap(), "1");
            let _ = CString::from_raw(text_ptr.cast_mut());

            let text_ptr2 = sarif_column_text(ptr, 99);
            assert!(text_ptr2.is_null());
            drop(Box::from_raw(ptr));
        }
    }

    #[test]
    fn test_sarif_column_text_null_result() {
        let ptr = sarif_column_text(ptr::null_mut(), 0);
        assert!(ptr.is_null());
    }

    #[test]
    fn test_sarif_column_int() {
        let ptr = make_raw_result();
        unsafe {
            sarif_step(ptr);
            assert_eq!(sarif_column_int(ptr, 0), 1);
            assert_eq!(sarif_column_int(ptr, 1), 0);
            assert_eq!(sarif_column_int(ptr, 99), 0);
            drop(Box::from_raw(ptr));
        }
    }

    #[test]
    fn test_sarif_column_int_null_result() {
        assert_eq!(sarif_column_int(ptr::null_mut(), 0), 0);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_sarif_column_double() {
        let ptr = Box::into_raw(Box::new(SarifResult {
            columns: vec!["val".to_string()],
            rows: vec![vec!["3.14".to_string()]],
            current_row: 0,
        }));
        unsafe {
            sarif_step(ptr);
            let val = sarif_column_double(ptr, 0);
            assert!((val - 3.14_f64).abs() < 1e-10);
            assert!(sarif_column_double(ptr, 99).abs() < f64::EPSILON);
            drop(Box::from_raw(ptr));
        }
    }

    #[test]
    fn test_sarif_column_double_null_result() {
        assert!(sarif_column_double(ptr::null_mut(), 0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sarif_finalize_null() {
        sarif_finalize(ptr::null_mut());
    }

    #[test]
    fn test_sarif_close_null() {
        sarif_close(ptr::null_mut());
    }

    #[test]
    fn test_sarif_result_close_null() {
        sarif_result_close(ptr::null_mut());
    }

    #[test]
    fn test_sarif_open_null_path() {
        let ptr = sarif_open(ptr::null_mut());
        assert!(ptr.is_null());
    }

    #[test]
    fn test_sarif_prepare_null_args() {
        assert!(sarif_prepare(ptr::null_mut(), ptr::null_mut()).is_null());

        let query_ptr = Box::into_raw(Box::new(SarifQuery::default()));
        assert!(sarif_prepare(query_ptr, ptr::null_mut()).is_null());
        assert!(
            sarif_prepare(ptr::null_mut(), CString::new("SELECT 1").unwrap().as_ptr()).is_null()
        );
        unsafe {
            drop(Box::from_raw(query_ptr));
        }
    }
}
