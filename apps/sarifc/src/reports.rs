use std::collections::BTreeMap;

#[cfg(feature = "codegen")]
use sarif_codegen::{Program, RuntimeError, RuntimeValue, run_function};
use sarif_frontend::diagnostics::render_diagnostics;
#[cfg(feature = "codegen")]
use sarif_frontend::semantic::Profile;
use sarif_syntax::ast::lower as lower_ast;
use sarif_syntax::lexer::lex;
use sarif_syntax::parser::parse;
use sarif_syntax::{Diagnostic, Span};
use sarif_tools::format::format_file;
use sarif_tools::report::{
    render_semantic_check as render_semantic_check_output,
    render_semantic_doc as render_semantic_doc_output, semantic_package_snapshot_from_analysis,
    semantic_snapshot, semantic_snapshot_from_analysis,
};

use crate::{LoadedSource, PackageSegment};

pub fn render_semantic_format(target: &LoadedSource) -> Result<String, String> {
    let mut output = String::new();
    for segment in &target.segments {
        let formatted = format_segment(segment)?;
        append_formatted_segment(&mut output, &formatted);
    }
    Ok(output)
}

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

pub fn render_semantic_check(target: &LoadedSource, profile: Profile) -> Result<String, String> {
    let all_diags = semantic_check_diagnostics(target, profile);
    let blocking_diags = LoadedSource::blocking_diagnostics(&all_diags, profile);
    target.ensure_no_diagnostics(&blocking_diags, "check failed")?;
    Ok(render_semantic_check_output(&semantic_snapshot(profile)))
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
        }
    }
    names.join("\n")
}

pub fn render_bootstrap_check(loaded: &LoadedSource) -> Result<String, String> {
    loaded.ensure_no_diagnostics(&loaded.ast_diagnostics(), "bootstrap check failed")?;
    let program = bootstrap_tools_program()?;
    let package_names = collect_package_names(loaded);
    let mut all_diagnostics = String::new();
    for segment in &loaded.segments {
        let check_output = run_function(
            program,
            "check_text",
            &[
                RuntimeValue::Text(segment.source.clone()),
                RuntimeValue::Text(package_names.clone()),
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
    loaded.ensure_no_diagnostics(&loaded.ast_diagnostics(), "bootstrap doc failed")?;
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

fn format_segment(segment: &PackageSegment) -> Result<String, String> {
    let lexed = lex(&segment.source);
    if !lexed.diagnostics.is_empty() {
        return Err(render_segment_failure(
            segment,
            &lexed.diagnostics,
            "format failed",
        ));
    }

    let parsed = parse(&lexed.tokens);
    if !parsed.diagnostics.is_empty() {
        return Err(render_segment_failure(
            segment,
            &parsed.diagnostics,
            "format failed",
        ));
    }

    let lowered = lower_ast(&parsed.root);
    if !lowered.diagnostics.is_empty() {
        return Err(render_segment_failure(
            segment,
            &lowered.diagnostics,
            "format failed",
        ));
    }

    Ok(format_file(&lowered.file))
}

fn render_segment_failure(
    segment: &PackageSegment,
    diagnostics: &[Diagnostic],
    failure: &str,
) -> String {
    eprint!(
        "{}",
        render_diagnostics(&segment.path, &segment.source, diagnostics)
    );
    failure.to_owned()
}

fn semantic_check_diagnostics(target: &LoadedSource, profile: Profile) -> Vec<Diagnostic> {
    target.mir_diagnostics(profile)
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
