#![allow(unsafe_code)]

use std::cell::RefCell;

use crate::{SarifPlan, SarifQuery, SarifResult, open_database, prepare_query};

thread_local! {
    static CURRENT_DB: RefCell<Option<SarifQuery>> = RefCell::new(None);
    static CURRENT_RESULT: RefCell<Option<SarifResult>> = RefCell::new(None);
}

pub struct SarifQueryHandle {
    db: SarifQuery,
}

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

pub struct SarifPlanHandle {
    plan: SarifPlan,
}

impl SarifPlanHandle {
    pub fn execute(&self) -> Result<SarifResultSetHandle, String> {
        let result = SarifResult::default();
        CURRENT_RESULT.with(|cell| {
            *cell.borrow_mut() = Some(result.clone());
        });
        Ok(SarifResultSetHandle { result })
    }
}

pub struct SarifResultSetHandle {
    result: SarifResult,
}

impl SarifResultSetHandle {
    pub fn next(&mut self) -> bool {
        self.result.step()
    }

    pub fn column_count(&self) -> usize {
        self.result.column_count()
    }

    pub fn row_count(&self) -> usize {
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

pub fn get_plan_pretty(plan: &SarifPlan) -> String {
    if let Some(ref optimized) = plan.optimized {
        optimized.pretty()
    } else {
        String::from("no optimized plan")
    }
}

pub fn execute_plan(_plan: &SarifPlan) -> Result<SarifResult, String> {
    Ok(SarifResult::default())
}
