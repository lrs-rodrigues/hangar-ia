use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::Context;
use sha2::{Digest, Sha256};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind, new_index};

pub const HASHING_V1_DIMENSIONS: usize = 128;
pub const HASHING_V1_PROVIDER: &str = "hashing-v1";
pub const HASHING_V1_MODEL_REVISION: &str = "builtin-1";
pub const LOCAL_MULTILINGUAL_V1_PROVIDER: &str = "local-multilingual-v1";
pub const LOCAL_MULTILINGUAL_V1_MODEL_REVISION: &str =
    "qdrant-paraphrase-multilingual-minilm-l12-v2-onnx-q";
pub const LOCAL_MULTILINGUAL_V1_DIMENSIONS: usize = 384;
const LOCAL_MULTILINGUAL_CACHE_REPOSITORY: &str =
    "models--Qdrant--paraphrase-multilingual-MiniLM-L12-v2-onnx-Q";

/// Immutable identity of an embedding space. Index generations, manifests and
/// retrieval must agree on all three fields; vectors from different spaces are
/// never comparable or mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddingProfile {
    pub provider: &'static str,
    pub model_revision: &'static str,
    pub dimensions: usize,
}

pub const HASHING_V1_PROFILE: EmbeddingProfile = EmbeddingProfile {
    provider: HASHING_V1_PROVIDER,
    model_revision: HASHING_V1_MODEL_REVISION,
    dimensions: HASHING_V1_DIMENSIONS,
};

pub const LOCAL_MULTILINGUAL_V1_PROFILE: EmbeddingProfile = EmbeddingProfile {
    provider: LOCAL_MULTILINGUAL_V1_PROVIDER,
    model_revision: LOCAL_MULTILINGUAL_V1_MODEL_REVISION,
    dimensions: LOCAL_MULTILINGUAL_V1_DIMENSIONS,
};

impl EmbeddingProfile {
    pub fn matches_manifest(self, provider: &str, model_revision: &str, dimensions: usize) -> bool {
        self.provider == provider
            && self.model_revision == model_revision
            && self.dimensions == dimensions
    }
}

/// The only component permitted to create vectors. It keeps the selected
/// embedding space explicit, so vector generations cannot mix providers.
pub enum EmbeddingProvider {
    HashingV1,
    LocalMultilingualV1 {
        model: Mutex<fastembed::TextEmbedding>,
    },
}

impl EmbeddingProvider {
    pub fn hashing_v1() -> Self {
        Self::HashingV1
    }

    /// Loads only bytes from an already verified model cache. It deliberately
    /// uses FastEmbed's user-defined model API rather than its hub API, making
    /// network access impossible on the serving path.
    pub fn local_multilingual_v1_from_verified_cache(root: &Path) -> anyhow::Result<Self> {
        let snapshots = root
            .join(LOCAL_MULTILINGUAL_CACHE_REPOSITORY)
            .join("snapshots");
        let mut entries = fs::read_dir(&snapshots)
            .with_context(|| format!("reading local model snapshots {}", snapshots.display()))?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir());
        let snapshot = entries
            .next()
            .context("verified local model cache has no snapshot")?
            .path();
        anyhow::ensure!(
            entries.next().is_none(),
            "verified local model cache must contain exactly one snapshot"
        );
        let read = |name: &str| -> anyhow::Result<Vec<u8>> {
            let path = snapshot.join(name);
            fs::read(&path)
                .with_context(|| format!("reading local model artifact {}", path.display()))
        };
        let model = fastembed::UserDefinedEmbeddingModel::new(
            read("model_optimized.onnx")?,
            fastembed::TokenizerFiles {
                tokenizer_file: read("tokenizer.json")?,
                config_file: read("config.json")?,
                special_tokens_map_file: read("special_tokens_map.json")?,
                tokenizer_config_file: read("tokenizer_config.json")?,
            },
        )
        .with_pooling(fastembed::Pooling::Mean)
        .with_quantization(fastembed::QuantizationMode::Static);
        let model = fastembed::TextEmbedding::try_new_from_user_defined(
            model,
            fastembed::InitOptionsUserDefined::new(),
        )
        .context("loading verified local multilingual ONNX model")?;
        Ok(Self::LocalMultilingualV1 {
            model: Mutex::new(model),
        })
    }

    pub fn profile(&self) -> EmbeddingProfile {
        match self {
            Self::HashingV1 => HASHING_V1_PROFILE,
            Self::LocalMultilingualV1 { .. } => LOCAL_MULTILINGUAL_V1_PROFILE,
        }
    }

    pub fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        match self {
            Self::HashingV1 => Ok(embed_hashing_v1(text)),
            Self::LocalMultilingualV1 { model } => {
                let mut model = model
                    .lock()
                    .map_err(|_| anyhow::anyhow!("local embedding model lock poisoned"))?;
                let vectors = model
                    .embed(vec![text.to_owned()], None)
                    .context("embedding text with local multilingual model")?;
                let vector = vectors
                    .into_iter()
                    .next()
                    .context("local multilingual model returned no vector")?;
                anyhow::ensure!(
                    vector.len() == LOCAL_MULTILINGUAL_V1_DIMENSIONS,
                    "local multilingual model returned unexpected dimensions"
                );
                Ok(vector)
            }
        }
    }
}

