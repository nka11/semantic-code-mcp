use std::collections::HashMap;

use anyhow::{anyhow, Result};

use crate::codegen_tool::extract_citations_from_text;
use crate::prompt_contract::{merge_citations, validate_citations};
use crate::types::{AgentTool, ToolInput, ToolOutput};

/// Agent router that plans and dispatches queries to registered tools.
pub struct AgentRouter {
    tools: HashMap<String, Box<dyn AgentTool>>,
}

impl AgentRouter {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool with the router.
    pub fn register(&mut self, tool: impl AgentTool + 'static) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
    }

    /// Plan which tools to invoke based on query analysis.
    ///
    /// Uses keyword heuristics to determine the appropriate tool chain:
    /// - SPARQL-like patterns (SELECT, ASK, CONSTRUCT, PREFIX) → SparqlTool
    /// - Retrieval keywords (find, search, retrieve, similar, related) → RagTool
    /// - Generation keywords (generate, write, create, explain, summarize) → CodegenTool with RAG context
    pub fn plan(&self, query: &str) -> Vec<ToolInput> {
        let lower = query.to_lowercase();

        // Direct SPARQL query
        if is_sparql_query(&lower) {
            return vec![ToolInput::Sparql {
                query: query.to_string(),
            }];
        }

        // RAG retrieval + codegen for generation tasks
        if is_generation_query(&lower) {
            let mut steps = Vec::new();
            if self.tools.contains_key("rag") {
                steps.push(ToolInput::Rag {
                    query: query.to_string(),
                    top_k: Some(10),
                });
            }
            steps.push(ToolInput::Codegen {
                prompt: query.to_string(),
                context: String::new(), // filled after RAG step
            });
            return steps;
        }

        // RAG retrieval for search queries
        if is_retrieval_query(&lower) {
            return vec![ToolInput::Rag {
                query: query.to_string(),
                top_k: Some(10),
            }];
        }

        // Default: try RAG if available, otherwise codegen
        if self.tools.contains_key("rag") {
            vec![ToolInput::Rag {
                query: query.to_string(),
                top_k: Some(10),
            }]
        } else if self.tools.contains_key("codegen") {
            vec![ToolInput::Codegen {
                prompt: query.to_string(),
                context: String::new(),
            }]
        } else {
            vec![]
        }
    }

