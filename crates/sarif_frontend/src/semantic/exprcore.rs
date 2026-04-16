use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::hir::{ConstExpr, Effect, Expr};
use sarif_syntax::{Diagnostic, Span};

use super::{
    ConstSignature, EnumVariantInfo, FunctionSignature, Type, best_match, enum_variant_info,
    expect_type, matching_numeric_type, split_enum_variant_path, suggestion_help,
    support::{
        enum_literal_type_name, field_names_for_type, field_type, type_contains_affine_values,
    },
    types_compatible,
};

#[derive(Clone, Debug)]
pub struct ExprInfo {
    pub ty: super::Type,
    pub calls: Vec<CallSite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    pub callee: String,
}

#[derive(Clone, Debug, Default)]
pub enum ExprContext {
    #[default]
    Body,
    BodyTail,
    Statement,
    ContractRequires,
    ContractEnsures,
}

pub const fn nested_expr_context(context: &ExprContext) -> ExprContext {
    match context {
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement => ExprContext::Body,
        ExprContext::ContractRequires => ExprContext::ContractRequires,
        ExprContext::ContractEnsures => ExprContext::ContractEnsures,
    }
}

pub const fn allows_runtime_builtin_context(context: &ExprContext) -> bool {
    matches!(
        context,
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement
    )
}

