use std::collections::BTreeMap;

#[cfg(feature = "codegen")]
use sarif_codegen::{Program, RuntimeError, RuntimeValue, run_function_native as run_function};
use sarif_frontend::diagnostics::render_diagnostics;
use sarif_frontend::semantic::Profile;
use sarif_syntax::{Diagnostic, Span};
use sarif_tools::report::{
    render_semantic_doc as render_semantic_doc_output, semantic_package_snapshot_from_analysis,
    semantic_snapshot_from_analysis,
};

use crate::{LoadedSource, PackageSegment};

pub fn render_semantic_doc(target: &LoadedSource, profile: Profile) -> Result<String, String> {
    let diags = semantic_doc_diagnostics(target, profile);
    target.ensure_no_diagnostics(
        &LoadedSource::blocking_diagnostics(&diags, profile),
        "doc generation failed",
    )?;

    let analysis = target.database.semantic(target.source_id, profile);
    let const_values = semantic_const_values(target);
    let rendered = if target.segments.len() > 1 {
        let sections = target
            .segments
            .iter()
            .map(|segment| (segment.path.clone(), segment.combined_span))
            .collect::<Vec<_>>();
        let snapshot =
            semantic_package_snapshot_from_analysis(profile, &analysis, &const_values, &sections);
        render_semantic_doc_output(&snapshot)
    } else {
        let snapshot = semantic_snapshot_from_analysis(profile, &analysis, &const_values);
        render_semantic_doc_output(&snapshot)
    };
    Ok(rendered)
}

#[cfg(feature = "codegen")]
pub fn render_bootstrap_format(loaded: &LoadedSource) -> Result<String, String> {
    loaded.ensure_no_diagnostics(&loaded.ast_diagnostics(), "bootstrap format failed")?;
    let program = bootstrap_format_program()?;
    let mut output = String::new();
    for segment in &loaded.segments {
        let formatted = run_function(
            program,
            "format_text",
            &[RuntimeValue::Text(segment.source.clone())],
        )
        .map_err(|error| {
            let message = match error {
                RuntimeError::Message(m) => m,
                RuntimeError::EffectUnwind {
                    effect, operation, ..
                } => format!("unhandled effect {effect}.{operation}"),
            };
            format!("runtime error: {message}")
        })?;
        let formatted = match formatted {
            RuntimeValue::Text(text) => text,
            other => {
                return Err(format!(
                    "bootstrap formatter must return Text, found {}",
                    other.render()
                ));
            }
        };
        append_formatted_segment(&mut output, &formatted);
    }
    Ok(output)
}

#[cfg(feature = "codegen")]
fn bootstrap_tools_program() -> Result<&'static Program, String> {
    static BOOTSTRAP_PROGRAM: std::sync::OnceLock<Result<Program, String>> =
        std::sync::OnceLock::new();
    let cached = BOOTSTRAP_PROGRAM.get_or_init(|| {
        let manifest_path = format!(
            "{}/../../bootstrap/sarif_tools/Sarif.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let loaded = LoadedSource::load(&manifest_path)?;
        let diags = loaded.mir_diagnostics(Profile::Core);
        loaded.ensure_no_diagnostics(
            &LoadedSource::blocking_diagnostics(&diags, Profile::Core),
            "bootstrap tools failed",
        )?;
        Ok(loaded.mir().program.clone())
    });
    cached.as_ref().map_err(Clone::clone)
}

#[cfg(feature = "codegen")]
fn bootstrap_format_program() -> Result<&'static Program, String> {
    bootstrap_tools_program()
}

