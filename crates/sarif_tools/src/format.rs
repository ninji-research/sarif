use std::fmt::Write;

use sarif_syntax::ast::{AstFile, BinaryOp, Expr, Item};

#[must_use]
pub fn format_file(file: &AstFile) -> String {
    let mut output = String::new();

    for (index, item) in file.items.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }

        format_item(&mut output, item);
    }

    output
}

fn format_item(output: &mut String, item: &Item) {
    match item {
        Item::Const(const_item) => {
            writeln!(
                output,
                "const {}: {} = {};",
                const_item.name,
                const_item.ty,
                format_expr(&const_item.value),
            )
            .expect("writing to string cannot fail");
        }
        Item::Function(function) => format_function_item(output, function),
        Item::Enum(enum_item) => {
            writeln!(output, "enum {} {{", enum_item.name).expect("writing to string cannot fail");
            for variant in &enum_item.variants {
                writeln!(
                    output,
                    "    {}{},",
                    variant.name,
                    variant
                        .payload
                        .as_ref()
                        .map_or_else(String::new, |payload| format!("({payload})")),
                )
                .expect("writing to string cannot fail");
            }
            output.push('}');
            output.push('\n');
        }
        Item::Struct(struct_item) => {
            writeln!(output, "struct {} {{", struct_item.name)
                .expect("writing to string cannot fail");
            for field in &struct_item.fields {
                writeln!(output, "    {}: {},", field.name, field.ty)
                    .expect("writing to string cannot fail");
            }
            output.push('}');
            output.push('\n');
        }
        Item::Effect(effect_item) => {
            writeln!(output, "effect {} {{", effect_item.name)
                .expect("writing to string cannot fail");
            for method in &effect_item.methods {
                write!(output, "    fn {}(", method.name).expect("writing to string cannot fail");
                for (param_index, param) in method.params.iter().enumerate() {
                    if param_index > 0 {
                        output.push_str(", ");
                    }
                    write!(output, "{}: {}", param.name, param.ty)
                        .expect("writing to string cannot fail");
                }
                output.push(')');
                if let Some(return_type) = &method.return_type {
                    write!(output, " -> {return_type}").expect("writing to string cannot fail");
                }
                output.push_str(";\n");
            }
            output.push('}');
            output.push('\n');
        }
    }
}

