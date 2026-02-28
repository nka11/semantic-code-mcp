use std::collections::{BTreeMap, HashMap};

/// A raw RDF triple as plain strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTriple {
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

/// Converts raw RDF triples into deterministic text chunks suitable for embedding.
pub struct Canonicalizer {
    /// Prefix → full IRI mapping for CURIE shortening.
    prefixes: Vec<(String, String)>,
}

impl Default for Canonicalizer {
    fn default() -> Self {
        Self {
            prefixes: vec![
                ("code:".into(), "https://ds-labs.org/code#".into()),
                (
                    "rdf:".into(),
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#".into(),
                ),
                (
                    "rdfs:".into(),
                    "http://www.w3.org/2000/01/rdf-schema#".into(),
                ),
                ("xsd:".into(), "http://www.w3.org/2001/XMLSchema#".into()),
            ],
        }
    }
}

impl Canonicalizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a custom prefix mapping.
    pub fn with_prefix(mut self, prefix: &str, iri: &str) -> Self {
        self.prefixes.push((prefix.to_string(), iri.to_string()));
        self
    }

    /// Expand a CURIE like `"code:Function"` into its full IRI.
    /// Returns the input unchanged if no prefix matches.
    pub fn expand_curie(&self, curie: &str) -> String {
        for (prefix, iri) in &self.prefixes {
            if let Some(local) = curie.strip_prefix(prefix.as_str()) {
                return format!("{iri}{local}");
            }
        }
        curie.to_string()
    }

    /// Shorten a full IRI to a CURIE if a matching prefix exists.
    fn shorten_iri(&self, iri: &str) -> String {
        for (prefix, full) in &self.prefixes {
            if let Some(local) = iri.strip_prefix(full.as_str()) {
                return format!("{prefix}{local}");
            }
        }
        iri.to_string()
    }

    /// Group flat triples by subject, preserving predicate order via BTreeMap.
    pub fn group_by_subject(
        &self,
        triples: &[RawTriple],
    ) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
        let mut groups: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for t in triples {
            groups
                .entry(t.subject.clone())
                .or_default()
                .entry(t.predicate.clone())
                .or_default()
                .push(t.object.clone());
        }
        groups
    }

    /// Replace blank node subjects with a stable label derived from
    /// `code:name` or `rdfs:label` properties, falling back to a hash.
    pub fn collapse_blank_nodes(
        &self,
        groups: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    ) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
        let code_name = self.expand_curie("code:name");
        let rdfs_label = self.expand_curie("rdfs:label");

        let mut result = BTreeMap::new();
        // Build a mapping from blank node IDs to stable labels.
        let mut bnode_map: HashMap<String, String> = HashMap::new();

        for (subj, preds) in &groups {
            if subj.starts_with("_:") {
                let label = preds
                    .get(&code_name)
                    .and_then(|v| v.first())
                    .or_else(|| preds.get(&rdfs_label).and_then(|v| v.first()));
                let stable = match label {
                    Some(l) => format!("_:{}", strip_literal(l)),
                    None => {
                        // Deterministic hash from sorted predicates
                        let mut hasher_input = String::new();
                        for (p, objs) in preds {
                            for o in objs {
                                hasher_input.push_str(p);
                                hasher_input.push('\0');
                                hasher_input.push_str(o);
                                hasher_input.push('\0');
                            }
                        }
                        format!("_:h{:016x}", simple_hash(&hasher_input))
                    }
                };
                bnode_map.insert(subj.clone(), stable);
            }
        }

        for (subj, preds) in groups {
            let new_subj = bnode_map.get(&subj).cloned().unwrap_or(subj);
            // Also replace blank node references in objects
            let new_preds = preds
                .into_iter()
                .map(|(p, objs)| {
                    let new_objs = objs
                        .into_iter()
                        .map(|o| bnode_map.get(&o).cloned().unwrap_or(o))
                        .collect();
                    (p, new_objs)
                })
                .collect();
            result.insert(new_subj, new_preds);
        }
        result
    }

    /// Canonicalize a single subject group into deterministic text.
    pub fn canonicalize_group(
        &self,
        subject: &str,
        preds: &BTreeMap<String, Vec<String>>,
    ) -> String {
        let mut lines = vec![self.shorten_iri(subject)];
        // Sort predicates for determinism (BTreeMap already sorted)
        for (pred, objs) in preds {
            let short_pred = self.shorten_iri(pred);
            for obj in objs {
                let short_obj = self.normalize_object(obj);
                lines.push(format!("  {short_pred}: {short_obj}"));
            }
        }
        lines.join("\n")
    }

    /// Normalize an object value: strip `^^xsd:string`, shorten IRIs.
    fn normalize_object(&self, obj: &str) -> String {
        // Strip ^^xsd:string suffix from literals
        let xsd_string_full = "^^http://www.w3.org/2001/XMLSchema#string";
        let xsd_string_curie = "^^xsd:string";
        let stripped = obj
            .strip_suffix(xsd_string_full)
            .or_else(|| obj.strip_suffix(xsd_string_curie))
            .unwrap_or(obj);
        self.shorten_iri(stripped)
    }

    /// Full canonicalization pipeline: group → collapse blank nodes → canonicalize each group.
    /// Returns `Vec<(subject_iri, canonical_text)>`.
    pub fn canonicalize(&self, triples: &[RawTriple]) -> Vec<(String, String)> {
        let groups = self.group_by_subject(triples);
        let groups = self.collapse_blank_nodes(groups);
        groups
            .iter()
            .map(|(subj, preds)| {
                let text = self.canonicalize_group(subj, preds);
                (subj.clone(), text)
            })
            .collect()
    }
}