/// The offline compatibility embedding. It intentionally makes no semantic
/// quality claim; production providers will implement the same boundary.
pub fn embed_hashing_v1(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; HASHING_V1_DIMENSIONS];
    for token in text
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let digest = Sha256::digest(token.as_bytes());
        let bucket = usize::from(digest[0]) % HASHING_V1_DIMENSIONS;
        let sign = if digest[1] & 1 == 0 { 1.0 } else { -1.0 };
        vector[bucket] += sign;
    }
    normalize(&mut vector);
    vector
}

pub struct WorkspaceVectorIndex {
    root: PathBuf,
}

impl WorkspaceVectorIndex {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let root = data_dir.join("vectors");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Publishes a complete replacement generated only from canonical manifests.
    /// The old generation remains intact until the new file is fully written.
    pub fn publish_generation(
        &self,
        organization_id: &str,
        workspace_id: &str,
        profile: EmbeddingProfile,
        entries: &[(u64, Vec<f32>)],
    ) -> anyhow::Result<()> {
        for (_, vector) in entries {
            anyhow::ensure!(
                vector.len() == profile.dimensions,
                "invalid vector dimensions"
            );
        }
        let path = self.index_path(organization_id, workspace_id, profile);
        let index = self.new_index(profile)?;
        index.reserve(entries.len().max(1))?;
        for (key, vector) in entries {
            index.add(*key, vector)?;
        }
        self.save_atomically(&index, &path)
    }

