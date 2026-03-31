use std::collections::BTreeMap;
use std::fmt::Write;

use sarif_frontend::semantic::{Analysis, ItemReport};
use sarif_syntax::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownDoc {
    pub sections: Vec<MarkdownSection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownSection {
    pub heading: Option<String>,
    pub items: Vec<MarkdownItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownItem {
    Const {
        name: String,
        ty: String,
        value: Option<String>,
    },
    Function {
        name: String,
        signature: String,
        ownership: String,
        rt_status: String,
    },
    Enum {
        name: String,
        variants: usize,
        ownership: String,
        rt_status: String,
    },
    Struct {
        name: String,
        ownership: String,
        rt_status: String,
    },
}

#[must_use]
pub fn render_markdown(analysis: &Analysis) -> String {
    render_markdown_with_const_values(analysis, &BTreeMap::new())
}

#[must_use]
pub fn render_markdown_with_const_values(
    analysis: &Analysis,
    const_values: &BTreeMap<String, String>,
) -> String {
    render_markdown_doc(&build_markdown_doc_with_const_values(
        analysis,
        const_values,
    ))
}

#[must_use]
pub fn render_markdown_package_with_const_values(
    analysis: &Analysis,
    const_values: &BTreeMap<String, String>,
    sections: &[(String, Span)],
) -> String {
    render_markdown_doc(&build_markdown_package_with_const_values(
        analysis,
        const_values,
        sections,
    ))
}

#[must_use]
pub fn build_markdown_doc_with_const_values(
    analysis: &Analysis,
    const_values: &BTreeMap<String, String>,
) -> MarkdownDoc {
    MarkdownDoc {
        sections: vec![MarkdownSection {
            heading: None,
            items: build_markdown_items(&analysis.reports, const_values),
        }],
    }
}

#[must_use]
pub fn build_markdown_package_with_const_values(
    analysis: &Analysis,
    const_values: &BTreeMap<String, String>,
    sections: &[(String, Span)],
) -> MarkdownDoc {
    let mut rendered_sections = Vec::new();
    let mut emitted = false;
    for (path, span) in sections {
        let reports = analysis
            .reports
            .iter()
            .filter(|report| {
                let report_span = report.span();
                report_span.start >= span.start && report_span.end <= span.end
            })
            .collect::<Vec<_>>();
        if reports.is_empty() {
            continue;
        }
        emitted = true;
        rendered_sections.push(MarkdownSection {
            heading: Some(path.clone()),
            items: build_markdown_items(reports, const_values),
        });
    }

    if emitted {
        MarkdownDoc {
            sections: rendered_sections,
        }
    } else {
        build_markdown_doc_with_const_values(analysis, const_values)
    }
}

#[must_use]
pub fn render_markdown_doc(document: &MarkdownDoc) -> String {
    let mut output = String::new();
    writeln!(&mut output, "# Sarif Semantic Docs\n\n").expect("writing to a string cannot fail");

    let multiple_sections = document.sections.len() > 1
        || document
            .sections
            .first()
            .is_some_and(|section| section.heading.is_some());
    for section in &document.sections {
        if let Some(heading) = &section.heading {
            writeln!(&mut output, "## {heading}\n").expect("writing to a string cannot fail");
        }
        let heading_level = if multiple_sections { 3 } else { 2 };
        render_items(&mut output, &section.items, heading_level);
    }

    output
}

fn build_markdown_items<'a>(
    reports: impl IntoIterator<Item = &'a ItemReport>,
    const_values: &BTreeMap<String, String>,
) -> Vec<MarkdownItem> {
    reports
        .into_iter()
        .map(|report| match report {
            ItemReport::Const(const_item) => MarkdownItem::Const {
                name: const_item.name.clone(),
                ty: const_item.ty.render(),
                value: const_values.get(&const_item.name).cloned(),
            },
            ItemReport::Function(function) => MarkdownItem::Function {
                name: function.name.clone(),
                signature: function.signature.clone(),
                ownership: function.ownership_status.clone(),
                rt_status: function.rt_status.clone(),
            },
            ItemReport::Enum(enum_item) => MarkdownItem::Enum {
                name: enum_item.name.clone(),
                variants: enum_item.variant_count,
                ownership: enum_item.ownership_status.clone(),
                rt_status: enum_item.rt_status.clone(),
            },
            ItemReport::Struct(struct_item) => MarkdownItem::Struct {
                name: struct_item.name.clone(),
                ownership: struct_item.ownership_status.clone(),
                rt_status: struct_item.rt_status.clone(),
            },
        })
        .collect()
}

fn render_items(output: &mut String, items: &[MarkdownItem], heading_level: usize) {
    let heading = "#".repeat(heading_level);

    for item in items {
        match item {
            MarkdownItem::Const { name, ty, value } => {
                writeln!(output, "{heading} const {name}\n")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- type: `{ty}`").expect("writing to a string cannot fail");
                if let Some(value) = value {
                    writeln!(output, "- value: `{value}`")
                        .expect("writing to a string cannot fail");
                }
            }
            MarkdownItem::Function {
                name,
                signature,
                ownership,
                rt_status,
            } => {
                writeln!(output, "{heading} fn {name}\n").expect("writing to a string cannot fail");
                writeln!(output, "- signature: `{signature}`")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- ownership: `{ownership}`")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- rt status: `{rt_status}`\n")
                    .expect("writing to a string cannot fail");
            }
            MarkdownItem::Enum {
                name,
                variants,
                ownership,
                rt_status,
            } => {
                writeln!(output, "{heading} enum {name}\n")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- variants: `{variants}`")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- ownership: `{ownership}`")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- rt status: `{rt_status}`\n")
                    .expect("writing to a string cannot fail");
            }
            MarkdownItem::Struct {
                name,
                ownership,
                rt_status,
            } => {
                writeln!(output, "{heading} struct {name}\n")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- ownership: `{ownership}`")
                    .expect("writing to a string cannot fail");
                writeln!(output, "- rt status: `{rt_status}`\n")
                    .expect("writing to a string cannot fail");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sarif_frontend::semantic::{
        Analysis, ConstReport, FunctionReport, ItemReport, StructReport, Type,
    };
    use sarif_syntax::Span;

    use super::{
        MarkdownDoc, MarkdownItem, MarkdownSection, build_markdown_doc_with_const_values,
        build_markdown_package_with_const_values, render_markdown_doc,
    };

    #[test]
    fn render_markdown_doc_renders_single_section_without_package_heading() {
        let document = MarkdownDoc {
            sections: vec![MarkdownSection {
                heading: None,
                items: vec![
                    MarkdownItem::Const {
                        name: "answer".to_owned(),
                        ty: "I32".to_owned(),
                        value: Some("42".to_owned()),
                    },
                    MarkdownItem::Function {
                        name: "main".to_owned(),
                        signature: "fn main() -> I32".to_owned(),
                        ownership: "ok".to_owned(),
                        rt_status: "ok".to_owned(),
                    },
                ],
            }],
        };

        let rendered = render_markdown_doc(&document);

        assert_eq!(
            rendered,
            "# Sarif Semantic Docs\n\n\n## const answer\n\n- type: `I32`\n- value: `42`\n## fn main\n\n- signature: `fn main() -> I32`\n- ownership: `ok`\n- rt status: `ok`\n\n"
        );
    }

    #[test]
    fn render_markdown_doc_renders_package_sections_with_nested_item_headings() {
        let document = MarkdownDoc {
            sections: vec![MarkdownSection {
                heading: Some("src/main.sarif".to_owned()),
                items: vec![MarkdownItem::Struct {
                    name: "Pair".to_owned(),
                    ownership: "ok".to_owned(),
                    rt_status: "ok".to_owned(),
                }],
            }],
        };

        let rendered = render_markdown_doc(&document);

        assert_eq!(
            rendered,
            "# Sarif Semantic Docs\n\n\n## src/main.sarif\n\n### struct Pair\n\n- ownership: `ok`\n- rt status: `ok`\n\n"
        );
    }

    #[test]
    fn build_markdown_doc_with_const_values_keeps_const_values_and_reports() {
        let analysis = Analysis {
            reports: vec![
                ItemReport::Const(ConstReport {
                    name: "answer".to_owned(),
                    ty: Type::I32,
                    span: Span::new(0, 1),
                }),
                ItemReport::Function(FunctionReport {
                    name: "main".to_owned(),
                    signature: "fn main() -> I32".to_owned(),
                    ownership_status: "ok".to_owned(),
                    rt_status: "ok".to_owned(),
                    span: Span::new(2, 3),
                }),
            ],
            diagnostics: Vec::new(),
        };
        let const_values = BTreeMap::from([(String::from("answer"), String::from("42"))]);

        let document = build_markdown_doc_with_const_values(&analysis, &const_values);

        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.sections[0].heading, None);
        assert_eq!(
            document.sections[0].items,
            vec![
                MarkdownItem::Const {
                    name: "answer".to_owned(),
                    ty: "I32".to_owned(),
                    value: Some("42".to_owned()),
                },
                MarkdownItem::Function {
                    name: "main".to_owned(),
                    signature: "fn main() -> I32".to_owned(),
                    ownership: "ok".to_owned(),
                    rt_status: "ok".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn build_markdown_package_with_const_values_groups_reports_by_section() {
        let analysis = Analysis {
            reports: vec![
                ItemReport::Const(ConstReport {
                    name: "answer".to_owned(),
                    ty: Type::I32,
                    span: Span::new(0, 4),
                }),
                ItemReport::Struct(StructReport {
                    name: "Pair".to_owned(),
                    ownership_status: "ok".to_owned(),
                    rt_status: "ok".to_owned(),
                    span: Span::new(10, 15),
                }),
            ],
            diagnostics: Vec::new(),
        };
        let sections = vec![
            ("src/consts.sarif".to_owned(), Span::new(0, 5)),
            ("src/types.sarif".to_owned(), Span::new(10, 16)),
        ];

        let document =
            build_markdown_package_with_const_values(&analysis, &BTreeMap::new(), &sections);

        assert_eq!(document.sections.len(), 2);
        assert_eq!(
            document.sections[0].heading.as_deref(),
            Some("src/consts.sarif")
        );
        assert_eq!(
            document.sections[1].heading.as_deref(),
            Some("src/types.sarif")
        );
        assert_eq!(
            document.sections[0].items,
            vec![MarkdownItem::Const {
                name: "answer".to_owned(),
                ty: "I32".to_owned(),
                value: None,
            }]
        );
        assert_eq!(
            document.sections[1].items,
            vec![MarkdownItem::Struct {
                name: "Pair".to_owned(),
                ownership: "ok".to_owned(),
                rt_status: "ok".to_owned(),
            }]
        );
    }
}
