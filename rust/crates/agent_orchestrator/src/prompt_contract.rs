use crate::types::{Citation, ToolOutput};

/// Validates that the tool output contains at least one citation.
///
/// Per the prompt contract (SPECIFICATIONS.md §17), responses must be grounded
/// in retrieved data. An output with content but no citations violates this.
pub fn validate_citations(output: &ToolOutput) -> bool {
    output.content.is_empty() || !output.citations.is_empty()
}

/// Assembles a grounded prompt for the LLM, injecting context and citation instructions.
pub fn assemble_prompt(query: &str, context: &str, citations: &[Citation]) -> String {
    let mut prompt = String::new();

    if !context.is_empty() {
        prompt.push_str("## Retrieved Context\n\n");
        prompt.push_str(context);
        prompt.push_str("\n\n");
    }

    if !citations.is_empty() {
        prompt.push_str("## Source Citations\n\n");
        for citation in citations {
            match citation {
                Citation::Iri(iri) => prompt.push_str(&format!("- [iri:{iri}]\n")),
                Citation::ChunkId(id) => prompt.push_str(&format!("- [chunk:{id}]\n")),
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Answer the question below using ONLY the retrieved context above.\n\
         Cite your sources using [iri:...] for RDF IRIs and [chunk:...] for chunk IDs.\n\
         If the context does not contain enough information, say so explicitly.\n\n",
    );

    prompt.push_str("## Question\n\n");
    prompt.push_str(query);

    prompt
}

/// Extracts citation markers from response text.
///
/// Scans for `[iri:...]` and `[chunk:...]` patterns.
pub fn extract_citations(text: &str) -> Vec<Citation> {
    crate::codegen_tool::extract_citations_from_text(text)
}

/// Merges citations from multiple tool outputs, deduplicating.
pub fn merge_citations(outputs: &[ToolOutput]) -> Vec<Citation> {
    let mut merged = Vec::new();
    for output in outputs {
        for citation in &output.citations {
            if !merged.contains(citation) {
                merged.push(citation.clone());
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_with_citations() {
        let output = ToolOutput {
            content: "Alice is a person.".into(),
            citations: vec![Citation::Iri("http://example.org/alice".into())],
        };
        assert!(validate_citations(&output));
    }

    #[test]
    fn invalid_without_citations() {
        let output = ToolOutput {
            content: "Some claim without evidence.".into(),
            citations: vec![],
        };
        assert!(!validate_citations(&output));
    }

    #[test]
    fn empty_content_is_valid() {
        let output = ToolOutput {
            content: String::new(),
            citations: vec![],
        };
        assert!(validate_citations(&output));
    }

    #[test]
    fn prompt_assembly_with_context() {
        let citations = vec![
            Citation::Iri("http://example.org/alice".into()),
            Citation::ChunkId("chunk1".into()),
        ];
        let prompt = assemble_prompt("Who is Alice?", "Alice is a person.", &citations);
        assert!(prompt.contains("## Retrieved Context"));
        assert!(prompt.contains("Alice is a person."));
        assert!(prompt.contains("[iri:http://example.org/alice]"));
        assert!(prompt.contains("[chunk:chunk1]"));
        assert!(prompt.contains("## Question"));
        assert!(prompt.contains("Who is Alice?"));
    }

    #[test]
    fn prompt_assembly_without_context() {
        let prompt = assemble_prompt("What is RDF?", "", &[]);
        assert!(!prompt.contains("## Retrieved Context"));
        assert!(!prompt.contains("## Source Citations"));
        assert!(prompt.contains("## Question"));
        assert!(prompt.contains("What is RDF?"));
    }

    #[test]
    fn extract_citations_roundtrip() {
        let citations = vec![
            Citation::Iri("http://example.org/x".into()),
            Citation::ChunkId("abc".into()),
        ];
        let prompt = assemble_prompt("test", "ctx", &citations);
        let extracted = extract_citations(&prompt);
        // The assembled prompt contains the cited sources plus instruction examples
        assert!(extracted.contains(&Citation::Iri("http://example.org/x".into())));
        assert!(extracted.contains(&Citation::ChunkId("abc".into())));
    }

    #[test]
    fn merge_deduplicates() {
        let outputs = vec![
            ToolOutput {
                content: "a".into(),
                citations: vec![
                    Citation::Iri("http://example.org/x".into()),
                    Citation::ChunkId("c1".into()),
                ],
            },
            ToolOutput {
                content: "b".into(),
                citations: vec![
                    Citation::Iri("http://example.org/x".into()),
                    Citation::ChunkId("c2".into()),
                ],
            },
        ];
        let merged = merge_citations(&outputs);
        assert_eq!(merged.len(), 3); // x, c1, c2
    }
}
