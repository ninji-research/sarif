use std::collections::BTreeMap;

use sarif_frontend::semantic::{Analysis, Profile};
use sarif_syntax::Span;

use crate::doc::{
    MarkdownDoc, build_markdown_doc_with_const_values, build_markdown_package_with_const_values,
    render_markdown_doc,
};

#[derive(Clone, Debug)]
pub struct SemanticSnapshot {
    profile: Profile,
    document: MarkdownDoc,
}

impl SemanticSnapshot {
    #[must_use]
    pub const fn profile(&self) -> Profile {
        self.profile
    }
    #[must_use]
    pub const fn document(&self) -> &MarkdownDoc {
        &self.document
    }
}

#[must_use]
pub const fn semantic_snapshot(profile: Profile) -> SemanticSnapshot {
    SemanticSnapshot {
        profile,
        document: MarkdownDoc {
            sections: Vec::new(),
        },
    }
}

#[must_use]
pub fn semantic_snapshot_from_analysis(
    profile: Profile,
    analysis: &Analysis,
    const_values: &BTreeMap<String, String>,
) -> SemanticSnapshot {
    semantic_snapshot_with_docs(
        profile,
        build_markdown_doc_with_const_values(analysis, const_values),
    )
}

#[must_use]
pub fn semantic_package_snapshot_from_analysis(
    profile: Profile,
    analysis: &Analysis,
    const_values: &BTreeMap<String, String>,
    sections: &[(String, Span)],
) -> SemanticSnapshot {
    semantic_snapshot_with_docs(
        profile,
        build_markdown_package_with_const_values(analysis, const_values, sections),
    )
}

#[must_use]
pub fn render_semantic_check(snapshot: &SemanticSnapshot) -> String {
    format!("ok [{}]\n", snapshot.profile().keyword())
}

#[must_use]
pub fn render_semantic_doc(snapshot: &SemanticSnapshot) -> String {
    let mut rendered = render_markdown_doc(snapshot.document());
    rendered.push('\n');
    rendered
}

#[must_use]
pub const fn semantic_snapshot_with_docs(
    profile: Profile,
    document: MarkdownDoc,
) -> SemanticSnapshot {
    SemanticSnapshot { profile, document }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sarif_frontend::semantic::Profile;
    use sarif_syntax::Span;

    use crate::doc::{MarkdownDoc, MarkdownSection};

    use super::{
        render_semantic_check, render_semantic_doc, semantic_package_snapshot_from_analysis,
        semantic_snapshot, semantic_snapshot_from_analysis, semantic_snapshot_with_docs,
    };

    #[test]
    fn render_semantic_check_renders_profile_keyword() {
        let snapshot = semantic_snapshot(Profile::Core);
        assert_eq!(render_semantic_check(&snapshot), "ok [core]\n");
    }

    #[test]
    fn render_semantic_doc_appends_trailing_newline() {
        let snapshot = semantic_snapshot_with_docs(
            Profile::Core,
            MarkdownDoc {
                sections: vec![MarkdownSection {
                    heading: None,
                    items: Vec::new(),
                }],
            },
        );

        assert_eq!(
            render_semantic_doc(&snapshot),
            "# Sarif Semantic Docs\n\n\n\n"
        );
    }

    #[test]
    fn semantic_snapshot_from_analysis_builds_markdown_document() {
        let analysis = sarif_frontend::semantic::Analysis {
            reports: Vec::new(),
            diagnostics: Vec::new(),
        };
        let snapshot = semantic_snapshot_from_analysis(Profile::Core, &analysis, &BTreeMap::new());

        assert_eq!(
            render_semantic_doc(&snapshot),
            "# Sarif Semantic Docs\n\n\n\n"
        );
    }

    #[test]
    fn semantic_package_snapshot_from_analysis_builds_package_document() {
        let analysis = sarif_frontend::semantic::Analysis {
            reports: Vec::new(),
            diagnostics: Vec::new(),
        };
        let snapshot = semantic_package_snapshot_from_analysis(
            Profile::Core,
            &analysis,
            &BTreeMap::new(),
            &[("pkg/src/main.sarif".to_owned(), Span::new(0, 1))],
        );

        assert_eq!(
            render_semantic_doc(&snapshot),
            "# Sarif Semantic Docs\n\n\n\n"
        );
    }
}