#[cfg(feature = "codegen")]
fn collect_package_names(loaded: &LoadedSource) -> String {
    use sarif_syntax::ast::Item;
    let ast = loaded.database.ast(loaded.source_id);
    let mut names: Vec<&str> = Vec::new();
    for item in &ast.file.items {
        match item {
            Item::Function(f) => names.push(&f.name),
            Item::Struct(s) => names.push(&s.name),
            Item::Enum(e) => names.push(&e.name),
            Item::Const(c) => names.push(&c.name),
            Item::Effect(e) => names.push(&e.name),
            Item::ExternBlock(b) => {
                for f in &b.functions {
                    names.push(&f.name);
                }
            }
            Item::Import(_) => {}
        }
    }
    names.join("\n")
}
pub fn render_bootstrap_check(loaded: &LoadedSource) -> Result<String, String> {
    loaded.ensure_no_diagnostics(&loaded.ast_diagnostics(), "bootstrap check failed")?;
    eprintln!("DEBUG: Loading bootstrap tools program...");
    let program = bootstrap_tools_program()?;
    eprintln!("DEBUG: Loaded bootstrap tools program.");
    let package_names = collect_package_names(loaded);
    let mut accumulated_fn_sigs = String::new();
    for segment in &loaded.segments {
        eprintln!("DEBUG: Collecting fn sigs for {}...", segment.path);
        let sigs_output = run_function(
            program,
            "collect_fn_sigs_text",
            &[
                RuntimeValue::Text(segment.source.clone()),
                RuntimeValue::Text(accumulated_fn_sigs.clone()),
            ],
        )
        .map_err(|error| {
            let message = match error {
                RuntimeError::Message(m) => m,
                RuntimeError::EffectUnwind {
                    effect, operation, ..
                } => format!("unhandled effect {effect}.{operation}"),
            };
            format!("runtime error collecting fn sigs: {message}")
        })?;
        accumulated_fn_sigs = match sigs_output {
            RuntimeValue::Text(text) => text,
            other => {
                return Err(format!(
                    "collect_fn_sigs_text must return Text, found {}",
                    other.render()
                ));
            }
        };
    }
    let mut all_diagnostics = String::new();
    for segment in &loaded.segments {
        eprintln!("DEBUG: Checking segment {}...", segment.path);
        let check_output = run_function(
            program,
            "check_text",
            &[
                RuntimeValue::Text(segment.source.clone()),
                RuntimeValue::Text(package_names.clone()),
                RuntimeValue::Text(accumulated_fn_sigs.clone()),
            ],
        )
        .map_err(|error| {
            let message = match error {
                RuntimeError::Message(m) => m,
                RuntimeError::EffectUnwind {
                    effect, operation, ..
                } => format!("unhandled effect {effect}.{operation}"),
            };
            format!("runtime error: {message}")
        })?;
        let check_output = match check_output {
            RuntimeValue::Text(text) => text,
            other => {
                return Err(format!(
                    "bootstrap checker must return Text, found {}",
                    other.render()
                ));
            }
        };
        if check_output != "ok [core]\n" {
            all_diagnostics.push_str(&check_output);
        }
    }
    if all_diagnostics.is_empty() {
        Ok("ok [core]\n".to_owned())
    } else {
        Err(all_diagnostics)
    }
}
pub fn render_bootstrap_doc(loaded: &LoadedSource) -> Result<String, String> {
    loaded.ensure_no_diagnostics(
        &LoadedSource::blocking_diagnostics(
            &loaded.semantic_diagnostics(Profile::Core),
            Profile::Core,
        ),
        "doc generation failed",
    )?;
    let program = bootstrap_tools_program()?;
    let segments: Vec<(String, String)> = loaded
        .segments
        .iter()
        .map(|s| (s.path.clone(), s.source.clone()))
        .collect();
    let segment_count = segments.len();
    let segments_for_thread = segments.clone();
    #[allow(clippy::items_after_statements)]
    const STACK_SIZE: usize = 256 * 1024 * 1024;
    let result: Result<Vec<String>, String> = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let mut outputs = Vec::with_capacity(segment_count);
            for (_, source) in segments_for_thread {
                let doc_output =
                    run_function(program, "doc_text", &[RuntimeValue::Text(source.clone())])
                        .map_err(|error| {
                            let message = match error {
                                RuntimeError::Message(m) => m,
                                RuntimeError::EffectUnwind {
                                    effect, operation, ..
                                } => format!("unhandled effect {effect}.{operation}"),
                            };
                            format!("runtime error: {message}")
                        })?;
                let text = match doc_output {
                    RuntimeValue::Text(text) => text,
                    other => {
                        return Err(format!(
                            "bootstrap doc generator must return Text, found {}",
                            other.render()
                        ));
                    }
                };
                outputs.push(text);
            }
            Ok(outputs)
        })
        .map_err(|e| format!("failed to spawn doc thread: {e}"))?
        .join()
        .map_err(|_| "bootstrap doc thread panicked".to_owned())?;
    let mut output = String::new();
    if segment_count > 1 {
        output.push_str("# Sarif Semantic Docs\n\n\n");
    }
    for (i, text) in result?.iter().enumerate() {
        if segment_count > 1 {
            let path = &segments[i].0;
            output.push_str("## ");
            output.push_str(path);
            output.push_str("\n\n");
            let indented = text
                .replace("# Sarif Semantic Docs\n\n\n", "")
                .replace("## ", "### ");
            output.push_str(&indented);
        } else {
            append_formatted_segment(&mut output, text);
        }
    }
    Ok(output)
}

