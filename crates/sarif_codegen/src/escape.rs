use std::collections::HashMap;

use sarif_syntax::Diagnostic;

use crate::{Function, Inst, Program, ValueId};

#[derive(Clone, Debug)]
struct CalleeInfo {
    return_escapes: bool,
}

pub fn analyze_escapes(program: &Program) -> Vec<Diagnostic> {
    let mut callee_map: HashMap<String, CalleeInfo> = program
        .functions
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                CalleeInfo {
                    return_escapes: false,
                },
            )
        })
        .collect();

    // Fixed-point iteration: compute return_escapes for each function.
    // return_escapes is true when the function's result value is in the
    // escaped set — meaning it references arena memory created inside
    // the function or transitively through callees.
    // Uses a snapshot of the previous iteration for the interprocedural
    // analysis, so changes propagate upward through the call graph.
    loop {
        let mut changed = false;
        let snapshot = callee_map.clone();
        for function in &program.functions {
            let may_escape = function.result.is_some_and(|result| {
                function.effects.iter().any(|e| e == "alloc")
                    && function
                        .return_type
                        .as_deref()
                        .is_some_and(type_can_hold_arena_memory)
                    && value_may_escape(result, function, &snapshot)
            });
            if let Some(info) = callee_map.get_mut(&function.name)
                && info.return_escapes != may_escape
            {
                info.return_escapes = may_escape;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut diagnostics = Vec::new();
    for function in &program.functions {
        for diag in analyze_function(function, &callee_map) {
            diagnostics.push(diag);
        }
    }
    diagnostics
}

fn analyze_function(
    function: &Function,
    callee_map: &HashMap<String, CalleeInfo>,
) -> Vec<Diagnostic> {
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
        callee_map,
    };
    env.analyze(&function.instructions);

    let idx = value.0 as usize;
    idx < env.escaped.len() && env.escaped[idx]
}

struct Env<'a> {
    escaped: &'a mut Vec<bool>,
    esc_locals: &'a mut Vec<bool>,
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
            | Inst::TextIndexContains { dest, .. }
            | Inst::TextIndexGetOrInsert { dest, .. }
            | Inst::TextIndexSet { dest, .. }
            | Inst::ListSortText { dest, .. }
            | Inst::ListSortRecordTextField { dest, .. }
            | Inst::ListSet { dest, .. }
            | Inst::TextBuilderFinish { dest, .. }
            | Inst::Perform { dest, .. } => {
                self.mark(*dest);
            }

            Inst::BytesSlice { dest, bytes, .. } => {
                if self.is_escaped(*bytes) {
                    self.mark(*dest);
                }
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
                    && info.return_escapes
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
                dest,
                body_insts,
                body_result,
                arms,
            } => {
                self.analyze(body_insts);
                if body_result.is_some_and(|r| self.is_escaped(r)) {
                    self.mark(*dest);
                }
                for arm in arms {
                    self.analyze(&arm.body_insts);
                    if arm.body_result.is_some_and(|r| self.is_escaped(r)) {
                        self.mark(*dest);
                    }
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
            | Inst::EnumToI32 { .. }
            | Inst::EnumToText { .. }
            | Inst::Add { .. }
            | Inst::Sub { .. }
            | Inst::Mul { .. }
            | Inst::Div { .. }
            | Inst::Rem { .. }
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
    matches!(ty, "Text" | "TextBuilder" | "TextIndex" | "List" | "Bytes")
        || ty.starts_with('[')
        || ty.contains("Text")
        || ty.contains("List")
        || ty.contains("Bytes")
}

#[cfg(test)]
mod tests {
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    use crate::lower;

    fn lower_source(source: &str) -> crate::MirLowering {
        let lexed = lex(source);
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        lower(&hir.module)
    }

    #[test]
    fn no_alloc_effect_no_diagnostic() {
        let mir = lower_source("fn main() -> Text { arg_text(0) }");
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        assert!(
            diags.is_empty(),
            "no alloc effect should not produce escape diagnostics"
        );
    }

    #[test]
    fn i32_return_no_diagnostic() {
        let mir = lower_source(
            "\
fn main() -> I32 effects [alloc] { 42 }",
        );
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(diags.is_empty(), "I32 cannot hold arena memory");
    }

    #[test]
    fn text_created_inside_triggers_diagnostic() {
        let mir = lower_source(
            "\
fn main() -> Text effects [alloc] { arg_text(0) }",
        );
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            !diags.is_empty(),
            "returning arena text should trigger diagnostic"
        );
        assert!(diags.iter().any(|d| d.code == "escape.analysis.required"));
    }

    #[test]
    fn param_passthrough_no_diagnostic() {
        let mir = lower_source(
            "\
fn identity(s: Text) -> Text effects [alloc] { s }
fn main() -> I32 { 0 }",
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        assert!(
            diags.is_empty(),
            "passing a parameter through should not produce a false positive"
        );
    }

    #[test]
    fn text_through_if_triggers_diagnostic() {
        let mir = lower_source(
            "\
fn choose(c: Bool) -> Text effects [alloc] {
  if c { arg_text(0) } else { arg_text(1) }
}
fn main() -> Text effects [alloc] { choose(true) }",
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        assert!(
            !diags.is_empty(),
            "escaping through if should trigger diagnostic"
        );
    }

    #[test]
    fn type_list_text_is_arena_holding() {
        assert!(
            super::type_can_hold_arena_memory("List[Text]"),
            "List[Text] holds arena memory"
        );
        assert!(
            super::type_can_hold_arena_memory("List[I32]"),
            "List[I32] holds arena memory via List"
        );
        assert!(
            super::type_can_hold_arena_memory("Bytes"),
            "Bytes holds arena memory"
        );
        assert!(
            !super::type_can_hold_arena_memory("I32"),
            "I32 does not hold arena memory"
        );
        assert!(
            !super::type_can_hold_arena_memory("F64"),
            "F64 does not hold arena memory"
        );
        assert!(
            !super::type_can_hold_arena_memory("Bool"),
            "Bool does not hold arena memory"
        );
    }

    #[test]
    fn alloc_text_ret_list_triggers_diagnostic() {
        let mir = lower_source(
            "\
fn take(s: Text) -> Text effects [alloc] { s }
fn main() -> Text effects [alloc] { take(arg_text(0)) }",
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        assert!(
            !diags.is_empty(),
            "returning passed-through arena text should trigger on main"
        );
    }

    #[test]
    fn non_alloc_callee_no_false_positive() {
        let mir = lower_source(
            "\
fn helper() -> I32 { 42 }
fn main() -> I32 effects [alloc] { helper() }",
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        assert!(
            diags.is_empty(),
            "calling non-alloc function should not cause false positive"
        );
    }

    #[test]
    fn alloc_callee_triggers_on_caller() {
        let mir = lower_source(
            "\
fn inner() -> Text effects [alloc] { arg_text(0) }
fn outer() -> Text effects [alloc] { inner() }
fn main() -> Text effects [alloc] { outer() }",
        );
        let diags = super::analyze_escapes(&mir.program);
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        assert!(
            !diags.is_empty(),
            "allocating callee should trigger on caller"
        );
    }

    #[test]
    fn negative_enum_discriminant_does_not_collide_with_explicit() {
        let mir = lower_source(
            "\
enum Color {
    ExplicitZero = 0,
    Negative = -1,
    Next,
}
fn main() -> I32 effects [alloc] { enum_to_i32(Color.Next {}) }",
        );
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        let result = crate::run_main(&mir.program).unwrap();
        // Old bug: Negative = 0 (truncated from -1), colliding with ExplicitZero = 0.
        // Then Next = 1 (auto-increment from 0), so Next would return 1.
        // After fix: Negative auto-increments to 1 (skipping 0), Next = 2.
        assert_eq!(
            result,
            crate::RuntimeValue::Int(2),
            "Next should auto-increment to 2 (ExplicitZero=0, Negative=1, Next=2)"
        );
    }

    #[test]
    fn positive_enum_discriminants_still_work() {
        let mir = lower_source(
            "\
enum Color {
    Red = 10,
    Blue,
    Green = 20,
    Yellow,
}
fn main() -> I32 effects [alloc] { enum_to_i32(Color.Yellow {}) }",
        );
        assert!(
            mir.diagnostics.is_empty(),
            "lowering should succeed: {:#?}",
            mir.diagnostics,
        );
        let result = crate::run_main(&mir.program).unwrap();
        assert_eq!(
            result,
            crate::RuntimeValue::Int(21),
            "Yellow should be 21 (Green=20, auto-increment)"
        );
    }
}
