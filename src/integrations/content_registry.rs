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

    pub fn all(&self) -> Vec<&ContentApiDefinition> {
        let mut definitions: Vec<_> = self.definitions.values().collect();
        definitions.sort_by(|left, right| left.content_type.cmp(&right.content_type));
        definitions
    }
}
