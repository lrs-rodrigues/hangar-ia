use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use sha2::{Digest, Sha256};
use tantivy::{
    Index, ReloadPolicy, TantivyDocument,
    collector::TopDocs,
    doc,
    query::QueryParser,
    schema::{STORED, STRING, TEXT, Value},
};

pub const TEXT_INDEX_VERSION: u32 = 1;
const WRITER_MEMORY_BYTES: usize = 15_000_000;

#[derive(Debug, Clone)]
pub struct TextEntry {
    pub chunk_id: String,
    pub content: String,
}

pub struct WorkspaceTextIndex {
    root: PathBuf,
}

impl WorkspaceTextIndex {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let root = data_dir.join("text");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Publishes a complete, immutable index generation. The caller activates
    /// this generation in canonical storage only after this returns.
    pub fn publish_generation(
        &self,
        organization_id: &str,
        workspace_id: &str,
        generation: &str,
        entries: &[TextEntry],
    ) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(
            !generation.trim().is_empty(),
            "text generation cannot be empty"
        );
        let workspace_dir = self.workspace_dir(organization_id, workspace_id);
        fs::create_dir_all(&workspace_dir)?;
        let final_path = workspace_dir.join(generation);
        anyhow::ensure!(!final_path.exists(), "text index generation already exists");
        let temporary_path = workspace_dir.join(format!(".{generation}.tmp"));
        if temporary_path.exists() {
            fs::remove_dir_all(&temporary_path).with_context(|| {
                format!(
                    "removing interrupted text generation {}",
                    temporary_path.display()
                )
            })?;
        }
        fs::create_dir(&temporary_path)?;
        let schema = schema();
        let index = Index::create_in_dir(&temporary_path, schema.clone())?;
        let chunk_id = schema.get_field("chunk_id")?;
        let content = schema.get_field("content")?;
        let mut writer = index.writer(WRITER_MEMORY_BYTES)?;
        for entry in entries {
            writer.add_document(
                doc!(chunk_id => entry.chunk_id.clone(), content => entry.content.clone()),
            )?;
        }
        writer.commit()?;
        drop(writer);
        fs::rename(&temporary_path, &final_path)
            .with_context(|| format!("publishing text index {}", final_path.display()))?;
        Ok(final_path)
    }

    pub fn search(
        &self,
        organization_id: &str,
        workspace_id: &str,
        generation: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        anyhow::ensure!(!query.trim().is_empty(), "query cannot be empty");
        let path = self.generation_path(organization_id, workspace_id, generation);
        if !path.is_dir() {
            return Ok(Vec::new());
        }
        let index = Index::open_in_dir(&path)
            .with_context(|| format!("opening text index {}", path.display()))?;
        let schema = index.schema();
        let chunk_id = schema.get_field("chunk_id")?;
        let content = schema.get_field("content")?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        reader.reload()?;
        let parser = QueryParser::for_index(&index, vec![content]);
        let query = parser.parse_query(query)?;
        let searcher = reader.searcher();
        let documents = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;
        documents
            .into_iter()
            .map(|(score, address)| {
                let document: TantivyDocument = searcher.doc(address)?;
                let key = document
                    .get_first(chunk_id)
                    .and_then(|value| Value::as_str(&value))
                    .context("text index document is missing chunk_id")?;
                Ok((key.to_owned(), score))
            })
            .collect()
    }

    pub fn generation_path(
        &self,
        organization_id: &str,
        workspace_id: &str,
        generation: &str,
    ) -> PathBuf {
        self.workspace_dir(organization_id, workspace_id)
            .join(generation)
    }

    /// Removes temporary or unreachable generation directories. The caller
    /// passes canonical active generations; no index file is used as truth.
    pub fn cleanup_unreferenced_generations(
        &self,
        active_paths: &BTreeSet<PathBuf>,
    ) -> anyhow::Result<usize> {
        let mut removed = 0;
        for workspace in fs::read_dir(&self.root)? {
            let workspace = workspace?;
            if !workspace.file_type()?.is_dir() {
                continue;
            }
            for generation in fs::read_dir(workspace.path())? {
                let generation = generation?;
                let path = generation.path();
                if !generation.file_type()?.is_dir() {
                    continue;
                }
                let name = generation.file_name();
                let temporary = name.to_string_lossy().starts_with('.');
                if temporary || !active_paths.contains(&path) {
                    fs::remove_dir_all(&path).with_context(|| {
                        format!("removing stale text generation {}", path.display())
                    })?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    fn workspace_dir(&self, organization_id: &str, workspace_id: &str) -> PathBuf {
        let scope = format!("{organization_id}:{workspace_id}:tantivy:{TEXT_INDEX_VERSION}");
        self.root.join(hex::encode(Sha256::digest(scope)))
    }
}

fn schema() -> tantivy::schema::Schema {
    let mut builder = tantivy::schema::Schema::builder();
    builder.add_text_field("chunk_id", STRING | STORED);
    builder.add_text_field("content", TEXT);
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_persist_search_and_isolate_workspaces() {
        let temporary = tempfile::tempdir().unwrap();
        let index = WorkspaceTextIndex::open(temporary.path()).unwrap();
        index
            .publish_generation(
                "acme",
                "payments",
                "generation-one",
                &[TextEntry {
                    chunk_id: "chunk-one".into(),
                    content: "OIDC workload identity for payment services".into(),
                }],
            )
            .unwrap();
        assert_eq!(
            index
                .search(
                    "acme",
                    "payments",
                    "generation-one",
                    "workload identity",
                    10
                )
                .unwrap()[0]
                .0,
            "chunk-one"
        );
        assert!(
            index
                .search(
                    "acme",
                    "research",
                    "generation-one",
                    "workload identity",
                    10
                )
                .unwrap()
                .is_empty()
        );
        let reopened = WorkspaceTextIndex::open(temporary.path()).unwrap();
        assert_eq!(
            reopened
                .search("acme", "payments", "generation-one", "OIDC", 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn cleanup_removes_only_unreferenced_or_temporary_generations() {
        let temporary = tempfile::tempdir().unwrap();
        let index = WorkspaceTextIndex::open(temporary.path()).unwrap();
        let active = index
            .publish_generation(
                "acme",
                "payments",
                "active",
                &[TextEntry {
                    chunk_id: "chunk-one".into(),
                    content: "OIDC".into(),
                }],
            )
            .unwrap();
        index
            .publish_generation(
                "acme",
                "payments",
                "orphan",
                &[TextEntry {
                    chunk_id: "chunk-two".into(),
                    content: "orphan".into(),
                }],
            )
            .unwrap();
        let workspace = active.parent().unwrap();
        fs::create_dir(workspace.join(".interrupted.tmp")).unwrap();
        assert_eq!(
            index
                .cleanup_unreferenced_generations(&BTreeSet::from([active.clone()]))
                .unwrap(),
            2
        );
        assert!(active.exists());
        assert!(!workspace.join("orphan").exists());
    }

    #[test]
    fn bm25_ranks_exact_lexical_match_first() {
        let temporary = tempfile::tempdir().unwrap();
        let index = WorkspaceTextIndex::open(temporary.path()).unwrap();
        index
            .publish_generation(
                "acme",
                "payments",
                "ranking",
                &[
                    TextEntry {
                        chunk_id: "exact".into(),
                        content: "The deployment runbook rotates a zephyr signing credential."
                            .into(),
                    },
                    TextEntry {
                        chunk_id: "other".into(),
                        content: "The deployment runbook documents ordinary maintenance.".into(),
                    },
                ],
            )
            .unwrap();
        let results = index
            .search("acme", "payments", "ranking", "zephyr credential", 10)
            .unwrap();
        assert_eq!(results[0].0, "exact");
        assert!(results[0].1 > 0.0);
    }
}