pub fn render_package_diagnostics(
    display_path: &str,
    source: &str,
    segments: &[PackageSegment],
    diagnostics: &[Diagnostic],
) -> String {
    let mut rendered = String::new();
    for diagnostic in diagnostics {
        rendered.push_str(&render_segment_diagnostic(
            display_path,
            source,
            segments,
            diagnostic,
        ));
    }
    rendered
}

fn render_segment_diagnostic(
    display_path: &str,
    source: &str,
    segments: &[PackageSegment],
    diagnostic: &Diagnostic,
) -> String {
    if let Some((segment, span)) = map_diagnostic_to_segment(segments, diagnostic.span) {
        let mapped = Diagnostic::new(
            diagnostic.code,
            diagnostic.message.clone(),
            span,
            diagnostic.help.clone(),
        );
        render_diagnostics(&segment.path, &segment.source, &[mapped])
    } else {
        render_diagnostics(display_path, source, std::slice::from_ref(diagnostic))
    }
}

fn map_diagnostic_to_segment(
    segments: &[PackageSegment],
    span: Span,
) -> Option<(&PackageSegment, Span)> {
    for segment in segments {
        if span.start >= segment.combined_span.start && span.end <= segment.combined_span.end {
            return Some((
                segment,
                Span::new(
                    span.start - segment.combined_span.start,
                    span.end - segment.combined_span.start,
                ),
            ));
        }
    }
    None
}

fn append_formatted_segment(output: &mut String, formatted: &str) {
    if !output.is_empty() && !output.ends_with("\n\n") {
        if output.ends_with('\n') {
            output.push('\n');
        } else {
            output.push_str("\n\n");
        }
    }
    output.push_str(formatted);
}

#[cfg(feature = "codegen")]
fn semantic_doc_diagnostics(target: &LoadedSource, profile: Profile) -> Vec<Diagnostic> {
    target.mir_diagnostics(profile)
}

#[cfg(not(feature = "codegen"))]
fn semantic_doc_diagnostics(target: &LoadedSource, profile: Profile) -> Vec<Diagnostic> {
    target.semantic_diagnostics(profile)
}

#[cfg(feature = "codegen")]
fn semantic_const_values(target: &LoadedSource) -> BTreeMap<String, String> {
    target
        .mir()
        .const_values
        .iter()
        .map(|(name, value)| (name.clone(), value.render()))
        .collect()
}

#[cfg(not(feature = "codegen"))]
fn semantic_const_values(_target: &LoadedSource) -> BTreeMap<String, String> {
    BTreeMap::new()
}

pub fn render_sarif_json(
    target: &LoadedSource,
    diagnostics: &[Diagnostic],
    profile: Profile,
) -> String {
    let mut results = Vec::new();
    for diagnostic in diagnostics {
        let is_alloc_escape_warning = diagnostic.code == "semantic.alloc-escape";
        let level = if is_alloc_escape_warning && profile != Profile::Rt {
            "warning"
        } else {
            "error"
        };

        let (file_path, rel_span, source_text) = if let Some((segment, span)) =
            map_diagnostic_to_segment(&target.segments, diagnostic.span)
        {
            (&segment.path, span, &segment.source)
        } else {
            (&target.path, diagnostic.span, &target.source)
        };

        let (start_line, start_col) = get_line_col(source_text, rel_span.start);
        let (end_line, end_col) = get_line_col(source_text, rel_span.end.max(rel_span.start));

        let help_text = diagnostic.help.as_ref().map_or_else(String::new, |help| {
            format!("\\n\\nHelp: {}", escape_json(help))
        });
        let full_message = format!("{}{}", escape_json(&diagnostic.message), help_text);

        let result_json = format!(
            r#"        {{
          "ruleId": "{}",
          "level": "{}",
          "message": {{
            "text": "{}"
          }},
          "locations": [
            {{
              "physicalLocation": {{
                "artifactLocation": {{
                  "uri": "{}"
                }},
                "region": {{
                  "startLine": {},
                  "startColumn": {},
                  "endLine": {},
                  "endColumn": {}
                }}
              }}
            }}
          ]
        }}"#,
            escape_json(diagnostic.code),
            level,
            full_message,
            escape_json(file_path),
            start_line,
            start_col,
            end_line,
            end_col
        );
        results.push(result_json);
    }

    format!(
        r#"{{
  "$schema": "https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0-rtm.5.json",
  "version": "2.1.0",
  "runs": [
    {{
      "tool": {{
        "driver": {{
          "name": "sarifc",
          "rules": []
        }}
      }},
      "results": [
{}
      ]
    }}
  ]
}}
"#,
        results.join(",\n")
    )
}

fn get_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn escape_json(s: &str) -> String {
    let mut escaped = String::new();
    for c in s.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}
