#![allow(dead_code)]

use std::collections::HashMap;

#[derive(Clone)]
pub struct ContentApiDefinition {
    pub content_type: String,
    pub base_url: String,
}

#[derive(Clone)]
pub struct ContentTypeRegistry {
    definitions: HashMap<String, ContentApiDefinition>,
}

impl ContentTypeRegistry {
    pub fn new(definitions: Vec<ContentApiDefinition>) -> Self {
        let definitions = definitions
            .into_iter()
            .map(|definition| (definition.content_type.clone(), definition))
            .collect();

        Self { definitions }
    }

    pub fn get(&self, content_type: &str) -> Option<&ContentApiDefinition> {
        self.definitions.get(content_type)
    }

    pub fn contains(&self, content_type: &str) -> bool {
        self.definitions.contains_key(content_type)
    }

    pub fn all(&self) -> Vec<&ContentApiDefinition> {
        let mut definitions: Vec<_> = self.definitions.values().collect();
        definitions.sort_by(|left, right| left.content_type.cmp(&right.content_type));
        definitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_returns_definitions_by_content_type() {
        let registry = ContentTypeRegistry::new(vec![
            ContentApiDefinition {
                content_type: "post".to_string(),
                base_url: "http://post-api".to_string(),
            },
            ContentApiDefinition {
                content_type: "bonus_hunter".to_string(),
                base_url: "http://bonus-api".to_string(),
            },
        ]);

        let definition = registry.get("post").expect("post definition must exist");

        assert_eq!(definition.content_type, "post");
        assert_eq!(definition.base_url, "http://post-api");
        assert!(registry.get("top_picks").is_none());
    }

    #[test]
    fn registry_all_returns_sorted_definitions() {
        let registry = ContentTypeRegistry::new(vec![
            ContentApiDefinition {
                content_type: "top_picks".to_string(),
                base_url: "http://top-picks-api".to_string(),
            },
            ContentApiDefinition {
                content_type: "bonus_hunter".to_string(),
                base_url: "http://bonus-api".to_string(),
            },
            ContentApiDefinition {
                content_type: "post".to_string(),
                base_url: "http://post-api".to_string(),
            },
        ]);

        let ordered: Vec<_> = registry
            .all()
            .into_iter()
            .map(|definition| definition.content_type.as_str())
            .collect();

        assert_eq!(ordered, vec!["bonus_hunter", "post", "top_picks"]);
    }
}