pub fn require_runtime_builtin_context(
    code: &'static str,
    builtin: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
    context: &ExprContext,
) {
    if allows_runtime_builtin_context(context) {
        return;
    }
    diagnostics.push(Diagnostic::new(
        code,
        format!("builtin `{builtin}` is only available in executable function bodies"),
        span,
        Some("Use this builtin inside a function body or body-tail expression.".to_owned()),
    ));
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn infer_call_expr(
    expr: &crate::hir::CallExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let mut calls = Vec::new();
    let args = expr
        .args
        .iter()
        .map(|arg| {
            let info = infer_expr(
                arg,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            calls.extend(info.calls.iter().cloned());
            info
        })
        .collect::<Vec<_>>();

    if expr.callee == ARRAY_LEN_BUILTIN && !functions.contains_key(ARRAY_LEN_BUILTIN) {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.len-arity",
                format!(
                    "builtin `{ARRAY_LEN_BUILTIN}` expects 1 argument but got {}",
                    args.len(),
                ),
                expr.span,
                Some("Call `len(xs)` with exactly one array argument.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let arg = &args[0];
        return match &arg.ty {
            Type::Array(_, _) => ExprInfo {
                ty: Type::I32,
                calls,
            },
            Type::Error => ExprInfo {
                ty: Type::Error,
                calls,
            },
            _ => {
                diagnostics.push(Diagnostic::new(
                    "semantic.len-type",
                    format!(
                        "builtin `{ARRAY_LEN_BUILTIN}` expects an array argument, found `{}`",
                        arg.ty.render(),
                    ),
                    expr.span,
                    Some("Pass an internal stage-0 array such as `len(xs)`.".to_owned()),
                ));
                ExprInfo {
                    ty: Type::Error,
                    calls,
                }
            }
        };
    }

    if expr.callee == "text_len" && !functions.contains_key("text_len") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_len-arity",
                format!(
                    "builtin `text_len` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_len(text)` with exactly one Text argument.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let arg = &args[0];
        return match &arg.ty {
            Type::Text => ExprInfo {
                ty: Type::I32,
                calls,
            },
            Type::Error => ExprInfo {
                ty: Type::Error,
                calls,
            },
            _ => {
                diagnostics.push(Diagnostic::new(
                    "semantic.text_len-type",
                    format!(
                        "builtin `text_len` expects a Text argument, found `{}`",
                        arg.ty.render()
                    ),
                    expr.span,
                    Some("Pass a Text argument.".to_owned()),
                ));
                ExprInfo {
                    ty: Type::Error,
                    calls,
                }
            }
        };
    }

    if expr.callee == "bytes_len" && !functions.contains_key("bytes_len") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_len-arity",
                format!(
                    "builtin `bytes_len` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `bytes_len(bytes)` with exactly one Bytes argument.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let arg = &args[0];
        return match &arg.ty {
            Type::Bytes => ExprInfo {
                ty: Type::I32,
                calls,
            },
            Type::Error => ExprInfo {
                ty: Type::Error,
                calls,
            },
            _ => {
                diagnostics.push(Diagnostic::new(
                    "semantic.bytes_len-type",
                    format!(
                        "builtin `bytes_len` expects a Bytes argument, found `{}`",
                        arg.ty.render()
                    ),
                    expr.span,
                    Some("Pass a Bytes argument.".to_owned()),
                ));
                ExprInfo {
                    ty: Type::Error,
                    calls,
                }
            }
        };
    }

    if expr.callee == "text_byte" && !functions.contains_key("text_byte") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_byte-arity",
                format!(
                    "builtin `text_byte` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_byte(text, index)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let first_arg = &args[0];
        let second_arg = &args[1];
        if first_arg.ty != Type::Text && first_arg.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_byte-type",
                format!(
                    "builtin `text_byte` first argument must be Text, found `{}`",
                    first_arg.ty.render()
                ),
                expr.span,
                Some("Pass a Text argument.".to_owned()),
            ));
        }
        if second_arg.ty != Type::I32 && second_arg.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_byte-type",
                format!(
                    "builtin `text_byte` second argument must be I32, found `{}`",
                    second_arg.ty.render()
                ),
                expr.span,
                Some("Pass an I32 index.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "bytes_byte" && !functions.contains_key("bytes_byte") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_byte-arity",
                format!(
                    "builtin `bytes_byte` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `bytes_byte(bytes, index)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let first_arg = &args[0];
        let second_arg = &args[1];
        if first_arg.ty != Type::Bytes && first_arg.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_byte-type",
                format!(
                    "builtin `bytes_byte` first argument must be Bytes, found `{}`",
                    first_arg.ty.render()
                ),
                expr.span,
                Some("Pass a Bytes argument.".to_owned()),
            ));
        }
        if second_arg.ty != Type::I32 && second_arg.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_byte-type",
                format!(
                    "builtin `bytes_byte` second argument must be I32, found `{}`",
                    second_arg.ty.render()
                ),
                expr.span,
                Some("Pass an I32 index.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_concat" && !functions.contains_key("text_concat") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_concat-arity",
                format!(
                    "builtin `text_concat` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_concat(left, right)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let left = &args[0];
        let right = &args[1];
        if left.ty != Type::Text && left.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_concat-type",
                format!(
                    "builtin `text_concat` first argument must be Text, found `{}`",
                    left.ty.render()
                ),
                expr.span,
                Some("Pass a Text argument.".to_owned()),
            ));
        }
        if right.ty != Type::Text && right.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_concat-type",
                format!(
                    "builtin `text_concat` second argument must be Text, found `{}`",
                    right.ty.render()
                ),
                expr.span,
                Some("Pass a Text argument.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Text,
            calls,
        };
    }

    if expr.callee == "text_slice" && !functions.contains_key("text_slice") {
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_slice-arity",
                format!(
                    "builtin `text_slice` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_slice(text, start, end)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let text = &args[0];
        let start = &args[1];
        let end = &args[2];
        if text.ty != Type::Text && text.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_slice-type",
                format!(
                    "builtin `text_slice` first argument must be Text, found `{}`",
                    text.ty.render()
                ),
                expr.span,
                Some("Pass a Text argument.".to_owned()),
            ));
        }
        if start.ty != Type::I32 && start.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_slice-type",
                format!(
                    "builtin `text_slice` second argument must be I32, found `{}`",
                    start.ty.render()
                ),
                expr.span,
                Some("Pass an I32 start offset.".to_owned()),
            ));
        }
        if end.ty != Type::I32 && end.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_slice-type",
                format!(
                    "builtin `text_slice` third argument must be I32, found `{}`",
                    end.ty.render()
                ),
                expr.span,
                Some("Pass an I32 end offset.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Text,
            calls,
        };
    }

    if expr.callee == "bytes_slice" && !functions.contains_key("bytes_slice") {
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_slice-arity",
                format!(
                    "builtin `bytes_slice` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `bytes_slice(bytes, start, end)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let bytes = &args[0];
        let start = &args[1];
        let end = &args[2];
        if bytes.ty != Type::Bytes && bytes.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_slice-type",
                format!(
                    "builtin `bytes_slice` first argument must be Bytes, found `{}`",
                    bytes.ty.render()
                ),
                expr.span,
                Some("Pass a Bytes argument.".to_owned()),
            ));
        }
        if start.ty != Type::I32 && start.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_slice-type",
                format!(
                    "builtin `bytes_slice` second argument must be I32, found `{}`",
                    start.ty.render()
                ),
                expr.span,
                Some("Pass an I32 start offset.".to_owned()),
            ));
        }
        if end.ty != Type::I32 && end.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_slice-type",
                format!(
                    "builtin `bytes_slice` third argument must be I32, found `{}`",
                    end.ty.render()
                ),
                expr.span,
                Some("Pass an I32 end offset.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Bytes,
            calls,
        };
    }

    if expr.callee == "text_builder_new" && !functions.contains_key("text_builder_new") {
        require_runtime_builtin_context(
            "semantic.text_builder_new-runtime-context",
            "text_builder_new",
            expr.span,
            diagnostics,
            context,
        );
        if !caller_effects.contains(&Effect::Alloc) {
            diagnostics.push(Diagnostic::new(
                "semantic.alloc-effect",
                format!("builtin `text_builder_new` requires `alloc` effect in `{fn_name}`"),
                expr.span,
                Some("Add `effect alloc` to the function signature.".to_owned()),
            ));
        }
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_new-arity",
                format!(
                    "builtin `text_builder_new` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_new()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if expr.callee == "text_builder_append" && !functions.contains_key("text_builder_append") {
        require_runtime_builtin_context(
            "semantic.text_builder_append-runtime-context",
            "text_builder_append",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append-arity",
                format!(
                    "builtin `text_builder_append` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_append(builder, text)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append-type",
                format!(
                    "builtin `text_builder_append` first argument must be TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder accumulator.".to_owned()),
            ));
        }
        if args[1].ty != Type::Text && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append-type",
                format!(
                    "builtin `text_builder_append` second argument must be Text, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass a Text value to append.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if expr.callee == "text_builder_append_codepoint"
        && !functions.contains_key("text_builder_append_codepoint")
    {
        require_runtime_builtin_context(
            "semantic.text_builder_append_codepoint-runtime-context",
            "text_builder_append_codepoint",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_codepoint-arity",
                format!(
                    "builtin `text_builder_append_codepoint` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_append_codepoint(builder, codepoint)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_codepoint-type",
                format!(
                    "builtin `text_builder_append_codepoint` first argument must be TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder accumulator.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_codepoint-type",
                format!(
                    "builtin `text_builder_append_codepoint` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass a Unicode scalar value as an I32.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if expr.callee == "text_builder_append_ascii"
        && !functions.contains_key("text_builder_append_ascii")
    {
        require_runtime_builtin_context(
            "semantic.text_builder_append_ascii-runtime-context",
            "text_builder_append_ascii",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_ascii-arity",
                format!(
                    "builtin `text_builder_append_ascii` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_append_ascii(builder, byte)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_ascii-type",
                format!(
                    "builtin `text_builder_append_ascii` first argument must be TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder accumulator.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_ascii-type",
                format!(
                    "builtin `text_builder_append_ascii` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass an ASCII byte value as an I32.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if expr.callee == "text_builder_append_slice"
        && !functions.contains_key("text_builder_append_slice")
    {
        require_runtime_builtin_context(
            "semantic.text_builder_append_slice-runtime-context",
            "text_builder_append_slice",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 4 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_slice-arity",
                format!(
                    "builtin `text_builder_append_slice` expects 4 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_append_slice(builder, text, start, end)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_slice-type",
                format!(
                    "builtin `text_builder_append_slice` first argument must be TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder accumulator.".to_owned()),
            ));
        }
        if args[1].ty != Type::Text && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_slice-type",
                format!(
                    "builtin `text_builder_append_slice` second argument must be Text, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass a Text source value.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_slice-type",
                format!(
                    "builtin `text_builder_append_slice` third argument must be I32, found `{}`",
                    args[2].ty.render(),
                ),
                expr.span,
                Some("Pass an I32 start index.".to_owned()),
            ));
        }
        if args[3].ty != Type::I32 && args[3].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_slice-type",
                format!(
                    "builtin `text_builder_append_slice` fourth argument must be I32, found `{}`",
                    args[3].ty.render(),
                ),
                expr.span,
                Some("Pass an I32 end index.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if expr.callee == "text_builder_append_i32"
        && !functions.contains_key("text_builder_append_i32")
    {
        require_runtime_builtin_context(
            "semantic.text_builder_append_i32-runtime-context",
            "text_builder_append_i32",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_i32-arity",
                format!(
                    "builtin `text_builder_append_i32` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_append_i32(builder, value)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_i32-type",
                format!(
                    "builtin `text_builder_append_i32` first argument must be TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder accumulator.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_append_i32-type",
                format!(
                    "builtin `text_builder_append_i32` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass an integer value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if expr.callee == "text_builder_finish" && !functions.contains_key("text_builder_finish") {
        require_runtime_builtin_context(
            "semantic.text_builder_finish-runtime-context",
            "text_builder_finish",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_finish-arity",
                format!(
                    "builtin `text_builder_finish` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_builder_finish(builder)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_builder_finish-type",
                format!(
                    "builtin `text_builder_finish` expects TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Text,
            calls,
        };
    }

    if expr.callee == "list_new" && !functions.contains_key("list_new") {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_new",
            expr.span,
            diagnostics,
            context,
        );
        if !caller_effects.contains(&Effect::Alloc) {
            diagnostics.push(Diagnostic::new(
                "semantic.alloc-effect",
                format!("builtin `list_new` requires `alloc` effect in `{fn_name}`"),
                expr.span,
                Some("Add `effect alloc` to the function signature.".to_owned()),
            ));
        }
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_new-arity",
                format!(
                    "builtin `list_new` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_new(len, fill)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::I32 && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_new-type",
                format!(
                    "builtin `list_new` first argument must be I32, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass an integer length.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::List(Box::new(args[1].ty.clone())),
            calls,
        };
    }

    if expr.callee == "list_len" && !functions.contains_key("list_len") {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_len",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_len-arity",
                format!(
                    "builtin `list_len` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_len(vec)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if !matches!(args[0].ty, Type::List(_)) && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_len-type",
                format!(
                    "builtin `list_len` expects List, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a List value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "list_get" && !functions.contains_key("list_get") {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_get",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_get-arity",
                format!(
                    "builtin `list_get` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_get(vec, index)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let element_ty = if let Type::List(element) = &args[0].ty {
            (**element).clone()
        } else {
            if args[0].ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.list_get-type",
                    format!(
                        "builtin `list_get` first argument must be List, found `{}`",
                        args[0].ty.render(),
                    ),
                    expr.span,
                    Some("Pass a List value.".to_owned()),
                ));
            }
            Type::Error
        };
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_get-type",
                format!(
                    "builtin `list_get` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass an integer index.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: element_ty,
            calls,
        };
    }

    if expr.callee == "list_set" && !functions.contains_key("list_set") {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_set",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_set-arity",
                format!(
                    "builtin `list_set` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_set(vec, index, value)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let element_ty = if let Type::List(element) = &args[0].ty {
            Some((**element).clone())
        } else {
            if args[0].ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.list_set-type",
                    format!(
                        "builtin `list_set` first argument must be List, found `{}`",
                        args[0].ty.render(),
                    ),
                    expr.span,
                    Some("Pass a List value.".to_owned()),
                ));
            }
            None
        };
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_set-type",
                format!(
                    "builtin `list_set` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass an integer index.".to_owned()),
            ));
        }
        if let Some(expected) = element_ty
            && args[2].ty != expected
            && args[2].ty != Type::Error
        {
            diagnostics.push(Diagnostic::new(
                "semantic.list_set-type",
                format!(
                    "builtin `list_set` third argument must be {}, found `{}`",
                    expected.render(),
                    args[2].ty.render(),
                ),
                expr.span,
                Some(format!("Pass a {} element value.", expected.render())),
            ));
        }
        return ExprInfo {
            ty: args[0].ty.clone(),
            calls,
        };
    }

    if expr.callee == "list_push" && !functions.contains_key("list_push") {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_push",
            expr.span,
            diagnostics,
            context,
        );
        if !caller_effects.contains(&Effect::Alloc) {
            diagnostics.push(Diagnostic::new(
                "semantic.alloc-effect",
                format!("builtin `list_push` requires `alloc` effect in `{fn_name}`"),
                expr.span,
                Some("Add `effect alloc` to the function signature.".to_owned()),
            ));
        }
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_push-arity",
                format!(
                    "builtin `list_push` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_push(vec, len, value)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let element_ty = if let Type::List(element) = &args[0].ty {
            Some((**element).clone())
        } else {
            if args[0].ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.list_push-type",
                    format!(
                        "builtin `list_push` first argument must be List, found `{}`",
                        args[0].ty.render(),
                    ),
                    expr.span,
                    Some("Pass a List value.".to_owned()),
                ));
            }
            None
        };
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_push-type",
                format!(
                    "builtin `list_push` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass the used length as an integer.".to_owned()),
            ));
        }
        if let Some(expected) = element_ty
            && args[2].ty != expected
            && args[2].ty != Type::Error
        {
            diagnostics.push(Diagnostic::new(
                "semantic.list_push-type",
                format!(
                    "builtin `list_push` third argument must be {}, found `{}`",
                    expected.render(),
                    args[2].ty.render(),
                ),
                expr.span,
                Some(format!("Pass a {} element value.", expected.render())),
            ));
        }
        return ExprInfo {
            ty: args[0].ty.clone(),
            calls,
        };
    }

    if expr.callee == "list_sort_text" && !functions.contains_key("list_sort_text") {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_sort_text",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_sort_text-arity",
                format!(
                    "builtin `list_sort_text` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_sort_text(vec, len)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        match &args[0].ty {
            Type::List(element) if **element == Type::Text => {}
            Type::List(_) => diagnostics.push(Diagnostic::new(
                "semantic.list_sort_text-type",
                format!(
                    "builtin `list_sort_text` first argument must be List[Text], found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a List[Text] value.".to_owned()),
            )),
            Type::Error => {}
            _ => diagnostics.push(Diagnostic::new(
                "semantic.list_sort_text-type",
                format!(
                    "builtin `list_sort_text` first argument must be List[Text], found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a List[Text] value.".to_owned()),
            )),
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_sort_text-type",
                format!(
                    "builtin `list_sort_text` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass the used length as an integer.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: args[0].ty.clone(),
            calls,
        };
    }

    if expr.callee == "list_sort_by_text_field"
        && !functions.contains_key("list_sort_by_text_field")
    {
        require_runtime_builtin_context(
            "semantic.list-runtime-context",
            "list_sort_by_text_field",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.list_sort_by_text_field-arity",
                format!(
                    "builtin `list_sort_by_text_field` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `list_sort_by_text_field(vec, len, \"field\")`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        let mut list_ok = false;
        match &args[0].ty {
            Type::List(element) => {
                if let Type::Named(name) = &**element {
                    list_ok = true;
                    if let Some(fields) = struct_layouts.get(name) {
                        match &expr.args[2] {
                            Expr::String(field_name) => {
                                if let Some((_, field_ty)) =
                                    fields.iter().find(|(field, _)| field == &field_name.value)
                                {
                                    if *field_ty != Type::Text {
                                        diagnostics.push(Diagnostic::new(
                                            "semantic.list_sort_by_text_field-type",
                                            format!(
                                                "builtin `list_sort_by_text_field` field `{name}.{}`
must have type `Text`, found `{}`",
                                                field_name.value,
                                                field_ty.render(),
                                            ),
                                            expr.span,
                                            Some("Sort by a Text field.".to_owned()),
                                        ));
                                    }
                                } else {
                                    diagnostics.push(Diagnostic::new(
                                        "semantic.list_sort_by_text_field-field",
                                        format!(
                                            "record `{name}` has no field `{}`",
                                            field_name.value
                                        ),
                                        expr.span,
                                        Some("Use one of the declared Text field names.".to_owned()),
                                    ));
                                }
                            }
                            _ => diagnostics.push(Diagnostic::new(
                                "semantic.list_sort_by_text_field-field",
                                "builtin `list_sort_by_text_field` requires a string literal field name"
                                    .to_owned(),
                                expr.span,
                                Some("Pass a field name like `\"id\"`.".to_owned()),
                            )),
                        }
                    }
                } else if args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.list_sort_by_text_field-type",
                        format!(
                            "builtin `list_sort_by_text_field` first argument must be List[record], found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a list of records with a Text field.".to_owned()),
                    ));
                }
            }
            Type::Error => {}
            _ => diagnostics.push(Diagnostic::new(
                "semantic.list_sort_by_text_field-type",
                format!(
                    "builtin `list_sort_by_text_field` first argument must be List[record], found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a list of records with a Text field.".to_owned()),
            )),
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_sort_by_text_field-type",
                format!(
                    "builtin `list_sort_by_text_field` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass the used length as an integer.".to_owned()),
            ));
        }
        if args[2].ty != Type::Text && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.list_sort_by_text_field-type",
                format!(
                    "builtin `list_sort_by_text_field` third argument must be Text, found `{}`",
                    args[2].ty.render(),
                ),
                expr.span,
                Some("Pass a Text field name.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: if list_ok {
                args[0].ty.clone()
            } else {
                Type::Error
            },
            calls,
        };
    }

    if expr.callee == "text_index_new" && !functions.contains_key("text_index_new") {
        require_runtime_builtin_context(
            "semantic.text_index-runtime-context",
            "text_index_new",
            expr.span,
            diagnostics,
            context,
        );
        if !caller_effects.contains(&Effect::Alloc) {
            diagnostics.push(Diagnostic::new(
                "semantic.alloc-effect",
                format!("builtin `text_index_new` requires `alloc` effect in `{fn_name}`"),
                expr.span,
                Some("Add `effect alloc` to the function signature.".to_owned()),
            ));
        }
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_new-arity",
                format!(
                    "builtin `text_index_new` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_index_new()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::TextIndex,
            calls,
        };
    }

    if expr.callee == "text_index_get" && !functions.contains_key("text_index_get") {
        require_runtime_builtin_context(
            "semantic.text_index-runtime-context",
            "text_index_get",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_get-arity",
                format!(
                    "builtin `text_index_get` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_index_get(index, key)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextIndex && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_get-type",
                format!(
                    "builtin `text_index_get` first argument must be TextIndex, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextIndex handle.".to_owned()),
            ));
        }
        if args[1].ty != Type::Text && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_get-type",
                format!(
                    "builtin `text_index_get` second argument must be Text, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass a Text lookup key.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_index_set" && !functions.contains_key("text_index_set") {
        require_runtime_builtin_context(
            "semantic.text_index-runtime-context",
            "text_index_set",
            expr.span,
            diagnostics,
            context,
        );
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_set-arity",
                format!(
                    "builtin `text_index_set` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_index_set(index, key, value)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextIndex && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_set-type",
                format!(
                    "builtin `text_index_set` first argument must be TextIndex, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextIndex handle.".to_owned()),
            ));
        }
        if args[1].ty != Type::Text && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_set-type",
                format!(
                    "builtin `text_index_set` second argument must be Text, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass a Text lookup key.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_index_set-type",
                format!(
                    "builtin `text_index_set` third argument must be I32, found `{}`",
                    args[2].ty.render(),
                ),
                expr.span,
                Some("Pass an I32 slot value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextIndex,
            calls,
        };
    }

    if expr.callee == "f64_from_i32" && !functions.contains_key("f64_from_i32") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.f64_from_i32-arity",
                format!(
                    "builtin `f64_from_i32` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `f64_from_i32(value)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::I32 && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.f64_from_i32-type",
                format!(
                    "builtin `f64_from_i32` expects I32, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass an integer value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::F64,
            calls,
        };
    }

    if expr.callee == "text_from_f64_fixed" && !functions.contains_key("text_from_f64_fixed") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_from_f64_fixed-arity",
                format!(
                    "builtin `text_from_f64_fixed` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_from_f64_fixed(value, digits)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::F64 && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_from_f64_fixed-type",
                format!(
                    "builtin `text_from_f64_fixed` first argument must be F64, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a float value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_from_f64_fixed-type",
                format!(
                    "builtin `text_from_f64_fixed` second argument must be I32, found `{}`",
                    args[1].ty.render(),
                ),
                expr.span,
                Some("Pass an integer digit count.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Text,
            calls,
        };
    }

    if expr.callee == "sqrt" && !functions.contains_key("sqrt") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.sqrt-arity",
                format!("builtin `sqrt` expects 1 argument but got {}", args.len()),
                expr.span,
                Some("Call `sqrt(value)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::F64 && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.sqrt-type",
                format!(
                    "builtin `sqrt` expects F64, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a float value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::F64,
            calls,
        };
    }

    if expr.callee == "parse_i32" && !functions.contains_key("parse_i32") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.parse_i32-arity",
                format!(
                    "builtin `parse_i32` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `parse_i32(text)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.parse_i32-type",
                format!(
                    "builtin `parse_i32` expects Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "parse_i32_range" && !functions.contains_key("parse_i32_range") {
        if args.len() != 3 {
            diagnostics.push(Diagnostic::new(
                "semantic.parse_i32_range-arity",
                format!(
                    "builtin `parse_i32_range` expects 3 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `parse_i32_range(text, start, end)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.parse_i32_range-type",
                format!(
                    "builtin `parse_i32_range` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.parse_i32_range-type",
                format!(
                    "builtin `parse_i32_range` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.parse_i32_range-type",
                format!(
                    "builtin `parse_i32_range` third argument must be I32, found `{}`",
                    args[2].ty.render()
                ),
                expr.span,
                Some("Pass an integer end offset.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_eq_range" && !functions.contains_key("text_eq_range") {
        if args.len() != 4 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_eq_range-arity",
                format!(
                    "builtin `text_eq_range` expects 4 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_eq_range(text, start, end, expected)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_eq_range-type",
                format!(
                    "builtin `text_eq_range` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_eq_range-type",
                format!(
                    "builtin `text_eq_range` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_eq_range-type",
                format!(
                    "builtin `text_eq_range` third argument must be I32, found `{}`",
                    args[2].ty.render()
                ),
                expr.span,
                Some("Pass an integer end offset.".to_owned()),
            ));
        }
        if args[3].ty != Type::Text && args[3].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_eq_range-type",
                format!(
                    "builtin `text_eq_range` fourth argument must be Text, found `{}`",
                    args[3].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Bool,
            calls,
        };
    }

    if expr.callee == "text_cmp" && !functions.contains_key("text_cmp") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_cmp-arity",
                format!(
                    "builtin `text_cmp` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_cmp(left, right)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_cmp-type",
                format!(
                    "builtin `text_cmp` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::Text && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_cmp-type",
                format!(
                    "builtin `text_cmp` second argument must be Text, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_find_byte_range" && !functions.contains_key("text_find_byte_range") {
        if args.len() != 4 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_find_byte_range-arity",
                format!(
                    "builtin `text_find_byte_range` expects 4 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_find_byte_range(text, start, end, byte)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_find_byte_range-type",
                format!(
                    "builtin `text_find_byte_range` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_find_byte_range-type",
                format!(
                    "builtin `text_find_byte_range` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_find_byte_range-type",
                format!(
                    "builtin `text_find_byte_range` third argument must be I32, found `{}`",
                    args[2].ty.render()
                ),
                expr.span,
                Some("Pass an integer end offset.".to_owned()),
            ));
        }
        if args[3].ty != Type::I32 && args[3].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_find_byte_range-type",
                format!(
                    "builtin `text_find_byte_range` fourth argument must be I32, found `{}`",
                    args[3].ty.render()
                ),
                expr.span,
                Some("Pass an integer byte value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "bytes_find_byte_range" && !functions.contains_key("bytes_find_byte_range") {
        if args.len() != 4 {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_find_byte_range-arity",
                format!(
                    "builtin `bytes_find_byte_range` expects 4 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `bytes_find_byte_range(bytes, start, end, byte)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Bytes && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_find_byte_range-type",
                format!(
                    "builtin `bytes_find_byte_range` first argument must be Bytes, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Bytes value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_find_byte_range-type",
                format!(
                    "builtin `bytes_find_byte_range` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_find_byte_range-type",
                format!(
                    "builtin `bytes_find_byte_range` third argument must be I32, found `{}`",
                    args[2].ty.render()
                ),
                expr.span,
                Some("Pass an integer end offset.".to_owned()),
            ));
        }
        if args[3].ty != Type::I32 && args[3].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.bytes_find_byte_range-type",
                format!(
                    "builtin `bytes_find_byte_range` fourth argument must be I32, found `{}`",
                    args[3].ty.render()
                ),
                expr.span,
                Some("Pass an integer byte value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_line_end" && !functions.contains_key("text_line_end") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_line_end-arity",
                format!(
                    "builtin `text_line_end` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_line_end(text, start)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_line_end-type",
                format!(
                    "builtin `text_line_end` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_line_end-type",
                format!(
                    "builtin `text_line_end` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_next_line" && !functions.contains_key("text_next_line") {
        if args.len() != 2 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_line-arity",
                format!(
                    "builtin `text_next_line` expects 2 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_next_line(text, start)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_line-type",
                format!(
                    "builtin `text_next_line` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_line-type",
                format!(
                    "builtin `text_next_line` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_field_end" && !functions.contains_key("text_field_end") {
        if args.len() != 4 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_field_end-arity",
                format!(
                    "builtin `text_field_end` expects 4 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_field_end(text, start, end, byte)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_field_end-type",
                format!(
                    "builtin `text_field_end` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_field_end-type",
                format!(
                    "builtin `text_field_end` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_field_end-type",
                format!(
                    "builtin `text_field_end` third argument must be I32, found `{}`",
                    args[2].ty.render()
                ),
                expr.span,
                Some("Pass an integer end offset.".to_owned()),
            ));
        }
        if args[3].ty != Type::I32 && args[3].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_field_end-type",
                format!(
                    "builtin `text_field_end` fourth argument must be I32, found `{}`",
                    args[3].ty.render()
                ),
                expr.span,
                Some("Pass an integer delimiter byte.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "text_next_field" && !functions.contains_key("text_next_field") {
        if args.len() != 4 {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_field-arity",
                format!(
                    "builtin `text_next_field` expects 4 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `text_next_field(text, start, end, byte)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_field-type",
                format!(
                    "builtin `text_next_field` first argument must be Text, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        if args[1].ty != Type::I32 && args[1].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_field-type",
                format!(
                    "builtin `text_next_field` second argument must be I32, found `{}`",
                    args[1].ty.render()
                ),
                expr.span,
                Some("Pass an integer start offset.".to_owned()),
            ));
        }
        if args[2].ty != Type::I32 && args[2].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_field-type",
                format!(
                    "builtin `text_next_field` third argument must be I32, found `{}`",
                    args[2].ty.render()
                ),
                expr.span,
                Some("Pass an integer end offset.".to_owned()),
            ));
        }
        if args[3].ty != Type::I32 && args[3].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.text_next_field-type",
                format!(
                    "builtin `text_next_field` fourth argument must be I32, found `{}`",
                    args[3].ty.render()
                ),
                expr.span,
                Some("Pass an integer delimiter byte.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "arg_count" && !functions.contains_key("arg_count") {
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.arg_count-arity",
                format!(
                    "builtin `arg_count` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `arg_count()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::I32,
            calls,
        };
    }

    if expr.callee == "alloc_push" && !functions.contains_key("alloc_push") {
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.alloc_push-arity",
                format!(
                    "builtin `alloc_push` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `alloc_push()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::Unit,
            calls,
        };
    }

    if expr.callee == "alloc_pop" && !functions.contains_key("alloc_pop") {
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.alloc_pop-arity",
                format!(
                    "builtin `alloc_pop` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `alloc_pop()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::Unit,
            calls,
        };
    }

    if expr.callee == "arg_text" && !functions.contains_key("arg_text") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.arg_text-arity",
                format!(
                    "builtin `arg_text` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `arg_text(index)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::I32 && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.arg_text-type",
                format!(
                    "builtin `arg_text` expects I32, found `{}`",
                    args[0].ty.render()
                ),
                expr.span,
                Some("Pass an integer index.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Text,
            calls,
        };
    }

    if expr.callee == "stdin_text" && !functions.contains_key("stdin_text") {
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.stdin_text-arity",
                format!(
                    "builtin `stdin_text` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `stdin_text()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::Text,
            calls,
        };
    }

    if expr.callee == "stdin_bytes" && !functions.contains_key("stdin_bytes") {
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.stdin_bytes-arity",
                format!(
                    "builtin `stdin_bytes` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `stdin_bytes()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::Bytes,
            calls,
        };
    }

    if expr.callee == "stdin_bytes" && !functions.contains_key("stdin_bytes") {
        if !args.is_empty() {
            diagnostics.push(Diagnostic::new(
                "semantic.stdin_bytes-arity",
                format!(
                    "builtin `stdin_bytes` expects 0 arguments but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `stdin_bytes()` with no arguments.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::Bytes,
            calls,
        };
    }

    if expr.callee == "stdout_write" && !functions.contains_key("stdout_write") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.stdout_write-arity",
                format!(
                    "builtin `stdout_write` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `stdout_write(text)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::Text && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.stdout_write-type",
                format!(
                    "builtin `stdout_write` expects Text, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a Text value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::Unit,
            calls,
        };
    }

    if expr.callee == "stdout_write_builder" && !functions.contains_key("stdout_write_builder") {
        if args.len() != 1 {
            diagnostics.push(Diagnostic::new(
                "semantic.stdout_write_builder-arity",
                format!(
                    "builtin `stdout_write_builder` expects 1 argument but got {}",
                    args.len()
                ),
                expr.span,
                Some("Call `stdout_write_builder(builder)`.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.stdout_write_builder-type",
                format!(
                    "builtin `stdout_write_builder` expects TextBuilder, found `{}`",
                    args[0].ty.render(),
                ),
                expr.span,
                Some("Pass a TextBuilder value.".to_owned()),
            ));
        }
        return ExprInfo {
            ty: Type::TextBuilder,
            calls,
        };
    }

    if let Some((enum_name, variant)) = enum_variant_info(&expr.callee, enum_variants) {
        let expected_arity = usize::from(variant.payload.is_some());
        if args.len() != expected_arity {
            diagnostics.push(Diagnostic::new(
                "semantic.enum-constructor-arity",
                format!(
                    "enum constructor `{}` expects {} argument{} but got {}",
                    expr.callee,
                    expected_arity,
                    if expected_arity == 1 { "" } else { "s" },
                    args.len(),
                ),
                expr.span,
                Some("Pass exactly one payload argument for payload variants, or no arguments for plain variants.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        if let (Some(expected), Some(actual)) = (&variant.payload, args.first())
            && actual.ty != *expected
            && actual.ty != Type::Error
        {
            diagnostics.push(Diagnostic::new(
                "semantic.enum-constructor-type",
                format!(
                    "enum constructor `{}` expects `{}`, found `{}`",
                    expr.callee,
                    expected.render(),
                    actual.ty.render(),
                ),
                expr.span,
                Some("Pass a payload value of the declared variant type.".to_owned()),
            ));
            return ExprInfo {
                ty: Type::Error,
                calls,
            };
        }
        return ExprInfo {
            ty: Type::Named(enum_name.to_owned()),
            calls,
        };
    }

    let Some(callee) = functions.get(&expr.callee) else {
        let suggestion = best_match(&expr.callee, functions.keys().map(String::as_str));
        diagnostics.push(Diagnostic::new(
            "semantic.unknown-call",
            format!("call to unknown function `{}`", expr.callee),
            expr.span,
            Some(suggestion_help(suggestion, || {
                "Declare the callee before using it.".to_owned()
            })),
        ));
        return ExprInfo {
            ty: Type::Error,
            calls,
        };
    };

    if callee.params.len() != args.len() {
        diagnostics.push(Diagnostic::new(
            "semantic.call-arity",
            format!(
                "call to `{}` supplies {} arguments but {} were declared",
                expr.callee,
                args.len(),
                callee.params.len(),
            ),
            expr.span,
            Some("Match the argument list to the function signature.".to_owned()),
        ));
    }

    for ((_, expected, param_span), actual) in callee.params.iter().zip(args.iter()) {
        if !types_compatible(expected, &actual.ty) && actual.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.call-type",
                format!(
                    "argument type mismatch: expected `{}`, found `{}`",
                    expected.render(),
                    actual.ty.render(),
                ),
                *param_span,
                Some("Pass an argument of the declared parameter type.".to_owned()),
            ));
        }
    }

    if !matches!(
        context,
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement
    ) && !callee.effects.is_empty()
    {
        diagnostics.push(Diagnostic::new(
            "semantic.contract-effect",
            format!(
                "contract expression calls `{}` which declares effects [{}]",
                expr.callee,
                callee
                    .effects
                    .iter()
                    .map(Effect::keyword)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            expr.span,
            Some("Contracts must remain effect-free in stage-0.".to_owned()),
        ));
    }

    for effect in &callee.effects {
        if matches!(context, ExprContext::Body | ExprContext::BodyTail)
            && !caller_effects.contains(effect)
        {
            diagnostics.push(Diagnostic::new(
                "semantic.missing-effect",
                format!(
                    "function `{fn_name}` calls `{}` but does not declare the `{}` effect",
                    expr.callee,
                    effect.keyword(),
                ),
                expr.span,
                Some("Add the callee's effect to the caller or remove the call.".to_owned()),
            ));
        }
    }

    calls.push(CallSite {
        callee: expr.callee.clone(),
    });
    ExprInfo {
        ty: callee.return_type.clone(),
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_binary_expr(
    expr: &crate::hir::BinaryExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let left = infer_expr(
        &expr.left,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    let right = infer_expr(
        &expr.right,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    let mut calls = left.calls;
    calls.extend(right.calls);
    let ty = match expr.op {
        crate::hir::BinaryOp::And | crate::hir::BinaryOp::Or => {
            expect_type(
                diagnostics,
                &expr.left,
                &left.ty,
                &Type::Bool,
                expr.op.symbol(),
                "left",
                "Use boolean operands with `and` and `or`.",
            );
            expect_type(
                diagnostics,
                &expr.right,
                &right.ty,
                &Type::Bool,
                expr.op.symbol(),
                "right",
                "Use boolean operands with `and` and `or`.",
            );
            Type::Bool
        }
        crate::hir::BinaryOp::Add
        | crate::hir::BinaryOp::Sub
        | crate::hir::BinaryOp::Mul
        | crate::hir::BinaryOp::Div => {
            if expr.op == crate::hir::BinaryOp::Add
                && left.ty == Type::TextBuilder
                && matches!(right.ty, Type::Text | Type::I32)
            {
                Type::TextBuilder
            } else if let Some(ty) = matching_numeric_type(&left.ty, &right.ty) {
                ty
            } else {
                if left.ty != Type::Error && right.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.binary-type",
                        format!(
                            "operands of `{}` must both be `I32`, both be `F64`, or append to `TextBuilder`, found `{}` and `{}`",
                            expr.op.symbol(),
                            left.ty.render(),
                            right.ty.render(),
                        ),
                        expr.span,
                        Some("Use matching numeric operand types for arithmetic.".to_owned()),
                    ));
                }
                Type::Error
            }
        }
        crate::hir::BinaryOp::BitAnd
        | crate::hir::BinaryOp::BitOr
        | crate::hir::BinaryOp::BitXor
        | crate::hir::BinaryOp::Shl
        | crate::hir::BinaryOp::Shr => {
            expect_type(
                diagnostics,
                &expr.left,
                &left.ty,
                &Type::I32,
                expr.op.symbol(),
                "left",
                "Use `I32` operands with integer bitwise operators.",
            );
            expect_type(
                diagnostics,
                &expr.right,
                &right.ty,
                &Type::I32,
                expr.op.symbol(),
                "right",
                "Use `I32` operands with integer bitwise operators.",
            );
            Type::I32
        }
        crate::hir::BinaryOp::Lt
        | crate::hir::BinaryOp::Le
        | crate::hir::BinaryOp::Gt
        | crate::hir::BinaryOp::Ge => {
            if matching_numeric_type(&left.ty, &right.ty).is_none()
                && left.ty != Type::Error
                && right.ty != Type::Error
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.binary-type",
                    format!(
                        "operands of `{}` must both be `I32` or both be `F64`, found `{}` and `{}`",
                        expr.op.symbol(),
                        left.ty.render(),
                        right.ty.render(),
                    ),
                    expr.span,
                    Some("Use matching numeric operand types for comparisons.".to_owned()),
                ));
            }
            Type::Bool
        }
        crate::hir::BinaryOp::Eq | crate::hir::BinaryOp::Ne => {
            if left.ty != Type::Error && right.ty != Type::Error && left.ty != right.ty {
                diagnostics.push(Diagnostic::new(
                    "semantic.binary-type",
                    format!(
                        "operands of `{}` must have the same type, found `{}` and `{}`",
                        expr.op.symbol(),
                        left.ty.render(),
                        right.ty.render(),
                    ),
                    expr.span,
                    Some("Compare values of the same type.".to_owned()),
                ));
            }
            Type::Bool
        }
    };
    ExprInfo {
        ty: if left.ty == Type::Error || right.ty == Type::Error {
            Type::Error
        } else {
            ty
        },
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_unary_expr(
    expr: &crate::hir::UnaryExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let inner = infer_expr(
        &expr.inner,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    let ty = match expr.op {
        crate::hir::UnaryOp::Not => {
            expect_type(
                diagnostics,
                &expr.inner,
                &inner.ty,
                &Type::Bool,
                expr.op.symbol(),
                "operand",
                "Use a boolean operand with `not`.",
            );
            Type::Bool
        }
    };
    ExprInfo {
        ty: if inner.ty == Type::Error {
            Type::Error
        } else {
            ty
        },
        calls: inner.calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_field_expr(
    expr: &crate::hir::FieldExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    if let Some(enum_name) = enum_literal_type_name(
        &expr.base,
        &expr.field,
        enum_variants,
        diagnostics,
        expr.span,
    ) {
        return ExprInfo {
            ty: Type::Named(enum_name),
            calls: Vec::new(),
        };
    }
    let nested_context = nested_expr_context(context);
    let base = infer_expr(
        &expr.base,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    if expr.field == "len" {
        match &base.ty {
            Type::Array(_, _) | Type::List(_) | Type::Text => {
                return ExprInfo {
                    ty: Type::I32,
                    calls: base.calls,
                };
            }
            Type::Error => {
                return ExprInfo {
                    ty: Type::Error,
                    calls: base.calls,
                };
            }
            _ => {}
        }
    }
    let ty = if let Some(ty) = field_type(&base.ty, &expr.field, struct_layouts) {
        ty
    } else {
        let suggestion =
            field_names_for_type(&base.ty, struct_layouts).and_then(|fields: Vec<String>| {
                best_match(&expr.field, fields.iter().map(String::as_str))
            });
        diagnostics.push(Diagnostic::new(
            "semantic.field-access",
            format!(
                "cannot read field `{}` from value of type `{}`",
                expr.field,
                base.ty.render(),
            ),
            expr.span,
            Some(suggestion_help(suggestion, || {
                "Use a declared struct value and one of its field names.".to_owned()
            })),
        ));
        Type::Error
    };
    ExprInfo {
        ty: if base.ty == Type::Error || ty == Type::Error {
            Type::Error
        } else {
            ty
        },
        calls: base.calls,
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn infer_record_expr(
    expr: &crate::hir::RecordExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let mut calls = Vec::new();
    let field_infos = expr
        .fields
        .iter()
        .map(|field| {
            let info = infer_expr(
                &field.value,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            calls.extend(info.calls.iter().cloned());
            (field, info)
        })
        .collect::<Vec<_>>();

    let Some(layout) = struct_layouts.get(&expr.name) else {
        let suggestion = best_match(&expr.name, struct_layouts.keys().map(String::as_str));
        diagnostics.push(Diagnostic::new(
            "semantic.record-type",
            format!("record literal uses unknown struct `{}`", expr.name),
            expr.span,
            Some(suggestion_help(suggestion, || {
                "Declare the struct before constructing it.".to_owned()
            })),
        ));
        return ExprInfo {
            ty: Type::Error,
            calls,
        };
    };

    let mut ok = true;
    let mut seen = HashSet::<String>::new();
    for (field, info) in &field_infos {
        let Some(expected) = layout
            .iter()
            .find_map(|(name, ty)| (name == &field.name).then_some(ty))
        else {
            let suggestion = best_match(&field.name, layout.iter().map(|(name, _)| name.as_str()));
            diagnostics.push(Diagnostic::new(
                "semantic.record-field",
                format!("struct `{}` has no field `{}`", expr.name, field.name),
                field.span,
                Some(suggestion_help(suggestion, || {
                    "Use one of the declared struct fields.".to_owned()
                })),
            ));
            ok = false;
            continue;
        };
        if !seen.insert(field.name.clone()) {
            diagnostics.push(Diagnostic::new(
                "semantic.record-field",
                format!(
                    "record literal for `{}` initializes field `{}` more than once",
                    expr.name, field.name
                ),
                field.span,
                Some("Initialize each field exactly once.".to_owned()),
            ));
            ok = false;
        }
        if info.ty != *expected && info.ty != Type::Error {
            diagnostics.push(Diagnostic::new(
                "semantic.record-field-type",
                format!(
                    "field `{}` of `{}` expects `{}`, found `{}`",
                    field.name,
                    expr.name,
                    expected.render(),
                    info.ty.render(),
                ),
                field.span,
                Some("Make the field initializer match the declared field type.".to_owned()),
            ));
            ok = false;
        }
    }

    for (field_name, _) in layout {
        if !seen.contains(field_name) {
            diagnostics.push(Diagnostic::new(
                "semantic.record-field",
                format!(
                    "record literal for `{}` is missing field `{field_name}`",
                    expr.name
                ),
                expr.span,
                Some("Initialize every declared field exactly once.".to_owned()),
            ));
            ok = false;
        }
    }

    if field_infos.iter().any(|(_, info)| info.ty == Type::Error) {
        ok = false;
    }

    ExprInfo {
        ty: if ok {
            Type::Named(expr.name.clone())
        } else {
            Type::Error
        },
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_comptime_expr(
    body: &crate::hir::Body,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
) -> ExprInfo {
    // Comptime evaluation: runs the body at compile time with the same
    // type inference as runtime code. This enables:
    // - Pre-computing lookup tables
    // - Compile-time constant folding
    // - Deterministic initialization
    let info = super::infer_body(
        body,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );
    if body_contains_forbidden_comptime_effect(body) {
        diagnostics.push(Diagnostic::new(
            "semantic.comptime-effect",
            "comptime blocks may not perform effects or allocate dynamically",
            body.span,
            Some("Use deterministic constant operations only.".to_owned()),
        ));
        return ExprInfo {
            ty: Type::Error,
            calls: info.calls,
        };
    }
    ExprInfo {
        ty: info.ty,
        calls: info.calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_perform_expr(
    expr: &crate::hir::PerformExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
) -> ExprInfo {
    let mut calls = Vec::new();
    for arg in &expr.args {
        let info = super::infer_expr(
            arg,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            &ExprContext::default(),
        );
        calls.extend(info.calls);
    }

    // For now, we'll return Unit for performs until we implement effect declarations.
    ExprInfo {
        ty: Type::Unit,
        calls,
    }
}

fn body_contains_forbidden_comptime_effect(body: &crate::hir::Body) -> bool {
    body.statements.iter().any(|stmt| match stmt {
        crate::hir::Stmt::Let(binding) => contains_forbidden_comptime_effect(&binding.value),
        crate::hir::Stmt::Assign(assign) => contains_forbidden_comptime_effect(&assign.value),
        crate::hir::Stmt::Expr(expr) => contains_forbidden_comptime_effect(&expr.expr),
    }) || body
        .tail
        .as_ref()
        .is_some_and(contains_forbidden_comptime_effect)
}

fn contains_forbidden_comptime_effect(expr: &crate::hir::Expr) -> bool {
    use crate::hir::Expr;
    match expr {
        Expr::Call(call) => {
            matches!(
                call.callee.as_str(),
                "list_new"
                    | "text_builder_new"
                    | "text_builder_append"
                    | "text_builder_append_codepoint"
                    | "text_builder_append_ascii"
                    | "text_builder_append_slice"
                    | "text_builder_append_i32"
                    | "text_builder_finish"
                    | "stdout_write_builder"
                    | "list_push"
                    | "list_sort_text"
                    | "list_sort_by_text_field"
                    | "text_index_new"
                    | "text_index_get"
                    | "text_index_set"
                    | "text_line_end"
                    | "text_next_line"
                    | "text_field_end"
                    | "text_next_field"
                    | "stdin_bytes"
            ) || call.args.iter().any(contains_forbidden_comptime_effect)
        }
        Expr::Array(array) => array
            .elements
            .iter()
            .any(contains_forbidden_comptime_effect),
        Expr::Record(record) => record
            .fields
            .iter()
            .any(|f| contains_forbidden_comptime_effect(&f.value)),
        Expr::Group(group) => contains_forbidden_comptime_effect(&group.inner),
        Expr::Unary(unary) => contains_forbidden_comptime_effect(&unary.inner),
        Expr::Binary(binary) => {
            contains_forbidden_comptime_effect(&binary.left)
                || contains_forbidden_comptime_effect(&binary.right)
        }
        Expr::If(if_expr) => {
            contains_forbidden_comptime_effect(&if_expr.condition)
                || body_contains_forbidden_comptime_effect(&if_expr.then_body)
                || body_contains_forbidden_comptime_effect(&if_expr.else_body)
        }
        Expr::Match(match_expr) => {
            contains_forbidden_comptime_effect(&match_expr.scrutinee)
                || match_expr
                    .arms
                    .iter()
                    .any(|arm| body_contains_forbidden_comptime_effect(&arm.body))
        }
        Expr::While(while_expr) => {
            contains_forbidden_comptime_effect(&while_expr.condition)
                || body_contains_forbidden_comptime_effect(&while_expr.body)
        }
        Expr::Repeat(repeat_expr) => {
            contains_forbidden_comptime_effect(&repeat_expr.count)
                || body_contains_forbidden_comptime_effect(&repeat_expr.body)
        }
        Expr::Comptime(body) => body_contains_forbidden_comptime_effect(body),
        Expr::Perform(_) => true,
        Expr::Handle(handle) => {
            body_contains_forbidden_comptime_effect(&handle.body)
                || handle
                    .arms
                    .iter()
                    .any(|arm| body_contains_forbidden_comptime_effect(&arm.body))
        }
        Expr::Integer(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Name(_)
        | Expr::Field(_)
        | Expr::Index(_)
        | Expr::ContractResult(_) => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_handle_expr(
    expr: &crate::hir::HandleExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
) -> ExprInfo {
    let info = super::infer_body(
        &expr.body,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );
    ExprInfo {
        ty: info.ty,
        calls: info.calls,
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn infer_array_expr(
    expr: &crate::hir::ArrayExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let mut calls = Vec::new();
    let mut element_type = None::<Type>;
    let mut ok = true;

    if expr.elements.is_empty() {
        diagnostics.push(Diagnostic::new(
            "semantic.array-empty",
            format!("empty array literal in `{fn_name}` has no inferable type"),
            expr.span,
            Some("Use a non-empty array literal in stage-0.".to_owned()),
        ));
        ok = false;
    }

    for element in &expr.elements {
        let info = infer_expr(
            element,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            &nested_context,
        );
        calls.extend(info.calls);
        if let Some(expected) = &element_type {
            if *expected != info.ty && info.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.array-element-type",
                    format!(
                        "array elements in `{fn_name}` must have the same type, found `{}` and `{}`",
                        expected.render(),
                        info.ty.render(),
                    ),
                    element.span(),
                    Some("Make all array elements the same type.".to_owned()),
                ));
                ok = false;
            }
        } else {
            element_type = Some(info.ty.clone());
        }
        if info.ty == Type::Error {
            ok = false;
        }
    }

    if expr.repeat_len.is_some() && expr.elements.len() != 1 {
        diagnostics.push(Diagnostic::new(
            "semantic.array-repeat-shape",
            format!("repeat array literal in `{fn_name}` must use exactly one element expression"),
            expr.span,
            Some(
                "Use `[value; N]` for repeated arrays or `[a, b, c]` for explicit elements."
                    .to_owned(),
            ),
        ));
        ok = false;
    }

    if expr.repeat_len.as_ref() == Some(&ConstExpr::Literal(0)) {
        diagnostics.push(Diagnostic::new(
            "semantic.array-repeat-empty",
            format!("repeat array literal in `{fn_name}` must have positive length"),
            expr.span,
            Some("Use a positive fixed array length in `[value; N]`.".to_owned()),
        ));
        ok = false;
    }

    if expr.repeat_len.is_some()
        && element_type
            .as_ref()
            .is_some_and(|ty| type_contains_affine_values(ty, struct_layouts, enum_variants))
    {
        diagnostics.push(Diagnostic::new(
            "semantic.array-repeat-affine",
            format!(
                "repeat array literal in `{fn_name}` cannot duplicate affine element type `{}`",
                element_type
                    .as_ref()
                    .expect("repeat array element type should be present")
                    .render()
            ),
            expr.span,
            Some(
                "Use explicit elements for affine values, or keep `[value; N]` on duplicate-safe scalar/plain values."
                    .to_owned(),
            ),
        ));
        ok = false;
    }

    ExprInfo {
        ty: if ok {
            Type::Array(
                Box::new(element_type.unwrap_or(Type::Error)),
                expr.repeat_len.clone().unwrap_or_else(|| {
                    ConstExpr::Literal(u32::try_from(expr.elements.len()).unwrap_or(0))
                }),
            )
        } else {
            Type::Error
        },
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_contract_result_expr(
    expr: &crate::hir::ContractResultExpr,
    locals: &HashMap<String, Type>,
    _mutable_locals: &HashSet<String>,
    _functions: &BTreeMap<String, FunctionSignature>,
    _consts: &BTreeMap<String, ConstSignature>,
    _enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    _struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    _fn_name: &str,
    _caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    if !matches!(context, ExprContext::ContractEnsures) {
        diagnostics.push(Diagnostic::new(
            "semantic.contract-result-context",
            "`result` is only available inside `ensures` clauses",
            expr.span,
            Some("Use local bindings or parameters instead.".to_owned()),
        ));
    }
    if let Some(ty) = locals.get("result") {
        ExprInfo {
            ty: ty.clone(),
            calls: Vec::new(),
        }
    } else {
        ExprInfo {
            ty: Type::Error,
            calls: Vec::new(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_group_expr(
    expr: &crate::hir::GroupExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    infer_expr(
        &expr.inner,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_index_expr(
    expr: &crate::hir::IndexExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let base = infer_expr(
        &expr.base,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    let index = infer_expr(
        &expr.index,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    if index.ty != Type::I32 && index.ty != Type::Error {
        diagnostics.push(Diagnostic::new(
            "semantic.array-index-type",
            format!(
                "array index in `{fn_name}` must be `I32`, found `{}`",
                index.ty.render(),
            ),
            expr.index.span(),
            Some("Use an integer index for stage-0 array access.".to_owned()),
        ));
    }
    let ty = match &base.ty {
        Type::Array(element, _) | Type::List(element) => (**element).clone(),
        Type::Text => Type::I32,
        Type::Error => Type::Error,
        other => {
            diagnostics.push(Diagnostic::new(
                "semantic.array-index-base",
                format!(
                    "cannot index value of type `{}` in `{fn_name}`",
                    other.pretty(),
                ),
                expr.base.span(),
                Some("Index into a Text, List, or array value in stage-0.".to_owned()),
            ));
            Type::Error
        }
    };
    let mut calls = base.calls;
    calls.extend(index.calls);
    ExprInfo {
        ty: if base.ty == Type::Error || index.ty == Type::Error {
            Type::Error
        } else {
            ty
        },
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_if_expr(
    expr: &crate::hir::IfExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
) -> ExprInfo {
    let condition = infer_expr(
        &expr.condition,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &ExprContext::Body,
    );
    if condition.ty != Type::Bool && condition.ty != Type::Error {
        diagnostics.push(Diagnostic::new(
            "semantic.if-condition",
            format!(
                "`if` condition in `{fn_name}` must be `Bool`, found `{}`",
                condition.ty.render(),
            ),
            expr.condition.span(),
            Some("Use a boolean condition before the branch bodies.".to_owned()),
        ));
    }
    let then_info = super::infer_body(
        &expr.then_body,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );
    let else_info = super::infer_body(
        &expr.else_body,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );
    let mut calls = condition.calls;
    calls.extend(then_info.calls);
    calls.extend(else_info.calls);
    if then_info.ty != Type::Error
        && else_info.ty != Type::Error
        && !types_compatible(&then_info.ty, &else_info.ty)
    {
        diagnostics.push(Diagnostic::new(
            "semantic.if-branch-type",
            format!(
                "`if` branches in `{fn_name}` must return the same type, found `{}` and `{}`",
                then_info.ty.render(),
                else_info.ty.render(),
            ),
            expr.span,
            Some("Make both branch bodies produce the same type.".to_owned()),
        ));
    }
    let has_type_error = condition.ty == Type::Error
        || then_info.ty == Type::Error
        || else_info.ty == Type::Error
        || !types_compatible(&then_info.ty, &else_info.ty);
    ExprInfo {
        ty: if has_type_error {
            Type::Error
        } else {
            then_info.ty
        },
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_repeat_expr(
    expr: &crate::hir::RepeatExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    if !matches!(
        context,
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement
    ) {
        diagnostics.push(Diagnostic::new(
            "semantic.repeat-position",
            format!(
                "`repeat` in `{fn_name}` is only allowed as a direct body tail expression or statement"
            ),
            expr.span,
            Some(
                "Place `repeat` directly at the end of a body, or use it as an explicit statement."
                    .to_owned(),
            ),
        ));
    }
    let count = infer_expr(
        &expr.count,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &ExprContext::Body,
    );
    if count.ty != Type::I32 && count.ty != Type::Error {
        diagnostics.push(Diagnostic::new(
            "semantic.repeat-count",
            format!(
                "`repeat` count in `{fn_name}` must be `I32`, found `{}`",
                count.ty.render(),
            ),
            expr.count.span(),
            Some("Use an integer iteration count for stage-0 repeat loops.".to_owned()),
        ));
    }
    let mut body_locals = locals.clone();
    if let Some(binding) = &expr.binding {
        if body_locals.contains_key(binding) {
            diagnostics.push(Diagnostic::new(
                "semantic.duplicate-local",
                format!("local binding `{binding}` is already declared in `{fn_name}`"),
                expr.span,
                Some(
                    "Use a fresh loop index name instead of shadowing parameters or earlier locals."
                        .to_owned(),
                ),
            ));
        } else {
            body_locals.insert(binding.clone(), Type::I32);
        }
    }
    let body = super::infer_body(
        &expr.body,
        &body_locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );
    let mut calls = count.calls;
    calls.extend(body.calls);
    ExprInfo {
        ty: Type::Unit,
        calls,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_while_expr(
    expr: &crate::hir::WhileExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    if !matches!(
        context,
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement
    ) {
        diagnostics.push(Diagnostic::new(
            "semantic.while-position",
            format!(
                "`while` in `{fn_name}` is only allowed as a direct body tail expression or statement"
            ),
            expr.span,
            Some(
                "Place `while` directly at the end of a body, or use it as an explicit statement."
                    .to_owned(),
            ),
        ));
    }
    let condition = infer_expr(
        &expr.condition,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &ExprContext::Body,
    );
    if condition.ty != Type::Bool && condition.ty != Type::Error {
        diagnostics.push(Diagnostic::new(
            "semantic.while-condition",
            format!(
                "`while` condition in `{fn_name}` must be `Bool`, found `{}`",
                condition.ty.render(),
            ),
            expr.condition.span(),
            Some("Use a boolean condition for stage-0 `while` loops.".to_owned()),
        ));
    }
    let body = super::infer_body(
        &expr.body,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );
    let mut calls = condition.calls;
    calls.extend(body.calls);
    ExprInfo {
        ty: Type::Unit,
        calls,
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn infer_match_expr(
    expr: &crate::hir::MatchExpr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    let nested_context = nested_expr_context(context);
    let scrutinee = infer_expr(
        &expr.scrutinee,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        &nested_context,
    );
    let enum_name = match &scrutinee.ty {
        Type::Named(name) if enum_variants.contains_key(name) => Some(name.clone()),
        _ => None,
    };
    let declared_variants = enum_name
        .as_ref()
        .and_then(|name| enum_variants.get(name))
        .cloned()
        .unwrap_or_default();
    let supported_scrutinee = matches!(
        scrutinee.ty,
        Type::I32 | Type::Bool | Type::Text | Type::Error
    ) || enum_name.is_some();
    if !supported_scrutinee && scrutinee.ty != Type::Error {
        diagnostics.push(Diagnostic::new(
            "semantic.match-scrutinee",
            format!(
                "`match` in `{fn_name}` requires `I32`, `Bool`, `Text`, or an enum scrutinee, found `{}`",
                scrutinee.ty.render(),
            ),
            expr.scrutinee.span(),
            Some("Match over an integer, boolean, text, or declared enum value.".to_owned()),
        ));
    }

    let mut seen_enum_variants = BTreeSet::<String>::new();
    let mut seen_int_patterns = BTreeSet::<i64>::new();
    let mut seen_int_ranges = Vec::<(i64, i64, Span)>::new();
    let mut seen_text_patterns = BTreeSet::<String>::new();
    let mut seen_true = false;
    let mut seen_false = false;
    let mut has_wildcard = false;
    let mut branch_type = None::<Type>;
    let mut calls = scrutinee.calls;
    let mut ok = supported_scrutinee || scrutinee.ty == Type::Error;
    for (arm_index, arm) in expr.arms.iter().enumerate() {
        let is_last_arm = arm_index + 1 == expr.arms.len();
        let mut variant_payload = None::<Type>;

        match (&scrutinee.ty, &arm.pattern) {
            (_, crate::hir::MatchPattern::Wildcard { span }) => {
                if has_wildcard {
                    diagnostics.push(Diagnostic::new(
                        "semantic.match-pattern",
                        format!("wildcard arm `_` appears more than once in `{fn_name}`"),
                        *span,
                        Some("Keep at most one wildcard arm.".to_owned()),
                    ));
                    ok = false;
                }
                if !is_last_arm {
                    diagnostics.push(Diagnostic::new(
                        "semantic.match-pattern",
                        "wildcard arm `_` must be the final match arm",
                        *span,
                        Some("Move `_` to the final arm so later arms stay reachable.".to_owned()),
                    ));
                    ok = false;
                }
                has_wildcard = true;
            }
            (Type::Named(expected_enum), _) if enum_name.is_some() => {
                let (arm_ok, payload) = check_enum_match_pattern(
                    &arm.pattern,
                    expected_enum,
                    &declared_variants,
                    &mut seen_enum_variants,
                    diagnostics,
                    fn_name,
                );
                ok &= arm_ok;
                variant_payload = payload;
            }
            (Type::Bool, _) => {
                ok &= check_bool_match_pattern(
                    &arm.pattern,
                    &mut seen_true,
                    &mut seen_false,
                    diagnostics,
                    fn_name,
                );
            }
            (Type::I32, _) => {
                ok &= check_i32_match_pattern(
                    &arm.pattern,
                    &mut seen_int_patterns,
                    &mut seen_int_ranges,
                    diagnostics,
                    fn_name,
                );
            }
            (Type::Text, _) => {
                ok &= check_text_match_pattern(
                    &arm.pattern,
                    &mut seen_text_patterns,
                    diagnostics,
                    fn_name,
                );
            }
            (Type::Error, _) => {}
            (_, _) => {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "match arm `{}` is not compatible with scrutinee type `{}`",
                        arm.pattern.pretty(),
                        scrutinee.ty.render(),
                    ),
                    arm.pattern.span(),
                    Some(
                        "Use enum variants, matching literal kinds, integer ranges, or `_`."
                            .to_owned(),
                    ),
                ));
                ok = false;
            }
        }

        let mut arm_locals = locals.clone();
        if let (
            crate::hir::MatchPattern::Variant {
                binding: Some(binding),
                ..
            },
            Some(payload_ty),
        ) = (&arm.pattern, variant_payload)
        {
            arm_locals.insert(binding.clone(), payload_ty);
        }

        let body = super::infer_body(
            &arm.body,
            &arm_locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
        );
        calls.extend(body.calls);
        if let Some(expected) = &branch_type {
            if !types_compatible(expected, &body.ty) && body.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-branch-type",
                    format!(
                        "match arms in `{fn_name}` must return the same type, found `{}` and `{}`",
                        expected.render(),
                        body.ty.render(),
                    ),
                    arm.body.span,
                    Some("Make every match arm produce the same type.".to_owned()),
                ));
                ok = false;
            }
        } else {
            branch_type = Some(body.ty.clone());
        }
        if body.ty == Type::Error {
            ok = false;
        }
    }
    if let Some(expected_enum) = &enum_name {
        let missing = declared_variants
            .iter()
            .filter(|variant| !seen_enum_variants.contains(variant.name.as_str()))
            .map(|variant| variant.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() && !has_wildcard {
            diagnostics.push(Diagnostic::new(
                "semantic.match-exhaustive",
                format!(
                    "match in `{fn_name}` is not exhaustive for enum `{expected_enum}`; missing {}",
                    missing.join(", "),
                ),
                expr.span,
                Some("Add one arm for every declared enum variant.".to_owned()),
            ));
            ok = false;
        }
    } else if scrutinee.ty == Type::Bool {
        if !(has_wildcard || (seen_true && seen_false)) {
            diagnostics.push(Diagnostic::new(
                "semantic.match-exhaustive",
                format!(
                    "match in `{fn_name}` is not exhaustive for `Bool`; add the missing boolean arm or `_`"
                ),
                expr.span,
                Some("Cover both `true` and `false`, or end with `_`.".to_owned()),
            ));
            ok = false;
        }
    } else if matches!(scrutinee.ty, Type::I32 | Type::Text) && !has_wildcard {
        diagnostics.push(Diagnostic::new(
            "semantic.match-exhaustive",
            format!(
                "match in `{fn_name}` over `{}` requires a final `_` fallback arm",
                scrutinee.ty.render(),
            ),
            expr.span,
            Some("End integer and text matches with `_ => { ... }`.".to_owned()),
        ));
        ok = false;
    }
    ExprInfo {
        ty: if ok {
            branch_type.unwrap_or(Type::Unit)
        } else {
            Type::Error
        },
        calls,
    }
}

fn check_enum_match_pattern(
    pattern: &crate::hir::MatchPattern,
    expected_enum: &str,
    declared_variants: &[EnumVariantInfo],
    seen_enum_variants: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
) -> (bool, Option<Type>) {
    let crate::hir::MatchPattern::Variant {
        path,
        binding,
        span,
    } = pattern
    else {
        diagnostics.push(Diagnostic::new(
            "semantic.match-pattern",
            format!(
                "match arm `{}` is not compatible with scrutinee type `{expected_enum}`",
                pattern.pretty(),
            ),
            pattern.span(),
            Some("Use variants from the scrutinee's enum only.".to_owned()),
        ));
        return (false, None);
    };

    let Some((pattern_enum, variant_name)) = split_enum_variant_path(&path.path) else {
        diagnostics.push(Diagnostic::new(
            "semantic.match-pattern",
            format!(
                "match arm in `{fn_name}` must use `Enum.variant`, found `{}`",
                path.path,
            ),
            *span,
            Some("Rewrite the arm pattern as `Enum.variant`.".to_owned()),
        ));
        return (false, None);
    };
    if pattern_enum != expected_enum {
        diagnostics.push(Diagnostic::new(
            "semantic.match-pattern",
            format!(
                "match arm `{}` does not belong to enum `{expected_enum}`",
                pattern.pretty(),
            ),
            *span,
            Some("Use variants from the scrutinee's enum only.".to_owned()),
        ));
        return (false, None);
    }

    let Some(variant_info) = declared_variants
        .iter()
        .find(|variant| variant.name == variant_name)
    else {
        let suggestion = best_match(
            variant_name,
            declared_variants
                .iter()
                .map(|variant| variant.name.as_str()),
        );
        diagnostics.push(Diagnostic::new(
            "semantic.match-pattern",
            format!("enum `{expected_enum}` has no variant `{variant_name}`"),
            *span,
            Some(suggestion_help(suggestion, || {
                "Use one of the declared enum variants.".to_owned()
            })),
        ));
        return (false, None);
    };

    let mut ok = true;
    if !seen_enum_variants.insert(variant_name.to_owned()) {
        diagnostics.push(Diagnostic::new(
            "semantic.match-pattern",
            format!(
                "match arm `{}` appears more than once in `{fn_name}`",
                pattern.pretty()
            ),
            *span,
            Some("Keep one arm per enum variant.".to_owned()),
        ));
        ok = false;
    }
    let payload = variant_info.payload.clone();
    if payload.is_none() && binding.is_some() {
        diagnostics.push(Diagnostic::new(
            "semantic.match-pattern",
            format!(
                "payload binding `{}` is only valid for payload-carrying variants",
                binding.as_deref().unwrap_or("_"),
            ),
            *span,
            Some("Remove the binding, or match a variant that carries a payload.".to_owned()),
        ));
        ok = false;
    }
    (ok, payload)
}

#[allow(clippy::unnecessary_fold)]
fn check_bool_match_pattern(
    pattern: &crate::hir::MatchPattern,
    seen_true: &mut bool,
    seen_false: &mut bool,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
) -> bool {
    match pattern {
        crate::hir::MatchPattern::Bool { value, span } => {
            let seen = if *value { seen_true } else { seen_false };
            if *seen {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "match arm `{}` appears more than once in `{fn_name}`",
                        pattern.pretty()
                    ),
                    *span,
                    Some("Keep one arm per boolean value.".to_owned()),
                ));
                return false;
            }
            *seen = true;
            true
        }
        crate::hir::MatchPattern::Or { patterns, .. } => patterns.iter().fold(true, |ok, part| {
            check_bool_match_pattern(part, seen_true, seen_false, diagnostics, fn_name) && ok
        }),
        _ => {
            diagnostics.push(Diagnostic::new(
                "semantic.match-pattern",
                format!(
                    "match arm `{}` is not compatible with scrutinee type `Bool`",
                    pattern.pretty()
                ),
                pattern.span(),
                Some("Use `true`, `false`, `true | false`, or `_`.".to_owned()),
            ));
            false
        }
    }
}

#[allow(clippy::unnecessary_fold)]
fn check_i32_match_pattern(
    pattern: &crate::hir::MatchPattern,
    seen_int_patterns: &mut BTreeSet<i64>,
    seen_int_ranges: &mut Vec<(i64, i64, Span)>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
) -> bool {
    match pattern {
        crate::hir::MatchPattern::Integer { value, span } => {
            let mut ok = true;
            if !seen_int_patterns.insert(*value) {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "match arm `{}` appears more than once in `{fn_name}`",
                        pattern.pretty()
                    ),
                    *span,
                    Some("Keep one arm per integer literal.".to_owned()),
                ));
                ok = false;
            }
            if let Some((start, end, _)) = seen_int_ranges
                .iter()
                .copied()
                .find(|(start, end, _)| *start <= *value && *value < *end)
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "integer pattern `{value}` overlaps prior range `{start}..{end}` in `{fn_name}`"
                    ),
                    *span,
                    Some("Keep integer match arms disjoint.".to_owned()),
                ));
                ok = false;
            }
            ok
        }
        crate::hir::MatchPattern::IntegerRange { start, end, span } => {
            let mut ok = true;
            if start >= end {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!("integer range `{start}..{end}` must have `start < end`"),
                    *span,
                    Some("Use a non-empty half-open range such as `65..91`.".to_owned()),
                ));
                ok = false;
            }
            if seen_int_patterns
                .iter()
                .copied()
                .any(|value| *start <= value && value < *end)
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "integer range `{start}..{end}` overlaps a prior integer arm in `{fn_name}`"
                    ),
                    *span,
                    Some("Keep integer match arms disjoint.".to_owned()),
                ));
                ok = false;
            }
            if seen_int_ranges
                .iter()
                .any(|(other_start, other_end, _)| *start < *other_end && *other_start < *end)
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "integer range `{start}..{end}` overlaps a prior range arm in `{fn_name}`"
                    ),
                    *span,
                    Some("Keep integer match arms disjoint.".to_owned()),
                ));
                ok = false;
            }
            seen_int_ranges.push((*start, *end, *span));
            ok
        }
        crate::hir::MatchPattern::Or { patterns, .. } => patterns.iter().fold(true, |ok, part| {
            check_i32_match_pattern(
                part,
                seen_int_patterns,
                seen_int_ranges,
                diagnostics,
                fn_name,
            ) && ok
        }),
        _ => {
            diagnostics.push(Diagnostic::new(
                "semantic.match-pattern",
                format!(
                    "match arm `{}` is not compatible with scrutinee type `I32`",
                    pattern.pretty()
                ),
                pattern.span(),
                Some("Use integer literals, integer ranges, `|`, or `_`.".to_owned()),
            ));
            false
        }
    }
}

#[allow(clippy::unnecessary_fold)]
fn check_text_match_pattern(
    pattern: &crate::hir::MatchPattern,
    seen_text_patterns: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
) -> bool {
    match pattern {
        crate::hir::MatchPattern::String { value, span, .. } => {
            if !seen_text_patterns.insert(value.clone()) {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-pattern",
                    format!(
                        "match arm `{}` appears more than once in `{fn_name}`",
                        pattern.pretty()
                    ),
                    *span,
                    Some("Keep one arm per text literal.".to_owned()),
                ));
                return false;
            }
            true
        }
        crate::hir::MatchPattern::Or { patterns, .. } => patterns.iter().fold(true, |ok, part| {
            check_text_match_pattern(part, seen_text_patterns, diagnostics, fn_name) && ok
        }),
        _ => {
            diagnostics.push(Diagnostic::new(
                "semantic.match-pattern",
                format!(
                    "match arm `{}` is not compatible with scrutinee type `Text`",
                    pattern.pretty()
                ),
                pattern.span(),
                Some("Use text literals, `|`, or `_`.".to_owned()),
            ));
            false
        }
    }
}

const ARRAY_LEN_BUILTIN: &str = "len";

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn infer_expr(
    expr: &Expr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    super::infer_expr(
        expr,
        locals,
        mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
        context,
    )
}
