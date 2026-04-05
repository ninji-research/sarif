use std::{collections::BTreeMap, fmt::Write};

use sarif_codegen::{
    NativeEnum, NativeRecord, NativeValueKind, Program, collect_native_enums,
    collect_native_records,
};

pub(super) fn runtime_metadata_source(program: &Program) -> Result<String, String> {
    let main = program
        .functions
        .iter()
        .find(|function| function.name == "main")
        .ok_or_else(|| "missing `main` entrypoint".to_owned())?;
    record_metadata_source(
        program,
        main.return_type.as_deref(),
        "sarif_get_main_record_desc",
        true,
    )
}

fn record_metadata_source(
    program: &Program,
    result_type: Option<&str>,
    getter_name: &str,
    include_type_defs: bool,
) -> Result<String, String> {
    let records = collect_native_records(program)?;
    let enums = collect_native_enums(program);
    let enum_getter_name = getter_name.replacen("record", "enum", 1);
    let mut output = String::new();
    if include_type_defs {
        emit_metadata_type_defs(&mut output);
    }
    emit_metadata_forward_decls(&mut output, &records, &enums);
    emit_enum_metadata(&mut output, &records, &enums);
    emit_record_metadata(&mut output, &records);
    emit_metadata_getters(
        &mut output,
        result_type.unwrap_or("Unit"),
        getter_name,
        &enum_getter_name,
        &records,
        &enums,
    );

    Ok(output)
}

fn emit_metadata_type_defs(output: &mut String) {
    output.push_str("#include <stdint.h>\n\n");
    output.push_str("typedef struct SarifRecordDesc SarifRecordDesc;\n");
    output.push_str("typedef struct SarifEnumDesc SarifEnumDesc;\n");
    output.push_str("typedef struct SarifVariantDesc SarifVariantDesc;\n");
    output.push_str("typedef struct SarifFieldDesc {\n");
    output.push_str("    const char* name;\n");
    output.push_str("    uint32_t kind;\n");
    output.push_str("    uint64_t offset;\n");
    output.push_str("    const SarifRecordDesc* record;\n");
    output.push_str("    const SarifEnumDesc* enum_desc;\n");
    output.push_str("} SarifFieldDesc;\n\n");
    output.push_str("struct SarifRecordDesc {\n");
    output.push_str("    const char* name;\n");
    output.push_str("    uint64_t field_count;\n");
    output.push_str("    const SarifFieldDesc* fields;\n");
    output.push_str("};\n\n");
    output.push_str("struct SarifVariantDesc {\n");
    output.push_str("    const char* name;\n");
    output.push_str("    uint32_t payload_kind;\n");
    output.push_str("    const SarifRecordDesc* record;\n");
    output.push_str("    const SarifEnumDesc* enum_desc;\n");
    output.push_str("};\n\n");
    output.push_str("struct SarifEnumDesc {\n");
    output.push_str("    const char* name;\n");
    output.push_str("    uint64_t variant_count;\n");
    output.push_str("    const SarifVariantDesc* variants;\n");
    output.push_str("};\n\n");
}

fn emit_metadata_forward_decls(
    output: &mut String,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) {
    for record in records.values() {
        let ident = record_ident(&record.name);
        writeln!(output, "static const struct SarifRecordDesc {ident};")
            .expect("writing to a string cannot fail");
    }
    for name in enums.keys() {
        let ident = enum_ident(name);
        writeln!(output, "static const struct SarifEnumDesc {ident};")
            .expect("writing to a string cannot fail");
    }
    if !records.is_empty() || !enums.is_empty() {
        output.push('\n');
    }
}

fn emit_enum_metadata(
    output: &mut String,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) {
    for (name, enum_ty) in enums {
        let ident = enum_ident(name);
        writeln!(
            output,
            "static const SarifVariantDesc {ident}_variants[] = {{"
        )
        .expect("writing to a string cannot fail");
        for variant in &enum_ty.variants {
            let (payload_kind, child_record, child_enum) =
                variant.payload_type.as_deref().map_or_else(
                    || (0, "0".to_owned(), "0".to_owned()),
                    |payload| payload_metadata(payload, records, enums),
                );
            writeln!(
                output,
                "    {{ {name}, {payload_kind}, {child_record}, {child_enum} }},",
                name = c_string(&variant.name),
            )
            .expect("writing to a string cannot fail");
        }
        output.push_str("};\n");
        writeln!(
            output,
            "static const struct SarifEnumDesc {ident} = {{ {}, {}u, {ident}_variants }};\n",
            c_string(name),
            enum_ty.variants.len(),
        )
        .expect("writing to a string cannot fail");
    }
}

fn emit_record_metadata(output: &mut String, records: &BTreeMap<String, NativeRecord>) {
    for record in records.values() {
        let ident = record_ident(&record.name);
        writeln!(output, "static const SarifFieldDesc {ident}_fields[] = {{")
            .expect("writing to a string cannot fail");
        for field in &record.fields {
            writeln!(
                output,
                "    {{ {name}, {kind}, {offset}u, {child_record}, {child_enum} }},",
                name = c_string(&field.name),
                kind = c_kind(&field.kind),
                offset = field.offset,
                child_record = child_record_expr(&field.kind),
                child_enum = child_enum_expr(&field.kind),
            )
            .expect("writing to a string cannot fail");
        }
        output.push_str("};\n");
        write!(
            output,
            "static const struct SarifRecordDesc {ident} = {{ {name}, {count}u, {ident}_fields }};\n\n",
            name = c_string(&record.name),
            count = record.fields.len(),
        )
        .expect("writing to a string cannot fail");
    }
}

