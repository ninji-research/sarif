use ariadne::{Color, Label, Report, ReportKind, Source};
use sarif_syntax::Diagnostic;

/// Render diagnostics for one source file into an ANSI-colored string.
#[must_use]
pub fn render_diagnostics(file_name: &str, source: &str, diagnostics: &[Diagnostic]) -> String {
    let mut output = Vec::new();

    for diagnostic in diagnostics {
        let is_alloc_escape_warning = diagnostic.code == "semantic.alloc-escape";
        let is_builtin_shadow = diagnostic.code == "semantic.builtin-shadow";
        let report_kind = if is_alloc_escape_warning || is_builtin_shadow {
            ReportKind::Warning
        } else {
            ReportKind::Error
        };
        let span_start = diagnostic.span.start;
        let span_end = diagnostic.span.end.max(span_start);
        let mut report = Report::build(report_kind, (file_name, span_start..span_end))
            .with_code(diagnostic.code)
            .with_message(diagnostic.message.clone())
            .with_label(
                Label::new((file_name, span_start..span_end))
                    .with_message(diagnostic.message.clone())
                    .with_color(Color::Red),
            );

        if let Some(origin) = diagnostic.origin_span {
            let o_start = origin.start;
            let o_end = origin.end.max(o_start);
            report = report.with_label(
                Label::new((file_name, o_start..o_end))
                    .with_message("first declared here")
                    .with_color(Color::Blue),
            );
        }

        if let Some(help) = &diagnostic.help {
            report = report.with_help(help.clone());
        }

        report
            .finish()
            .write((file_name, Source::from(source)), &mut output)
            .expect("writing diagnostics to a buffer cannot fail");
    }

    String::from_utf8(output).expect("diagnostic output must be valid UTF-8")
}

#[cfg(test)]
mod tests {
    use sarif_syntax::{Diagnostic, Span};

    use crate::diagnostics::render_diagnostics;

    #[test]
    fn renders_a_compact_report() {
        let rendered = render_diagnostics(
            "example.sarif",
            "fn main() {}\n",
            &[Diagnostic::new(
                "parse.expected-token",
                "expected token",
                Span::new(0, 2),
                Some("insert the missing token".to_owned()),
            )],
        );

        assert!(rendered.contains("parse.expected-token"));
        assert!(rendered.contains("insert the missing token"));
    }
}
