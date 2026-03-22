#[cfg(feature = "format")]
use sarif_frontend::FrontendDatabase;
use sarif_frontend::diagnostics::render_diagnostics;
#[cfg(feature = "codegen")]
use sarif_frontend::semantic::Profile;
use sarif_syntax::{Diagnostic, Span};
#[cfg(feature = "format")]
use sarif_tools::format::format_file;

#[cfg(feature = "codegen")]
use super::LoadedSource;
use super::PackageSegment;
#[cfg(feature = "codegen")]
use super::load_bootstrap_tools;

#[cfg(feature = "codegen")]
pub fn render_bootstrap_format(target: &LoadedSource) -> Result<String, String> {
    target.ensure_no_diagnostics(&target.ast_diagnostics(), "bootstrap format failed")?;
    let tools = load_bootstrap_tools()?;
    let mut output = String::new();
    for segment in &target.segments {
        let formatted = tools.run_text_function(Profile::Core, "format_text", &segment.source)?;
        append_formatted_segment(&mut output, &formatted);
    }
    Ok(output)
}

pub fn render_package_diagnostics(
    display_path: &str,
    source: &str,
    segments: &[PackageSegment],
    diagnostics: &[Diagnostic],
) -> String {
    if segments.len() <= 1 {
        return render_diagnostics(display_path, source, diagnostics);
    }

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

#[cfg(feature = "format")]
pub fn render_format(target: &LoadedSource) -> Result<String, String> {
    if target.segments.len() <= 1 {
        let diagnostics = target.ast_diagnostics();
        if !diagnostics.is_empty() {
            return Err(render_package_diagnostics(
                &target.path,
                &target.source,
                &target.segments,
                &diagnostics,
            ));
        }
        return Ok(format_file(&target.database.ast(target.source_id).file));
    }

    let mut output = String::new();
    let mut rendered_errors = String::new();
    for segment in &target.segments {
        match format_source_segment(segment) {
            Ok(formatted) => append_formatted_segment(&mut output, &formatted),
            Err(diagnostics) => {
                rendered_errors.push_str(&render_diagnostics(
                    &segment.path,
                    &segment.source,
                    &diagnostics,
                ));
            }
        }
    }

    if rendered_errors.is_empty() {
        Ok(output)
    } else {
        Err(rendered_errors)
    }
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
        if span.start >= segment.start && span.end <= segment.end {
            return Some((
                segment,
                Span::new(span.start - segment.start, span.end - segment.start),
            ));
        }
    }
    None
}

#[cfg(feature = "format")]
fn append_formatted_segment(output: &mut String, formatted: &str) {
    if output.is_empty() || formatted.is_empty() {
        output.push_str(formatted);
        return;
    }

    if !output.ends_with('\n') {
        output.push('\n');
    }
    if !output.ends_with("\n\n") {
        output.push('\n');
    }
    output.push_str(formatted);
}

#[cfg(feature = "format")]
fn format_source_segment(segment: &PackageSegment) -> Result<String, Vec<Diagnostic>> {
    let mut database = FrontendDatabase::new();
    let source_id = database.add_source(segment.path.clone(), segment.source.clone());
    let mut diagnostics = database.lex(source_id).diagnostics;
    diagnostics.extend(database.parse(source_id).diagnostics);
    diagnostics.extend(database.ast(source_id).diagnostics);
    if diagnostics.is_empty() {
        Ok(format_file(&database.ast(source_id).file))
    } else {
        Err(diagnostics)
    }
}
