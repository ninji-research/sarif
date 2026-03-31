use ariadne::{Color, Label, Report, ReportKind, Source};
use sarif_syntax::Diagnostic;

/// Render diagnostics for one source file into an ANSI-colored string.
///
/// # Panics
///
/// Panics only if writing to the in-memory buffer fails or if the output is
/// not valid UTF-8, both of which should be unreachable here.
#[must_use]
pub fn render_diagnostics(file_name: &str, source: &str, diagnostics: &[Diagnostic]) -> String {
    let mut output = Vec::new();

    for diagnostic in diagnostics {
        let mut report = Report::build(
            ReportKind::Error,
            (file_name, diagnostic.span.start..diagnostic.span.end),
        )
        .with_code(diagnostic.code)
        .with_message(diagnostic.message.clone())
        .with_label(
            Label::new((file_name, diagnostic.span.start..diagnostic.span.end))
                .with_message(diagnostic.message.clone())
                .with_color(Color::Red),
        );

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
