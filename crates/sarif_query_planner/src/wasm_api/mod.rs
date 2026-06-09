#![allow(unsafe_code)]

use std::cell::RefCell;

use crate::optimizer::OptimizedPlan;
use crate::{SarifPlan, SarifQuery, SarifResult, open_database, prepare_query};

thread_local! {
    static CURRENT_DB: RefCell<Option<SarifQuery>> = const { RefCell::new(None) };
    static CURRENT_RESULT: RefCell<Option<SarifResult>> = const { RefCell::new(None) };
}

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
