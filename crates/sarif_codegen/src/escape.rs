use std::collections::HashMap;

use sarif_syntax::Diagnostic;

use crate::{Function, Inst, Program, ValueId};

#[derive(Clone, Debug)]
struct CalleeInfo {
    has_alloc: bool,
    return_type: Option<String>,
}

pub fn analyze_escapes(program: &Program) -> Vec<Diagnostic> {
    let callee_map: HashMap<String, CalleeInfo> = program
        .functions
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                CalleeInfo {
                    has_alloc: f.effects.iter().any(|e| e == "alloc"),
                    return_type: f.return_type.clone(),
                },
            )
        })
        .collect();

    let mut diagnostics = Vec::new();
    for function in &program.functions {
        for diag in analyze_function(function, &callee_map) {
            diagnostics.push(diag);
        }
    }
    diagnostics
}

fn analyze_function(function: &Function, callee_map: &HashMap<String, CalleeInfo>) -> Vec<Diagnostic> {
    let result = match function.result {
        Some(v) => v,
        None => return Vec::new(),
    };

    let has_alloc = function.effects.iter().any(|e| e == "alloc");
    if !has_alloc {
        return Vec::new();
    }

    let return_can_hold = function
        .return_type
        .as_deref()
        .is_some_and(type_can_hold_arena_memory);
    if !return_can_hold {
        return Vec::new();
    }

    if !value_may_escape(result, function, callee_map) {
        return Vec::new();
    }

    vec![Diagnostic::new(
        "escape.analysis.required",
        format!(
            "function `{}` with `alloc` effect returns `{}` that may reference arena memory",
            function.name,
            function.return_type.as_deref().unwrap_or("Unit"),
        ),
        function.span,
        Some(
            "Stage-1 requires all returned values to be escape-safe. \
             If the return value does not reference arena memory, remove the `alloc` effect."
                .to_owned(),
        ),
    )]
}

fn value_may_escape(
    value: ValueId,
    function: &Function,
    callee_map: &HashMap<String, CalleeInfo>,
) -> bool {
    let mut escaped = vec![false; function.value_count as usize];
    let mut esc_locals = vec![false; function.slot_count as usize];
    let mut env = Env {
        escaped: &mut escaped,
        esc_locals: &mut esc_locals,
        function,
        callee_map,
    };
    env.analyze(&function.instructions);

    let idx = value.0 as usize;
    idx < env.escaped.len() && env.escaped[idx]
}

struct Env<'a> {
    escaped: &'a mut Vec<bool>,
    esc_locals: &'a mut Vec<bool>,
    function: &'a Function,
    callee_map: &'a HashMap<String, CalleeInfo>,
}

