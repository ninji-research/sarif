use std::collections::BTreeMap;

#[cfg(all(test, feature = "codegen"))]
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

#[cfg(all(test, feature = "codegen"))]
static BOOTSTRAP_FORMAT_PROGRAM: std::sync::OnceLock<Result<Program, String>> =
    std::sync::OnceLock::new();

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
    target.ensure_no_diagnostics(&target.blocking_diagnostics(&diags), "doc generation failed")?;

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
    let blocking_diags = target.blocking_diagnostics(&all_diags);
    target.ensure_no_diagnostics(&blocking_diags, "check failed")?;
    Ok(render_semantic_check_output(&semantic_snapshot(profile)))
}

#[cfg(all(test, feature = "codegen"))]
pub fn render_bootstrap_format(target: &LoadedSource) -> Result<String, String> {
    target.ensure_no_diagnostics(&target.ast_diagnostics(), "bootstrap format failed")?;
    let program = bootstrap_format_program()?;
    let mut output = String::new();
    for segment in &target.segments {
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

#[cfg(all(test, feature = "codegen"))]
fn bootstrap_format_program() -> Result<&'static Program, String> {
    let cached = BOOTSTRAP_FORMAT_PROGRAM.get_or_init(|| {
        load_bootstrap_tools().and_then(|tools| {
            tools
                .lower_program(Profile::Core, "bootstrap format failed")
                .cloned()
        })
    });
    cached.as_ref().map_err(Clone::clone)
}

#[cfg(all(test, feature = "codegen"))]
fn load_bootstrap_tools() -> Result<LoadedSource, String> {
    let manifest_path = format!(
        "{}/../../bootstrap/sarif_tools/Sarif.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    LoadedSource::load(&manifest_path)
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
    target.semantic_diagnostics(profile)
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
