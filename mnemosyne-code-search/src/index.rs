use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tantivy::{
    schema::{Schema, STORED, TEXT},
    Index, IndexWriter, TantivyDocument,
};

/// A parsed function extracted from source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFunction {
    pub repo: String,
    pub file_path: String,
    pub name: String,
    pub docstring: Option<String>,
    pub body: String,
}

/// Wraps a Tantivy index with the schema used for code functions.
pub struct CodeIndex {
    pub(crate) index: Index,
    pub(crate) schema: CodeSchema,
}

/// Field handles for the code schema.
pub(crate) struct CodeSchema {
    pub schema: Schema,
    pub repo: tantivy::schema::Field,
    pub file_path: tantivy::schema::Field,
    pub name: tantivy::schema::Field,
    pub docstring: tantivy::schema::Field,
    pub body: tantivy::schema::Field,
}

impl CodeSchema {
    fn build() -> Self {
        let mut builder = Schema::builder();
        let repo = builder.add_text_field("repo", STORED | TEXT);
        let file_path = builder.add_text_field("file_path", STORED | TEXT);
        let name = builder.add_text_field("name", STORED | TEXT);
        let docstring = builder.add_text_field("docstring", STORED | TEXT);
        let body = builder.add_text_field("body", STORED | TEXT);
        CodeSchema {
            schema: builder.build(),
            repo,
            file_path,
            name,
            docstring,
            body,
        }
    }
}

impl CodeIndex {
    /// Create or open an index at `dir`.
    pub fn open_or_create(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let cs = CodeSchema::build();
        let index = Index::open_or_create(
            tantivy::directory::MmapDirectory::open(dir)?,
            cs.schema.clone(),
        )?;
        Ok(Self { index, schema: cs })
    }

    /// Index a batch of functions, then commit.
    pub fn add_functions(&self, fns: &[IndexedFunction]) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        for f in fns {
            let mut doc = TantivyDocument::default();
            doc.add_text(self.schema.repo, &f.repo);
            doc.add_text(self.schema.file_path, &f.file_path);
            doc.add_text(self.schema.name, &f.name);
            if let Some(ds) = &f.docstring {
                doc.add_text(self.schema.docstring, ds);
            }
            doc.add_text(self.schema.body, &f.body);
            writer.add_document(doc)?;
        }
        writer.commit()?;
        Ok(())
    }

    /// Index all Clojure files from a `CodeRepository` at HEAD.
    pub fn index_repository(
        &self,
        repo: &mnemosyne_code_storage::CodeRepository,
        repo_name: &str,
    ) -> Result<()> {
        let files = repo.files_at_rev("HEAD")?;
        let fns: Vec<IndexedFunction> = files
            .iter()
            .filter(|f| {
                f.path
                    .extension()
                    .is_some_and(|e| e == "clj" || e == "cljc" || e == "cljs")
            })
            .filter_map(|f| {
                let content = f.content_str()?;
                Some(IndexedFunction {
                    repo: repo_name.to_owned(),
                    file_path: f.path.to_string_lossy().into_owned(),
                    // Placeholder: a real implementation would parse the AST
                    name: f.path.file_stem()?.to_string_lossy().into_owned(),
                    docstring: None,
                    body: content.to_owned(),
                })
            })
            .collect();
        self.add_functions(&fns)
    }
}