impl Env<'_> {
    fn mark(&mut self, value: ValueId) {
        let idx = value.0 as usize;
        if idx < self.escaped.len() {
            self.escaped[idx] = true;
        }
    }

    fn is_escaped(&self, value: ValueId) -> bool {
        let idx = value.0 as usize;
        idx < self.escaped.len() && self.escaped[idx]
    }

    fn analyze(&mut self, insts: &[Inst]) {
        for inst in insts {
            self.analyze_inst(inst);
        }
    }

    fn analyze_inst(&mut self, inst: &Inst) {
        match inst {
            Inst::TextBuilderNew { dest }
            | Inst::TextIndexNew { dest }
            | Inst::ListNew { dest, .. }
            | Inst::ListPush { dest, .. }
            | Inst::TextConcat { dest, .. }
            | Inst::TextSlice { dest, .. }
            | Inst::BytesSlice { dest, .. }
            | Inst::TextFromF64Fixed { dest, .. }
            | Inst::ArgText { dest, .. }
            | Inst::StdinText { dest }
            | Inst::StdinBytes { dest }
            | Inst::TextBuilderAppend { dest, .. }
            | Inst::TextBuilderAppendCodepoint { dest, .. }
            | Inst::TextBuilderAppendAscii { dest, .. }
            | Inst::TextBuilderAppendSlice { dest, .. }
            | Inst::TextBuilderAppendI32 { dest, .. }
            | Inst::StdoutWriteBuilder { dest, .. }
            | Inst::TextIndexGet { dest, .. }
            | Inst::TextIndexGetOrInsert { dest, .. }
            | Inst::TextIndexSet { dest, .. }
            | Inst::ListSortText { dest, .. }
            | Inst::ListSortRecordTextField { dest, .. }
            | Inst::ListSet { dest, .. }
            | Inst::TextBuilderFinish { dest, .. }
            | Inst::Perform { dest, .. } => {
                self.mark(*dest);
            }

            Inst::ListGet { dest, list, .. } => {
                if self.is_escaped(*list) {
                    self.mark(*dest);
                }
            }

            Inst::LoadLocal { dest, slot } => {
                let idx = slot.0 as usize;
                if idx < self.esc_locals.len() && self.esc_locals[idx] {
                    self.mark(*dest);
                }
            }

            Inst::StoreLocal { slot, src } => {
                if self.is_escaped(*src) {
                    let idx = slot.0 as usize;
                    if idx < self.esc_locals.len() {
                        self.esc_locals[idx] = true;
                    }
                }
            }

            Inst::MakeRecord { dest, fields, .. } => {
                if fields.iter().any(|(_, v)| self.is_escaped(*v)) {
                    self.mark(*dest);
                }
            }

            Inst::MakeEnum { dest, payload, .. } => {
                if payload.is_some_and(|p| self.is_escaped(p)) {
                    self.mark(*dest);
                }
            }

            Inst::Field { dest, base, .. } => {
                if self.is_escaped(*base) {
                    self.mark(*dest);
                }
            }

            Inst::EnumPayload { dest, value, .. } => {
                if self.is_escaped(*value) {
                    self.mark(*dest);
                }
            }

            Inst::Call { dest, callee, args } => {
                if let Some(info) = self.callee_map.get(callee)
                    && info.has_alloc
                    && info
                        .return_type
                        .as_deref()
                        .is_some_and(type_can_hold_arena_memory)
                {
                    self.mark(*dest);
                }
                if args.iter().any(|a| self.is_escaped(*a)) {
                    self.mark(*dest);
                }
            }

            Inst::If {
                dest,
                condition: _,
                then_insts,
                then_result,
                else_insts,
                else_result,
            } => {
                let before_locals = self.esc_locals.clone();

                self.analyze(then_insts);
                let then_locals = self.esc_locals.clone();

                *self.esc_locals = before_locals;
                self.analyze(else_insts);

                for i in 0..self.esc_locals.len() {
                    if i < then_locals.len() && then_locals[i] {
                        self.esc_locals[i] = true;
                    }
                }

                if then_result.is_some_and(|r| self.is_escaped(r))
                    || else_result.is_some_and(|r| self.is_escaped(r))
                {
                    self.mark(*dest);
                }
            }

            Inst::While {
                dest,
                condition_insts,
                condition: _,
                body_insts,
            } => {
                loop {
                    let old_esc = self.escaped.clone();
                    let old_loc = self.esc_locals.clone();
                    self.analyze(condition_insts);
                    self.analyze(body_insts);
                    if *self.escaped == old_esc && *self.esc_locals == old_loc {
                        break;
                    }
                }
                self.mark(*dest);
            }

            Inst::Repeat {
                dest,
                count: _,
                index_slot: _,
                body_insts,
            } => {
                loop {
                    let old_esc = self.escaped.clone();
                    let old_loc = self.esc_locals.clone();
                    self.analyze(body_insts);
                    if *self.escaped == old_esc && *self.esc_locals == old_loc {
                        break;
                    }
                }
                self.mark(*dest);
            }

            Inst::Handle {
                dest: _,
                body_insts,
                body_result: _,
                arms,
            } => {
                self.analyze(body_insts);
                for arm in arms {
                    self.analyze(&arm.body_insts);
                }
            }

            Inst::Assert { .. }
            | Inst::AllocPush
            | Inst::AllocPop
            | Inst::StdoutWrite { .. }
            | Inst::LoadParam { .. }
            | Inst::ArgCount { .. }
            | Inst::ParseI32 { .. }
            | Inst::ParseI32Range { .. }
            | Inst::ParseF64 { .. }
            | Inst::F64FromI32 { .. }
            | Inst::TextLen { .. }
            | Inst::BytesLen { .. }
            | Inst::TextCmp { .. }
            | Inst::TextEqRange { .. }
            | Inst::TextFindByteRange { .. }
            | Inst::BytesFindByteRange { .. }
            | Inst::TextByte { .. }
            | Inst::BytesByte { .. }
            | Inst::TextLineEnd { .. }
            | Inst::TextNextLine { .. }
            | Inst::TextFieldEnd { .. }
            | Inst::TextNextField { .. }
            | Inst::ListLen { .. }
            | Inst::ConstInt { .. }
            | Inst::ConstF64 { .. }
            | Inst::ConstBool { .. }
            | Inst::ConstText { .. }
            | Inst::EnumTagEq { .. }
            | Inst::Add { .. }
            | Inst::Sub { .. }
            | Inst::Mul { .. }
            | Inst::Div { .. }
            | Inst::BitAnd { .. }
            | Inst::BitOr { .. }
            | Inst::BitXor { .. }
            | Inst::Shl { .. }
            | Inst::Shr { .. }
            | Inst::Sqrt { .. }
            | Inst::And { .. }
            | Inst::Or { .. }
            | Inst::Eq { .. }
            | Inst::Ne { .. }
            | Inst::Lt { .. }
            | Inst::Le { .. }
            | Inst::Gt { .. }
            | Inst::Ge { .. } => {}
        }
    }
}

fn type_can_hold_arena_memory(ty: &str) -> bool {
    matches!(
        ty,
        "Text" | "TextBuilder" | "TextIndex" | "List" | "Bytes"
    ) || ty.starts_with('[')
        || ty.contains("Text")
        || ty.contains("List")
        || ty.contains("Bytes")
}