/// Strip surrounding quotes from a literal value.
fn strip_literal(s: &str) -> &str {
    s.trim_matches('"')
}

/// Simple deterministic hash (FNV-1a style).
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curie_expansion() {
        let c = Canonicalizer::new();
        assert_eq!(
            c.expand_curie("code:Function"),
            "https://ds-labs.org/code#Function"
        );
        assert_eq!(
            c.expand_curie("rdf:type"),
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
        );
        // Unknown prefix returned as-is
        assert_eq!(c.expand_curie("unknown:Foo"), "unknown:Foo");
    }

    #[test]
    fn deterministic_ordering() {
        let c = Canonicalizer::new();
        let triples = vec![
            RawTriple {
                subject: "https://ds-labs.org/code#s1".into(),
                predicate: "https://ds-labs.org/code#name".into(),
                object: "foo".into(),
            },
            RawTriple {
                subject: "https://ds-labs.org/code#s1".into(),
                predicate: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".into(),
                object: "https://ds-labs.org/code#Function".into(),
            },
        ];
        let result = c.canonicalize(&triples);
        assert_eq!(result.len(), 1);
        let (_, text) = &result[0];
        // rdf:type should come before code:name (sorted by full IRI)
        let type_pos = text.find("rdf:type").unwrap();
        let name_pos = text.find("code:name").unwrap();
        assert!(
            type_pos < name_pos,
            "rdf:type should appear before code:name in sorted output"
        );
    }

    #[test]
    fn blank_node_collapsing() {
        let c = Canonicalizer::new();
        let triples = vec![
            RawTriple {
                subject: "_:b0".into(),
                predicate: "https://ds-labs.org/code#name".into(),
                object: "my_func".into(),
            },
            RawTriple {
                subject: "_:b0".into(),
                predicate: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".into(),
                object: "https://ds-labs.org/code#Function".into(),
            },
        ];
        let result = c.canonicalize(&triples);
        assert_eq!(result.len(), 1);
        let (subj, _) = &result[0];
        assert_eq!(subj, "_:my_func");
    }

    #[test]
    fn literal_normalization() {
        let c = Canonicalizer::new();
        let triples = vec![RawTriple {
            subject: "https://ds-labs.org/code#s1".into(),
            predicate: "https://ds-labs.org/code#name".into(),
            object: "\"hello\"^^http://www.w3.org/2001/XMLSchema#string".into(),
        }];
        let result = c.canonicalize(&triples);
        let (_, text) = &result[0];
        assert!(
            text.contains("\"hello\""),
            "should keep quoted value but strip ^^xsd:string"
        );
        assert!(
            !text.contains("XMLSchema#string"),
            "should strip xsd:string suffix"
        );
    }

    #[test]
    fn golden_string() {
        let c = Canonicalizer::new();
        let triples = vec![
            RawTriple {
                subject: "https://ds-labs.org/code#src/main.rs/my_function".into(),
                predicate: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".into(),
                object: "https://ds-labs.org/code#Function".into(),
            },
            RawTriple {
                subject: "https://ds-labs.org/code#src/main.rs/my_function".into(),
                predicate: "https://ds-labs.org/code#name".into(),
                object: "my_function".into(),
            },
            RawTriple {
                subject: "https://ds-labs.org/code#src/main.rs/my_function".into(),
                predicate: "https://ds-labs.org/code#visibility".into(),
                object: "pub".into(),
            },
        ];
        let result = c.canonicalize(&triples);
        let (_, text) = &result[0];
        let expected = "\
code:src/main.rs/my_function
  rdf:type: code:Function
  code:name: my_function
  code:visibility: pub";
        assert_eq!(text, expected);
    }
}
