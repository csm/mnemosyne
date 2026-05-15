use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata about a registered Clojure function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionTemplate {
    pub ns: String,
    pub name: String,
    pub docstring: Option<String>,
    /// Named slots that the structural editor should substitute when specialising this template.
    pub slots: Vec<String>,
    pub source: String,
}

/// In-process registry of known functions and templates.
#[derive(Default)]
pub struct FunctionRegistry {
    fns: HashMap<String, FunctionTemplate>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, f: FunctionTemplate) {
        self.fns.insert(format!("{}/{}", f.ns, f.name), f);
    }

    pub fn get(&self, qualified_name: &str) -> Option<&FunctionTemplate> {
        self.fns.get(qualified_name)
    }

    pub fn all_templates(&self) -> impl Iterator<Item = &FunctionTemplate> {
        self.fns.values().filter(|f| !f.slots.is_empty())
    }
}