    /// Removes interrupted publication artifacts. Canonical manifests decide
    /// whether a full generation needs rebuilding afterwards.
    pub fn remove_stale_temporary_files(&self) -> anyhow::Result<usize> {
        let mut removed = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")
            {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn search(
        &self,
        organization_id: &str,
        workspace_id: &str,
        profile: EmbeddingProfile,
        query: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<(u64, f32)>> {
        anyhow::ensure!(
            query.len() == profile.dimensions,
            "invalid vector dimensions"
        );
        let path = self.index_path(organization_id, workspace_id, profile);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let index = self.load_or_create(&path, profile)?;
        let matches = index.search(query, limit)?;
        Ok(matches.keys.into_iter().zip(matches.distances).collect())
    }

    fn load_or_create(&self, path: &Path, profile: EmbeddingProfile) -> anyhow::Result<Index> {
        let index = self.new_index(profile)?;
        if path.exists() {
            index
                .load(path.to_str().context("vector index path is not UTF-8")?)
                .with_context(|| format!("loading vector index {}", path.display()))?;
        }
        Ok(index)
    }

    fn new_index(&self, profile: EmbeddingProfile) -> anyhow::Result<Index> {
        let options = IndexOptions {
            dimensions: profile.dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 0,
            expansion_add: 0,
            expansion_search: 0,
            multi: false,
        };
        Ok(new_index(&options)?)
    }

    fn save_atomically(&self, index: &Index, path: &Path) -> anyhow::Result<()> {
        let temporary = path.with_extension("tmp");
        index.save(
            temporary
                .to_str()
                .context("vector index path is not UTF-8")?,
        )?;
        fs::File::open(&temporary)?.sync_all()?;
        fs::rename(&temporary, path)
            .with_context(|| format!("publishing vector index {}", path.display()))?;
        Ok(())
    }

    fn index_path(
        &self,
        organization_id: &str,
        workspace_id: &str,
        profile: EmbeddingProfile,
    ) -> PathBuf {
        let scope = format!(
            "{organization_id}:{workspace_id}:{}:{}:{}",
            profile.provider, profile.model_revision, profile.dimensions
        );
        self.root
            .join(format!("{}.usearch", hex::encode(Sha256::digest(scope))))
    }
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_embedding_is_deterministic_and_normalized() {
        let first = embed_hashing_v1("OIDC service authentication");
        let second = embed_hashing_v1("OIDC service authentication");
        assert_eq!(first, second);
        let norm = first.iter().map(|value| value * value).sum::<f32>();
        assert!((norm - 1.0).abs() < 0.000_1);
    }

    #[test]
    fn workspace_indexes_persist_and_isolate_vectors() {
        let temporary = tempfile::tempdir().unwrap();
        let index = WorkspaceVectorIndex::open(temporary.path()).unwrap();
        let vector = embed_hashing_v1("OIDC service authentication");
        index
            .publish_generation(
                "acme",
                "payments",
                HASHING_V1_PROFILE,
                &[(7, vector.clone())],
            )
            .unwrap();
        assert_eq!(
            index
                .search("acme", "payments", HASHING_V1_PROFILE, &vector, 1)
                .unwrap()[0]
                .0,
            7
        );
        assert!(
            index
                .search("acme", "other", HASHING_V1_PROFILE, &vector, 1)
                .unwrap()
                .is_empty()
        );
        let reopened = WorkspaceVectorIndex::open(temporary.path()).unwrap();
        assert_eq!(
            reopened
                .search("acme", "payments", HASHING_V1_PROFILE, &vector, 1)
                .unwrap()[0]
                .0,
            7
        );
    }

    #[test]
    fn replacement_generation_and_temporary_cleanup_are_recoverable() {
        let temporary = tempfile::tempdir().unwrap();
        let index = WorkspaceVectorIndex::open(temporary.path()).unwrap();
        let vector = embed_hashing_v1("OIDC service authentication");
        index
            .publish_generation(
                "acme",
                "payments",
                HASHING_V1_PROFILE,
                &[(7, vector.clone())],
            )
            .unwrap();
        assert_eq!(
            index
                .search("acme", "payments", HASHING_V1_PROFILE, &vector, 1)
                .unwrap()[0]
                .0,
            7
        );
        fs::write(index.root.join("interrupted.tmp"), b"partial index").unwrap();
        assert_eq!(index.remove_stale_temporary_files().unwrap(), 1);
        assert!(!index.root.join("interrupted.tmp").exists());
    }

    #[test]
    fn projection_path_includes_model_revision() {
        let temporary = tempfile::tempdir().unwrap();
        let index = WorkspaceVectorIndex::open(temporary.path()).unwrap();
        let current = index.index_path("acme", "payments", HASHING_V1_PROFILE);
        let legacy_scope = format!("acme:payments:{HASHING_V1_PROVIDER}");
        let legacy = index.root.join(format!(
            "{}.usearch",
            hex::encode(Sha256::digest(legacy_scope))
        ));
        assert_ne!(current, legacy);
    }
}
