use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use autoharness_domain::{ArtifactId, ArtifactRef};
use sha2::{Digest, Sha256};

use crate::{ToolError, ToolErrorKind};
use autoharness_domain::RetryAdvice;

static TEMPORARY_ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Content-addressed full-output storage capability.
#[async_trait]
pub trait ArtifactStore: Send + Sync {
    /// Stores bytes idempotently and returns verified metadata.
    async fn put(&self, bytes: &[u8], media_type: &str) -> Result<ArtifactRef, ToolError>;
}

/// Filesystem artifact store rooted in an application-owned directory.
pub struct FileArtifactStore {
    root: PathBuf,
}

impl FileArtifactStore {
    /// Creates the artifact directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        std::fs::create_dir_all(root.as_ref()).map_err(|_| artifact_error())?;
        let root = std::fs::canonicalize(root).map_err(|_| artifact_error())?;
        Ok(Self { root })
    }
}

#[async_trait]
impl ArtifactStore for FileArtifactStore {
    async fn put(&self, bytes: &[u8], media_type: &str) -> Result<ArtifactRef, ToolError> {
        if bytes.is_empty() {
            return Err(artifact_error());
        }
        let digest = Sha256::digest(bytes);
        let hex = hex(&digest);
        let artifact_id = ArtifactId::new(format!("sha256:{hex}")).map_err(|_| artifact_error())?;
        let path = self.root.join(&hex);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|_| artifact_error())?
        {
            verify_artifact(&path, &digest).await?;
        } else {
            let sequence = TEMPORARY_ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temporary =
                self.root
                    .join(format!(".{hex}.{}.{}.tmp", std::process::id(), sequence));
            let write_result = async {
                use tokio::io::AsyncWriteExt as _;

                let mut file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .await?;
                file.write_all(bytes).await?;
                file.sync_all().await?;
                drop(file);
                tokio::fs::rename(&temporary, &path).await
            }
            .await;
            if let Err(error) = write_result {
                let _ = tokio::fs::remove_file(&temporary).await;
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(artifact_error());
                }
                verify_artifact(&path, &digest).await?;
            }
        }
        ArtifactRef::new(
            artifact_id,
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            media_type,
        )
        .map_err(|_| artifact_error())
    }
}

async fn verify_artifact(path: &Path, digest: &[u8]) -> Result<(), ToolError> {
    let existing = tokio::fs::read(path).await.map_err(|_| artifact_error())?;
    if Sha256::digest(existing).as_slice() != digest {
        return Err(artifact_error());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn artifact_error() -> ToolError {
    ToolError::new(ToolErrorKind::Artifact, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn content_addressing_is_idempotent() {
        let directory = tempfile::tempdir().expect("directory");
        let store = FileArtifactStore::new(directory.path()).expect("store");
        let first = store.put(b"same", "text/plain").await.expect("artifact");
        let second = store.put(b"same", "text/plain").await.expect("artifact");
        assert_eq!(first, second);
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("entries")
                .count(),
            1
        );
    }
}