fn emit_metadata_getters(
    output: &mut String,
    result_type: &str,
    getter_name: &str,
    enum_getter_name: &str,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) {
    let (record_result, enum_result) = if records.contains_key(result_type) {
        (format!("&{}", record_ident(result_type)), "0".to_owned())
    } else if enums.contains_key(result_type) {
        ("0".to_owned(), format!("&{}", enum_ident(result_type)))
    } else {
        ("0".to_owned(), "0".to_owned())
    };
    writeln!(
        output,
        "const struct SarifRecordDesc* {getter_name}(void) {{"
    )
    .expect("writing to a string cannot fail");
    writeln!(output, "    return {record_result};").expect("writing to a string cannot fail");
    output.push_str("}\n");
    writeln!(
        output,
        "const struct SarifEnumDesc* {enum_getter_name}(void) {{"
    )
    .expect("writing to a string cannot fail");
    writeln!(output, "    return {enum_result};").expect("writing to a string cannot fail");
    output.push_str("}\n");
}

fn child_record_expr(kind: &NativeValueKind) -> String {
    match kind {
        NativeValueKind::Record(name) => format!("&{}", record_ident(name)),
        NativeValueKind::Unit
        | NativeValueKind::I32
        | NativeValueKind::F64
        | NativeValueKind::Bool
        | NativeValueKind::Text
        | NativeValueKind::TextIndex
        | NativeValueKind::TextBuilder
        | NativeValueKind::List(_)
        | NativeValueKind::Enum(_) => "0".to_owned(),
    }
}

fn child_enum_expr(kind: &NativeValueKind) -> String {
    match kind {
        NativeValueKind::Enum(name) => format!("&{}", enum_ident(name)),
        NativeValueKind::Unit
        | NativeValueKind::I32
        | NativeValueKind::F64
        | NativeValueKind::Bool
        | NativeValueKind::Text
        | NativeValueKind::TextIndex
        | NativeValueKind::TextBuilder
        | NativeValueKind::List(_)
        | NativeValueKind::Record(_) => "0".to_owned(),
    }
}

fn payload_metadata(
    name: &str,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) -> (u32, String, String) {
    match name {
        "I32" => (1, "0".to_owned(), "0".to_owned()),
        "Bool" => (2, "0".to_owned(), "0".to_owned()),
        "Text" => (3, "0".to_owned(), "0".to_owned()),
        other if records.contains_key(other) => {
            (4, format!("&{}", record_ident(other)), "0".to_owned())
        }
        other if enums.contains_key(other) => {
            (5, "0".to_owned(), format!("&{}", enum_ident(other)))
        }
        other => (
            0,
            "0".to_owned(),
            format!("/* unknown payload type `{other}` */ 0"),
        ),
    }
}

fn record_ident(name: &str) -> String {
    format!("sarif_record_{}", c_ident(name))
}

fn enum_ident(name: &str) -> String {
    format!("sarif_enum_{}", c_ident(name))
}

fn c_ident(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "generated".to_owned()
    } else {
        output
    }
}

fn c_string(value: &str) -> String {
    let mut output = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'\\' => output.push_str("\\\\"),
            b'"' => output.push_str("\\\""),
            0x20..=0x7e => output.push(char::from(byte)),
            _ => write!(output, "\\x{byte:02x}").expect("writing to a string cannot fail"),
        }
    }
    output.push('"');
    output
}

const fn c_kind(kind: &NativeValueKind) -> u32 {
    match kind {
        NativeValueKind::Unit => 0,
        NativeValueKind::I32 => 1,
        NativeValueKind::Bool => 2,
        NativeValueKind::Text => 3,
        NativeValueKind::Record(_) => 4,
        NativeValueKind::Enum(_) => 5,
        NativeValueKind::F64 => 6,
        NativeValueKind::TextBuilder => 7,
        NativeValueKind::List(_) => 8,
        NativeValueKind::TextIndex => 9,
    }
}

#[cfg(test)]
mod tests {
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    use super::runtime_metadata_source;
    use sarif_codegen::lower;

    #[test]
    fn emits_payload_enum_metadata_for_native_runtime() {
        let lexed = lex(
            "enum OptionText { none, some(Text) }\nfn main() -> OptionText { OptionText.some(\"hello\") }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let lowered = lower(&hir.module);
        let metadata = runtime_metadata_source(&lowered.program).expect("metadata should lower");

        assert!(metadata.contains("typedef struct SarifVariantDesc SarifVariantDesc;"));
        assert!(metadata.contains("{ \"some\", 3, 0, 0 },"));
        assert!(metadata.contains("const struct SarifEnumDesc* sarif_get_main_enum_desc(void)"));
    }
}
