#![allow(unsafe_code)]

use std::cell::RefCell;

use crate::optimizer::OptimizedPlan;
use crate::{SarifPlan, SarifQuery, SarifResult, open_database, prepare_query};

thread_local! {
    static CURRENT_DB: RefCell<Option<SarifQuery>> = const { RefCell::new(None) };
    static CURRENT_RESULT: RefCell<Option<SarifResult>> = const { RefCell::new(None) };
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct SarifQueryHandle {
    db: SarifQuery,
}

#[allow(dead_code)]
impl SarifQueryHandle {
    pub fn open(path: &str) -> Result<Self, String> {
        let db = open_database(path)?;
        CURRENT_DB.with(|cell| {
            *cell.borrow_mut() = Some(db.clone());
        });
        Ok(Self { db })
    }

    pub fn prepare(&self, sql: &str) -> Result<SarifPlanHandle, String> {
        let plan = prepare_query(&self.db, sql)?;
        Ok(SarifPlanHandle { plan })
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct SarifPlanHandle {
    plan: SarifPlan,
}

#[allow(dead_code)]
impl SarifPlanHandle {
    pub fn execute() -> SarifResultSetHandle {
        let result = SarifResult::default();
        CURRENT_RESULT.with(|cell| {
            *cell.borrow_mut() = Some(result.clone());
        });
        SarifResultSetHandle { result }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct SarifResultSetHandle {
    result: SarifResult,
}

#[allow(dead_code)]
impl SarifResultSetHandle {
    #[must_use]
    pub const fn next(&mut self) -> bool {
        self.result.step()
    }

    #[must_use]
    pub const fn column_count(&self) -> usize {
        self.result.column_count()
    }

    #[must_use]
    pub const fn row_count(&self) -> usize {
        self.result.row_count()
    }

    pub fn get_column(&self, name: &str) -> Option<String> {
        self.result
            .columns
            .iter()
            .position(|c| c == name)
            .and_then(|idx| self.result.column_text(idx))
            .cloned()
    }

    pub fn get_column_by_index(&self, idx: usize) -> Option<String> {
        self.result.column_text(idx).cloned()
    }
}

#[allow(dead_code)]
pub fn get_plan_pretty(plan: &SarifPlan) -> String {
    plan.optimized
        .as_ref()
        .map_or_else(|| String::from("no optimized plan"), OptimizedPlan::pretty)
}

#[allow(dead_code)]
pub fn execute_plan(_plan: &SarifPlan) -> SarifResult {
    SarifResult::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizer::OptimizedPlan;

    #[test]
    fn test_get_plan_pretty_with_optimized() {
        let db = SarifQuery::default();
        let optimized = OptimizedPlan::default();
        let plan = SarifPlan {
            query: db,
            optimized: Some(optimized),
        };
        let pretty = get_plan_pretty(&plan);
        assert!(pretty.contains("OPTIMIZED PLAN"));
    }

    #[test]
    fn test_get_plan_pretty_no_optimized() {
        let db = SarifQuery::default();
        let plan = SarifPlan {
            query: db,
            optimized: None,
        };
        assert_eq!(get_plan_pretty(&plan), "no optimized plan");
    }

    #[test]
    fn test_execute_plan_returns_default_result() {
        let db = SarifQuery::default();
        let plan = SarifPlan {
            query: db,
            optimized: None,
        };
        let result = execute_plan(&plan);
        assert_eq!(result.column_count(), 0);
        assert_eq!(result.row_count(), 0);
    }

    #[test]
    fn test_sarif_result_set_handle() {
        let result = SarifResult {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["2".to_string(), "bob".to_string()],
            ],
            current_row: 0,
        };
        let mut handle = SarifResultSetHandle { result };

        assert!(handle.next());
        assert_eq!(handle.column_count(), 2);
        assert_eq!(handle.row_count(), 2);
        assert_eq!(handle.get_column("id"), Some("1".to_string()));
        assert_eq!(handle.get_column_by_index(1), Some("alice".to_string()));

        assert!(handle.next());
        assert_eq!(handle.get_column("id"), Some("2".to_string()));

        assert!(!handle.next());
    }

    #[test]
    fn test_sarif_result_set_handle_empty() {
        let result = SarifResult::default();
        let mut handle = SarifResultSetHandle { result };

        assert!(!handle.next());
        assert_eq!(handle.column_count(), 0);
        assert_eq!(handle.row_count(), 0);
        assert_eq!(handle.get_column("anything"), None);
    }

    #[test]
    fn test_sarif_query_handle_requires_existing_file() {
        let result = SarifQueryHandle::open("/tmp/nonexistent_file_xyz");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
