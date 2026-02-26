use vector_store::SearchHit;

/// Sync trait for compressing search results into a context string within a token budget.
pub trait ContextCompressor: Send + Sync {
    /// Compress search hits into a single context string.
    ///
    /// `token_budget` is approximate: 1 token ≈ 4 characters.
    fn compress(&self, hits: &[SearchHit], token_budget: usize) -> String;
}

/// Compresses by iterating hits in order, adding citation headers, and truncating at budget.
pub struct TruncatingCompressor;

impl ContextCompressor for TruncatingCompressor {
    fn compress(&self, hits: &[SearchHit], token_budget: usize) -> String {
        if hits.is_empty() {
            return String::new();
        }

        let char_budget = token_budget * 4;
        let mut result = String::new();

        for hit in hits {
            let header = format!("[chunk:{}]\n", hit.id);
            let chunk = format!("{}{}\n\n", header, hit.text);

            if result.len() + chunk.len() <= char_budget {
                result.push_str(&chunk);
            } else {
                // Add as much as fits
                let remaining = char_budget.saturating_sub(result.len());
                if remaining > header.len() {
                    result.push_str(&chunk[..remaining]);
                }
                break;
            }
        }

        result.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_hit(id: &str, text: &str) -> SearchHit {
        SearchHit {
            id: id.into(),
            score: 0.9,
            text: text.into(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn empty_input() {
        let c = TruncatingCompressor;
        assert_eq!(c.compress(&[], 100), "");
    }

    #[test]
    fn within_budget() {
        let c = TruncatingCompressor;
        let hits = vec![make_hit("a", "hello"), make_hit("b", "world")];
        let result = c.compress(&hits, 100);
        assert!(result.contains("[chunk:a]"));
        assert!(result.contains("[chunk:b]"));
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[test]
    fn exceeding_budget_truncation() {
        let c = TruncatingCompressor;
        // Create hits that exceed a small budget
        let hits = vec![
            make_hit("a", &"x".repeat(100)),
            make_hit("b", &"y".repeat(100)),
        ];
        // Budget of 10 tokens = 40 chars — should not include all content
        let result = c.compress(&hits, 10);
        assert!(result.len() <= 40);
    }

    #[test]
    fn citation_headers_present() {
        let c = TruncatingCompressor;
        let hits = vec![make_hit("chunk-42", "some text")];
        let result = c.compress(&hits, 100);
        assert!(result.contains("[chunk:chunk-42]"));
    }
}
