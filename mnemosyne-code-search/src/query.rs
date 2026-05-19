use crate::{CodeIndex, IndexedFunction, Result};
use serde::{Deserialize, Serialize};
use tantivy::{
    collector::TopDocs, query::QueryParser, schema::Value, ReloadPolicy, TantivyDocument,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Natural-language or keyword description of the desired functionality.
    pub text: String,
    pub limit: usize,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            limit: 10,
        }
    }

    pub fn with_limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub score: f32,
    pub function: IndexedFunction,
}

impl CodeIndex {
    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchResult>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let searcher = reader.searcher();

        let parser = QueryParser::for_index(
            &self.index,
            vec![self.schema.name, self.schema.docstring, self.schema.body],
        );
        let query = parser.parse_query(&q.text)?;
        let top = searcher.search(&query, &TopDocs::with_limit(q.limit))?;

        let s = &self.schema;
        let results = top
            .into_iter()
            .filter_map(|(score, addr)| {
                let doc: TantivyDocument = searcher.doc(addr).ok()?;
                let get = |field| {
                    doc.get_first(field)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned()
                };
                Some(SearchResult {
                    score,
                    function: IndexedFunction {
                        repo: get(s.repo),
                        file_path: get(s.file_path),
                        name: get(s.name),
                        docstring: {
                            let d = get(s.docstring);
                            if d.is_empty() {
                                None
                            } else {
                                Some(d)
                            }
                        },
                        body: get(s.body),
                    },
                })
            })
            .collect();

        Ok(results)
    }
}