fn format_function_item(output: &mut String, function: &sarif_syntax::ast::Function) {
    write!(output, "fn {}(", function.name).expect("writing to string cannot fail");
    for (param_index, param) in function.params.iter().enumerate() {
        if param_index > 0 {
            output.push_str(", ");
        }
        write!(output, "{}: {}", param.name, param.ty).expect("writing to string cannot fail");
    }
    output.push(')');
    if let Some(return_type) = &function.return_type {
        write!(output, " -> {return_type}").expect("writing to string cannot fail");
    }
    if !function.effects.is_empty() {
        let mut effects = function.effects.iter().collect::<Vec<_>>();
        effects.sort_by_key(|effect| effect_rank(&effect.name));
        write!(
            output,
            " effects [{}]",
            effects
                .iter()
                .map(|effect| effect.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
        .expect("writing to string cannot fail");
    }
    if let Some(requires) = &function.requires {
        write!(output, " requires {}", format_expr(requires))
            .expect("writing to string cannot fail");
    }
    if let Some(ensures) = &function.ensures {
        write!(output, " ensures {}", format_expr(ensures)).expect("writing to string cannot fail");
    }
    output.push_str(" {\n");
    if let Some(body) = &function.body {
        format_body(output, body);
    }
    output.push('}');
    output.push('\n');
}

fn format_body(output: &mut String, body: &sarif_syntax::ast::Body) {
    format_body_with_indent(output, body, 1);
}

fn format_body_with_indent(output: &mut String, body: &sarif_syntax::ast::Body, indent: usize) {
    for statement in &body.statements {
        match statement {
            sarif_syntax::ast::Stmt::Let(binding) => {
                write_indent(output, indent);
                write!(
                    output,
                    "let {}{} = {};",
                    if binding.mutable { "mut " } else { "" },
                    binding.name,
                    format_stmt_expr_with_indent(&binding.value, indent)
                )
                .expect("writing to string cannot fail");
                output.push('\n');
            }
            sarif_syntax::ast::Stmt::Assign(stmt) => {
                write_indent(output, indent);
                write!(
                    output,
                    "{} = {};",
                    format_stmt_expr_with_indent(&stmt.target, indent),
                    format_stmt_expr_with_indent(&stmt.value, indent)
                )
                .expect("writing to string cannot fail");
                output.push('\n');
            }
            sarif_syntax::ast::Stmt::Expr(stmt) => {
                write_indent(output, indent);
                write!(
                    output,
                    "{};",
                    format_stmt_expr_with_indent(&stmt.expr, indent)
                )
                .expect("writing to string cannot fail");
                output.push('\n');
            }
        }
    }
    if let Some(tail) = &body.tail {
        write_indent(output, indent);
        write!(output, "{}", format_stmt_expr_with_indent(tail, indent))
            .expect("writing to string cannot fail");
        output.push('\n');
    }
}

fn format_body_block(body: &sarif_syntax::ast::Body, indent: usize) -> String {
    let mut output = String::new();
    output.push_str("{\n");
    format_body_with_indent(&mut output, body, indent + 1);
    write_indent(&mut output, indent);
    output.push('}');
    output
}

fn format_expr(expr: &Expr) -> String {
    format_expr_with_indent(expr, 0)
}

fn format_stmt_expr_with_indent(expr: &Expr, indent: usize) -> String {
    format_multiline_binary_chain(expr, indent)
        .unwrap_or_else(|| format_expr_with_indent(expr, indent))
}

fn format_array_expr(expr: &sarif_syntax::ast::ArrayExpr, indent: usize) -> String {
    expr.repeat_len.as_ref().map_or_else(
        || {
            format!(
                "[{}]",
                expr.elements
                    .iter()
                    .map(|element| format_expr_with_indent(element, indent))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        },
        |repeat_len| {
            format!(
                "[{}; {}]",
                format_expr_with_indent(
                    expr.elements.first().expect("repeat array has one element"),
                    indent
                ),
                match repeat_len {
                    sarif_syntax::ast::ArrayLen::Literal(len) => len.to_string(),
                    sarif_syntax::ast::ArrayLen::Name(name) => name.clone(),
                }
            )
        },
    )
}

fn format_expr_with_indent(expr: &Expr, indent: usize) -> String {
    match expr {
        Expr::Integer(expr) => expr.value.to_string(),
        Expr::Float(expr) => format_float_literal(expr.value),
        Expr::String(expr) => expr.literal.clone(),
        Expr::Bool(expr) => expr.value.to_string(),
        Expr::Name(expr) => expr.name.clone(),
        Expr::ContractResult(_) => "result".to_owned(),
        Expr::Call(expr) => format!(
            "{}({})",
            expr.callee,
            expr.args
                .iter()
                .map(|arg| format_expr_with_indent(arg, indent))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Expr::Array(expr) => format_array_expr(expr, indent),
        Expr::Field(expr) => format!(
            "{}.{}",
            format_expr_with_indent(&expr.base, indent),
            expr.field
        ),
        Expr::Index(expr) => format!(
            "{}[{}]",
            format_expr_with_indent(&expr.base, indent),
            format_expr_with_indent(&expr.index, indent)
        ),
        Expr::If(expr) => format_if_expr(expr, indent),
        Expr::Match(expr) => format_match_expr(expr, indent),
        Expr::Repeat(expr) => format_repeat_expr(expr, indent),
        Expr::While(expr) => format_while_expr(expr, indent),
        Expr::Record(expr) => format!(
            "{} {{ {} }}",
            expr.name,
            expr.fields
                .iter()
                .map(|field| format!(
                    "{}: {}",
                    field.name,
                    format_expr_with_indent(&field.value, indent)
                ))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Expr::Unary(expr) => format!(
            "{} {}",
            expr.op.symbol(),
            format_expr_with_indent(&expr.inner, indent)
        ),
        Expr::Binary(expr) => format!(
            "{} {} {}",
            format_binary_operand(&expr.left, expr.op, false, indent),
            expr.op.symbol(),
            format_binary_operand(&expr.right, expr.op, true, indent),
        ),
        Expr::Group(expr) => format!("({})", format_expr_with_indent(&expr.inner, indent)),
        Expr::Comptime(expr) => format!("comptime {}", format_body_block(&expr.body, indent)),
        Expr::Perform(expr) => {
            let args = expr
                .args
                .iter()
                .map(|arg| format_expr_with_indent(arg, indent))
                .collect::<Vec<_>>()
                .join(", ");
            format!("perform {}({})", expr.callee, args)
        }
        Expr::Handle(expr) => format!(
            "handle {} with {{\n{}{}}}",
            format_body_block(&expr.body, indent),
            expr.arms
                .iter()
                .map(|arm| format!(
                    "{}{}({}) => {}",
                    "    ".repeat(indent + 1),
                    arm.name,
                    arm.params.join(", "),
                    format_body_block(&arm.body, indent + 1)
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            if expr.arms.is_empty() {
                String::new()
            } else {
                format!("\n{}", "    ".repeat(indent))
            }
        ),
    }
}

fn format_float_literal(value: f64) -> String {
    let mut literal = value.to_string();
    if !literal.contains(['.', 'e', 'E']) {
        literal.push_str(".0");
    }
    literal
}

fn format_multiline_binary_chain(expr: &Expr, indent: usize) -> Option<String> {
    let Expr::Binary(binary) = expr else {
        return None;
    };
    if !matches!(binary.op, BinaryOp::Add | BinaryOp::And | BinaryOp::Or) {
        return None;
    }

    let mut operands = Vec::new();
    collect_binary_chain_operands(expr, binary.op, &mut operands);
    if operands.len() <= 2 {
        return None;
    }

    let continuation_indent = "    ".repeat(indent);
    Some(
        operands
            .iter()
            .map(|operand| format_binary_operand(operand, binary.op, false, indent))
            .collect::<Vec<_>>()
            .join(&format!(" {}\n{}", binary.op.symbol(), continuation_indent)),
    )
}

fn collect_binary_chain_operands<'a>(expr: &'a Expr, op: BinaryOp, operands: &mut Vec<&'a Expr>) {
    match expr {
        Expr::Binary(binary) if binary.op == op => {
            collect_binary_chain_operands(&binary.left, op, operands);
            collect_binary_chain_operands(&binary.right, op, operands);
        }
        _ => operands.push(expr),
    }
}

fn format_if_expr(expr: &sarif_syntax::ast::IfExpr, indent: usize) -> String {
    let mut output = format!(
        "if {} {}",
        format_expr_with_indent(&expr.condition, indent),
        format_block(&expr.then_body, indent),
    );
    if !body_is_empty(&expr.else_body) {
        write!(output, " else {}", format_block(&expr.else_body, indent))
            .expect("writing to string cannot fail");
    }
    output
}

fn format_match_expr(expr: &sarif_syntax::ast::MatchExpr, indent: usize) -> String {
    let mut output = format!(
        "match {} {{",
        format_expr_with_indent(&expr.scrutinee, indent)
    );
    if expr.arms.is_empty() {
        output.push('}');
        return output;
    }
    output.push('\n');
    for arm in &expr.arms {
        push_indent(&mut output, indent + 1);
        write!(
            output,
            "{} => {},",
            format_match_pattern(&arm.pattern),
            format_block(&arm.body, indent + 1)
        )
        .expect("writing to string cannot fail");
        output.push('\n');
    }
    push_indent(&mut output, indent);
    output.push('}');
    output
}

fn format_repeat_expr(expr: &sarif_syntax::ast::RepeatExpr, indent: usize) -> String {
    format!(
        "{} {{{}}}",
        expr.binding.as_ref().map_or_else(
            || format!("repeat {}", format_expr_with_indent(&expr.count, indent)),
            |binding| format!(
                "repeat {binding} in {}",
                format_expr_with_indent(&expr.count, indent)
            ),
        ),
        format_block_contents(&expr.body, indent),
    )
}

fn format_while_expr(expr: &sarif_syntax::ast::WhileExpr, indent: usize) -> String {
    format!(
        "while {} {{{}}}",
        format_expr_with_indent(&expr.condition, indent),
        format_block_contents(&expr.body, indent),
    )
}

fn format_match_pattern(pattern: &sarif_syntax::ast::MatchPattern) -> String {
    pattern.pretty()
}

fn format_binary_operand(
    expr: &Expr,
    parent_op: BinaryOp,
    is_right_child: bool,
    indent: usize,
) -> String {
    match expr {
        Expr::Binary(inner) if needs_parens(parent_op, inner.op, is_right_child) => {
            format!("({})", format_expr_with_indent(expr, indent))
        }
        _ => format_expr_with_indent(expr, indent),
    }
}

fn format_block(body: &sarif_syntax::ast::Body, indent: usize) -> String {
    format!("{{{}}}", format_block_contents(body, indent))
}

const fn body_is_empty(body: &sarif_syntax::ast::Body) -> bool {
    body.statements.is_empty() && body.tail.is_none()
}

fn format_block_contents(body: &sarif_syntax::ast::Body, indent: usize) -> String {
    if body.statements.is_empty() && body.tail.is_none() {
        return String::new();
    }

    let mut output = String::new();
    output.push('\n');
    format_body_with_indent(&mut output, body, indent + 1);
    push_indent(&mut output, indent);
    output
}

fn write_indent(output: &mut String, indent: usize) {
    push_indent(output, indent);
}

fn push_indent(output: &mut String, indent: usize) {
    for _ in 0..indent {
        output.push_str("    ");
    }
}

const fn needs_parens(parent: BinaryOp, child: BinaryOp, is_right_child: bool) -> bool {
    let parent_precedence = precedence(parent);
    let child_precedence = precedence(child);
    child_precedence < parent_precedence
        || (is_right_child
            && child_precedence == parent_precedence
            && matches!(
                parent,
                BinaryOp::Sub
                    | BinaryOp::Div
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge
            ))
}

const fn precedence(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or => 1,
        BinaryOp::And => 2,
        BinaryOp::BitOr => 3,
        BinaryOp::BitXor => 4,
        BinaryOp::BitAnd => 5,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            6
        }
        BinaryOp::Shl | BinaryOp::Shr => 7,
        BinaryOp::Add | BinaryOp::Sub => 8,
        BinaryOp::Mul | BinaryOp::Div => 9,
    }
}

fn effect_rank(name: &str) -> (usize, &str) {
    let rank = match name {
        "io" => 0,
        "alloc" => 1,
        "async" => 2,
        "parallel" => 3,
        "clock" => 4,
        "ffi" => 5,
        "nondet" => 6,
        _ => usize::MAX,
    };
    (rank, name)
}
