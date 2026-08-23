use std::collections::BTreeSet;

pub const DETERMINISTIC_EXTRACTOR: &str = "deterministic-keyword-v1";
pub const EXTRACTION_VERSION: u32 = 1;
pub const MAX_ENTITIES_PER_CHUNK: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityCandidate {
    pub normalized_name: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct RelationCandidate {
    pub source_normalized_name: String,
    pub target_normalized_name: String,
    pub relation_type: &'static str,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct Extraction {
    pub entities: Vec<EntityCandidate>,
    pub relations: Vec<RelationCandidate>,
}

/// This extractor deliberately produces *proposals*, not facts. It selects a
/// bounded set of meaningful lexical terms and records their co-occurrence as
/// evidence-backed relations. It is deterministic, offline, and safe to
/// replay; a model-backed extractor can later add another named version.
pub fn extract(content: &str) -> Extraction {
    let mut seen = BTreeSet::new();
    let mut entities = Vec::new();
    for raw in content.split(|character: char| !character.is_alphanumeric()) {
        let trimmed = raw.trim();
        let normalized_name = trimmed.to_lowercase();
        if normalized_name.len() < 3
            || is_stopword(&normalized_name)
            || !normalized_name.chars().any(char::is_alphabetic)
        {
            continue;
        }
        if seen.insert(normalized_name.clone()) {
            entities.push(EntityCandidate {
                normalized_name,
                display_name: trimmed.to_owned(),
            });
            if entities.len() >= MAX_ENTITIES_PER_CHUNK {
                break;
            }
        }
    }
    let mut relations = Vec::new();
    for (offset, source) in entities.iter().enumerate() {
        for target in entities.iter().skip(offset + 1) {
            let (source_normalized_name, target_normalized_name) =
                if source.normalized_name <= target.normalized_name {
                    (&source.normalized_name, &target.normalized_name)
                } else {
                    (&target.normalized_name, &source.normalized_name)
                };
            relations.push(RelationCandidate {
                source_normalized_name: source_normalized_name.clone(),
                target_normalized_name: target_normalized_name.clone(),
                relation_type: "co_occurs_with",
                confidence: 0.5,
            });
        }
    }
    Extraction {
        entities,
        relations,
    }
}

pub fn query_terms(query: &str) -> Vec<String> {
    extract(query)
        .entities
        .into_iter()
        .map(|entity| entity.normalized_name)
        .collect()
}

fn is_stopword(term: &str) -> bool {
    matches!(
        term,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "that"
            | "this"
            | "are"
            | "was"
            | "were"
            | "into"
            | "about"
            | "para"
            | "com"
            | "uma"
            | "que"
            | "dos"
            | "das"
            | "por"
            | "não"
            | "nos"
            | "nas"
            | "tem"
            | "são"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_is_deterministic_and_bounded() {
        let first = extract("OIDC authorizes Zephyr credential rotation for Payments.");
        let second = extract("OIDC authorizes Zephyr credential rotation for Payments.");
        assert_eq!(first.entities, second.entities);
        assert!(first.entities.len() <= MAX_ENTITIES_PER_CHUNK);
        assert!(
            first
                .relations
                .iter()
                .any(|relation| relation.relation_type == "co_occurs_with")
        );
    }
}