    /// Execute a query through the router: plan → execute tools → merge outputs.
    pub async fn execute(&self, query: &str) -> Result<ToolOutput> {
        let plan = self.plan(query);
        if plan.is_empty() {
            return Err(anyhow!("No tools available to handle query"));
        }

        let mut outputs = Vec::new();
        let mut accumulated_context = String::new();

        for input in plan {
            let tool_name = match &input {
                ToolInput::Sparql { .. } => "sparql",
                ToolInput::Rag { .. } => "rag",
                ToolInput::Codegen { .. } => "codegen",
            };

            let tool = self
                .tools
                .get(tool_name)
                .ok_or_else(|| anyhow!("Tool '{tool_name}' not registered"))?;

            // For codegen, inject accumulated context from previous steps
            let input = match input {
                ToolInput::Codegen { prompt, context } if context.is_empty() => {
                    ToolInput::Codegen {
                        prompt,
                        context: accumulated_context.clone(),
                    }
                }
                other => other,
            };

            let output = tool.call(input).await?;

            // Accumulate context for subsequent steps
            if !output.content.is_empty() {
                if !accumulated_context.is_empty() {
                    accumulated_context.push_str("\n\n");
                }
                accumulated_context.push_str(&output.content);
            }

            outputs.push(output);
        }

        // Merge all outputs
        let all_citations = merge_citations(&outputs);
        let combined_content = outputs
            .iter()
            .map(|o| o.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        // If the last step was codegen, extract inline citations from the generated text
        let mut final_citations = all_citations;
        if let Some(last) = outputs.last() {
            let inline = extract_citations_from_text(&last.content);
            for c in inline {
                if !final_citations.contains(&c) {
                    final_citations.push(c);
                }
            }
        }

        let result = ToolOutput {
            content: combined_content,
            citations: final_citations,
        };

        if !validate_citations(&result) {
            tracing::warn!("Response has content but no citations — prompt contract violation");
        }

        Ok(result)
    }
}

impl Default for AgentRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn is_sparql_query(lower: &str) -> bool {
    let trimmed = lower.trim();
    trimmed.starts_with("select ")
        || trimmed.starts_with("ask ")
        || trimmed.starts_with("construct ")
        || trimmed.starts_with("describe ")
        || trimmed.starts_with("prefix ")
}

fn is_generation_query(lower: &str) -> bool {
    let keywords = ["generate", "write", "create", "explain", "summarize"];
    keywords.iter().any(|kw| lower.contains(kw))
}

fn is_retrieval_query(lower: &str) -> bool {
    let keywords = ["find", "search", "retrieve", "similar", "related", "what"];
    keywords.iter().any(|kw| lower.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen_tool::{CodegenTool, MockLlmClient};
    use crate::rag_tool::RagTool;
    use crate::sparql_tool::SparqlTool;
    use crate::types::Citation;
    use rag_pipeline::{EmbeddingProvider, MockEmbeddingProvider, RagPipeline};
    use std::sync::Arc;
    use vector_store::inmemory::InMemoryVectorStore;
    use vector_store::{RagChunk, VectorStore};

    fn make_sparql_router() -> AgentRouter {
        let store = oxigraph::store::Store::new().unwrap();
        store
            .load_from_slice(
                oxigraph::io::RdfParser::from_format(oxigraph::io::RdfFormat::Turtle),
                br#"
                    @prefix ex: <http://example.org/> .
                    ex:alice ex:name "Alice" .
                    ex:bob ex:name "Bob" .
                "#,
            )
            .unwrap();
        let mut router = AgentRouter::new();
        router.register(SparqlTool::new(Arc::new(store)));
        router
    }

    async fn make_full_router() -> AgentRouter {
        let store = oxigraph::store::Store::new().unwrap();
        store
            .load_from_slice(
                oxigraph::io::RdfParser::from_format(oxigraph::io::RdfFormat::Turtle),
                br#"
                    @prefix ex: <http://example.org/> .
                    ex:alice ex:name "Alice" .
                "#,
            )
            .unwrap();

        let embedder = MockEmbeddingProvider::new(8);
        let vecs = embedder.embed(&["Alice is a person".into()]).await.unwrap();
        let vs = InMemoryVectorStore::new();
        vs.upsert(vec![RagChunk {
            id: "chunk1".into(),
            iri: Some("http://example.org/alice".into()),
            text: "Alice is a person".into(),
            graph: None,
            embedding: vecs[0].clone(),
            metadata: std::collections::HashMap::new(),
        }])
        .await
        .unwrap();

        let pipeline = Arc::new(RagPipeline::with_defaults(
            vs,
            MockEmbeddingProvider::new(8),
        ));

        let mut router = AgentRouter::new();
        router.register(SparqlTool::new(Arc::new(store)));
        router.register(RagTool::new(pipeline));
        router.register(CodegenTool::new(MockLlmClient));
        router
    }

    #[test]
    fn plan_sparql_query() {
        let router = make_sparql_router();
        let plan = router.plan("SELECT ?name WHERE { ?s <http://example.org/name> ?name }");
        assert_eq!(plan.len(), 1);
        assert!(matches!(&plan[0], ToolInput::Sparql { .. }));
    }

    #[test]
    fn plan_retrieval_query() {
        let router = make_sparql_router();
        let plan = router.plan("find functions related to authentication");
        assert_eq!(plan.len(), 1);
        assert!(matches!(&plan[0], ToolInput::Rag { .. }));
    }

    #[test]
    fn plan_generation_query() {
        let mut router = make_sparql_router();
        let vs = InMemoryVectorStore::new();
        let pipeline = Arc::new(RagPipeline::with_defaults(
            vs,
            MockEmbeddingProvider::new(8),
        ));
        router.register(RagTool::new(pipeline));
        router.register(CodegenTool::new(MockLlmClient));

        let plan = router.plan("explain the authentication flow");
        assert_eq!(plan.len(), 2); // RAG + Codegen
        assert!(matches!(&plan[0], ToolInput::Rag { .. }));
        assert!(matches!(&plan[1], ToolInput::Codegen { .. }));
    }

    #[tokio::test]
    async fn execute_sparql() {
        let router = make_sparql_router();
        let result = router
            .execute("SELECT ?s ?name WHERE { ?s <http://example.org/name> ?name } ORDER BY ?name")
            .await
            .unwrap();
        assert!(result.content.contains("Alice"));
        assert!(result.content.contains("Bob"));
        assert!(!result.citations.is_empty());
    }

    #[tokio::test]
    async fn execute_rag_retrieval() {
        let router = make_full_router().await;
        let result = router.execute("find Alice").await.unwrap();
        assert!(!result.content.is_empty());
        assert!(result
            .citations
            .iter()
            .any(|c| matches!(c, Citation::ChunkId(id) if id == "chunk1")));
    }

    #[tokio::test]
    async fn execute_generation_with_rag() {
        let router = make_full_router().await;
        let result = router.execute("explain who Alice is").await.unwrap();
        assert!(!result.content.is_empty());
    }

    #[tokio::test]
    async fn empty_router_returns_error() {
        let router = AgentRouter::new();
        let result = router.execute("hello").await;
        assert!(result.is_err());
    }
}
