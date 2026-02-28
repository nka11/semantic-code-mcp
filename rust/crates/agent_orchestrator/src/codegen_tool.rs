use anyhow::{anyhow, Result};

use crate::prompt_contract::assemble_prompt;
use crate::types::{AgentTool, Citation, ToolInput, ToolOutput};

/// Async trait for LLM text generation.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Generate text from a prompt.
    async fn generate(&self, prompt: &str) -> Result<String>;
}

/// Mock LLM client for testing that echoes the prompt.
pub struct MockLlmClient;

#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String> {
        Ok(format!("[generated] {prompt}"))
    }
}

/// Agent tool that generates text using an LLM with provided context.
pub struct CodegenTool {
    client: Box<dyn LlmClient>,
}

impl CodegenTool {
    pub fn new(client: impl LlmClient + 'static) -> Self {
        Self {
            client: Box::new(client),
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for CodegenTool {
    fn name(&self) -> &str {
        "codegen"
    }

    async fn call(&self, input: ToolInput) -> Result<ToolOutput> {
        let (prompt, context) = match input {
            ToolInput::Codegen { prompt, context } => (prompt, context),
            _ => return Err(anyhow!("CodegenTool expects ToolInput::Codegen")),
        };

        let full_prompt = assemble_prompt(&prompt, &context, &[]);

        let content = self.client.generate(&full_prompt).await?;

        // Extract citations the LLM included in its response
        let citations = extract_citations_from_text(&content);

        Ok(ToolOutput { content, citations })
    }
}

/// Extracts citations from generated text by scanning for `[iri:...]` and `[chunk:...]` markers.
pub fn extract_citations_from_text(text: &str) -> Vec<Citation> {
    let mut citations = Vec::new();
    for cap in text.match_indices("[iri:") {
        if let Some(end) = text[cap.0..].find(']') {
            let iri = &text[cap.0 + 5..cap.0 + end];
            let citation = Citation::Iri(iri.to_string());
            if !citations.contains(&citation) {
                citations.push(citation);
            }
        }
    }
    for cap in text.match_indices("[chunk:") {
        if let Some(end) = text[cap.0..].find(']') {
            let id = &text[cap.0 + 7..cap.0 + end];
            let citation = Citation::ChunkId(id.to_string());
            if !citations.contains(&citation) {
                citations.push(citation);
            }
        }
    }
    citations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn codegen_echoes_prompt() {
        let tool = CodegenTool::new(MockLlmClient);
        let output = tool
            .call(ToolInput::Codegen {
                prompt: "What is RDF?".into(),
                context: String::new(),
            })
            .await
            .unwrap();
        assert!(output.content.contains("What is RDF?"));
    }

    #[tokio::test]
    async fn codegen_includes_context() {
        let tool = CodegenTool::new(MockLlmClient);
        let output = tool
            .call(ToolInput::Codegen {
                prompt: "Summarize".into(),
                context: "RDF is a graph data model.".into(),
            })
            .await
            .unwrap();
        assert!(output.content.contains("RDF is a graph data model."));
        assert!(output.content.contains("Summarize"));
    }

    #[tokio::test]
    async fn wrong_input_type_returns_error() {
        let tool = CodegenTool::new(MockLlmClient);
        let result = tool
            .call(ToolInput::Sparql {
                query: "SELECT".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn extract_iri_citations() {
        let text = "Based on [iri:http://example.org/alice] and [iri:http://example.org/bob], the answer is clear.";
        let citations = extract_citations_from_text(text);
        assert_eq!(citations.len(), 2);
        assert!(citations.contains(&Citation::Iri("http://example.org/alice".into())));
        assert!(citations.contains(&Citation::Iri("http://example.org/bob".into())));
    }

    #[test]
    fn extract_chunk_citations() {
        let text = "See [chunk:abc123] for details. Also [chunk:def456].";
        let citations = extract_citations_from_text(text);
        assert_eq!(citations.len(), 2);
        assert!(citations.contains(&Citation::ChunkId("abc123".into())));
        assert!(citations.contains(&Citation::ChunkId("def456".into())));
    }

    #[test]
    fn extract_mixed_citations() {
        let text = "[iri:http://example.org/x] and [chunk:y] together.";
        let citations = extract_citations_from_text(text);
        assert_eq!(citations.len(), 2);
    }

    #[test]
    fn no_citations_in_plain_text() {
        let citations = extract_citations_from_text("Just plain text.");
        assert!(citations.is_empty());
    }

    #[test]
    fn deduplicates_citations() {
        let text = "[iri:http://example.org/x] and again [iri:http://example.org/x]";
        let citations = extract_citations_from_text(text);
        assert_eq!(citations.len(), 1);
    }
}
