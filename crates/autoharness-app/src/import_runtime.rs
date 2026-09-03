//! Safe construction of review-only memory proposals from workspace documents.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use autoharness_domain::{
    ConfidenceBasisPoints, ContextSourceKey, MemoryCommandEnvelope, MemoryCommandPayload,
    MemoryContent, MemoryEvidence, MemoryEvidenceId, MemoryEvidenceRelation, MemoryEvidenceSource,
    MemoryKind, MemoryOrigin, MemoryRevisionDraft, MemoryRevisionId, MemoryRevisionNumber,
    MemoryScope, MemorySubjectKey, MemoryValidity, Sensitivity, Sha256Digest, TrustClass,
    WorkspaceId,
};
use autoharness_memory::normalized_content_hash;
use sha2::{Digest as _, Sha256};

use crate::ids;

const IMPORT_SOURCE_KEY_PREFIX: &str = "workspace:imported-document:v1:";
const IMPORT_SUBJECT_KEY_PREFIX: &str = "imported-document:v1:";
const IMPORT_EVIDENCE_ID_PREFIX: &str = "imported-document-evidence-v1";

/// Safe failure classification for one explicit workspace-document import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportDocumentError {
    /// The requested path was empty, absolute, non-UTF-8, or contained traversal components.
    InvalidRelativePath,
    /// The configured workspace root could not be resolved to a directory.
    WorkspaceUnavailable,
    /// The requested document does not exist.
    DocumentNotFound,
    /// Canonical resolution placed the document outside the configured workspace.
    PathEscapesWorkspace,
    /// The requested path did not resolve to a regular file.
    NotRegularFile,
    /// The document could not be opened or read completely.
    DocumentUnavailable,
    /// The document exceeded the exact memory-content byte bound.
    DocumentTooLarge,
    /// The document was not valid UTF-8 text.
    InvalidUtf8,
    /// The document contained control characters that are unsafe for durable text memory.
    UnsafeControlCharacter,
    /// Existing domain validation rejected the proposed content or provenance values.
    InvalidDomainValue,
}

impl Display for ImportDocumentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRelativePath => {
                "the import path must be a workspace-relative document path without traversal"
            }
            Self::WorkspaceUnavailable => "the configured workspace is unavailable",
            Self::DocumentNotFound => "the requested workspace document does not exist",
            Self::PathEscapesWorkspace => {
                "the requested document resolves outside the configured workspace"
            }
            Self::NotRegularFile => "the requested workspace path is not a regular file",
            Self::DocumentUnavailable => "the requested workspace document could not be read",
            Self::DocumentTooLarge => "the workspace document exceeds the 16 KiB import limit",
            Self::InvalidUtf8 => "the workspace document is not valid UTF-8 text",
            Self::UnsafeControlCharacter => {
                "the workspace document contains unsafe control characters"
            }
            Self::InvalidDomainValue => {
                "the workspace document could not become a bounded memory proposal"
            }
        })
    }
}

impl Error for ImportDocumentError {}

/// Constructs one workspace-scoped, review-only proposal from exact document bytes.
///
/// The path must be relative to `workspace_root`. Canonical containment is
/// checked before reading, and the source key is derived from the canonical
/// relative identity without retaining the path itself. The returned command
/// has no activation operation or approval authority.
pub(crate) fn build_workspace_document_import(
    workspace_root: &Path,
    relative_path: &Path,
    workspace_id: WorkspaceId,
) -> Result<MemoryCommandEnvelope, ImportDocumentError> {
    validate_relative_path(relative_path)?;
    let root = canonical_workspace_root(workspace_root)?;
    let document = canonical_document_path(&root, relative_path)?;
    let relative_identity = canonical_relative_identity(&root, &document)?;
    let bytes = read_document_bounded(&document)?;
    let source_revision = raw_sha256(&bytes)?;
    let content = String::from_utf8(bytes).map_err(|_| ImportDocumentError::InvalidUtf8)?;
    if content
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ImportDocumentError::UnsafeControlCharacter);
    }
    let content =
        MemoryContent::new(content).map_err(|_| ImportDocumentError::InvalidDomainValue)?;
    let content_hash = normalized_content_hash(content.as_str())
        .map_err(|_| ImportDocumentError::InvalidDomainValue)?;
    let source_identity = imported_source_identity(&workspace_id, &relative_identity);
    let source_key = ContextSourceKey::new(format!("{IMPORT_SOURCE_KEY_PREFIX}{source_identity}"))
        .map_err(|_| ImportDocumentError::InvalidDomainValue)?;
    let subject_key =
        MemorySubjectKey::new(format!("{IMPORT_SUBJECT_KEY_PREFIX}{source_identity}"))
            .map_err(|_| ImportDocumentError::InvalidDomainValue)?;
    let revision_id = ids::memory_revision_id();
    let evidence_id = imported_evidence_id(&revision_id, &source_revision)?;
    let evidence = MemoryEvidence::new(
        evidence_id,
        MemoryEvidenceSource::ImportedDocument {
            source_key,
            source_revision,
        },
        MemoryEvidenceRelation::Supports,
        None,
        None,
    )
    .map_err(|_| ImportDocumentError::InvalidDomainValue)?;
    let revision = MemoryRevisionDraft::new(
        revision_id,
        MemoryRevisionNumber::FIRST,
        Some(subject_key),
        content,
        content_hash,
        MemoryOrigin::ImportedDocument,
        TrustClass::Imported,
        ConfidenceBasisPoints::new(8_000)
            .expect("the static imported-document confidence is valid"),
        Sensitivity::Internal,
        MemoryValidity::Indefinite,
        vec![evidence],
        Vec::new(),
    )
    .map_err(|_| ImportDocumentError::InvalidDomainValue)?;
    Ok(ids::memory_command(
        ids::memory_id(),
        None,
        MemoryCommandPayload::CreateMemory {
            scope: MemoryScope::Workspace(workspace_id),
            memory_kind: MemoryKind::Fact,
            revision,
        },
    ))
}

fn validate_relative_path(path: &Path) -> Result<(), ImportDocumentError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_)) || component.as_os_str().to_str().is_none()
        })
    {
        return Err(ImportDocumentError::InvalidRelativePath);
    }
    Ok(())
}

fn canonical_workspace_root(path: &Path) -> Result<PathBuf, ImportDocumentError> {
    let root =
        std::fs::canonicalize(path).map_err(|_| ImportDocumentError::WorkspaceUnavailable)?;
    if !root.is_dir() {
        return Err(ImportDocumentError::WorkspaceUnavailable);
    }
    Ok(root)
}

fn canonical_document_path(
    root: &Path,
    relative_path: &Path,
) -> Result<PathBuf, ImportDocumentError> {
    let requested = root.join(relative_path);
    let document = std::fs::canonicalize(&requested).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ImportDocumentError::DocumentNotFound
        } else {
            ImportDocumentError::DocumentUnavailable
        }
    })?;
    ensure_contained(root, &document)?;
    if !document.is_file() {
        return Err(ImportDocumentError::NotRegularFile);
    }
    Ok(document)
}

fn ensure_contained(root: &Path, document: &Path) -> Result<(), ImportDocumentError> {
    if document.starts_with(root) && document != root {
        Ok(())
    } else {
        Err(ImportDocumentError::PathEscapesWorkspace)
    }
}

fn canonical_relative_identity(
    root: &Path,
    document: &Path,
) -> Result<String, ImportDocumentError> {
    let relative = document
        .strip_prefix(root)
        .map_err(|_| ImportDocumentError::PathEscapesWorkspace)?;
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .ok_or(ImportDocumentError::InvalidRelativePath),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => Err(ImportDocumentError::InvalidRelativePath),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.is_empty() {
        return Err(ImportDocumentError::InvalidRelativePath);
    }
    Ok(components.join("/"))
}

fn read_document_bounded(path: &Path) -> Result<Vec<u8>, ImportDocumentError> {
    let metadata = std::fs::metadata(path).map_err(|_| ImportDocumentError::DocumentUnavailable)?;
    let maximum =
        u64::try_from(MemoryContent::MAX_BYTES).expect("the memory-content byte bound fits u64");
    if metadata.len() > maximum {
        return Err(ImportDocumentError::DocumentTooLarge);
    }
    let file = File::open(path).map_err(|_| ImportDocumentError::DocumentUnavailable)?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .unwrap_or(MemoryContent::MAX_BYTES)
            .min(MemoryContent::MAX_BYTES),
    );
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ImportDocumentError::DocumentUnavailable)?;
    if bytes.len() > MemoryContent::MAX_BYTES {
        return Err(ImportDocumentError::DocumentTooLarge);
    }
    Ok(bytes)
}

fn imported_evidence_id(
    revision_id: &MemoryRevisionId,
    source_revision: &Sha256Digest,
) -> Result<MemoryEvidenceId, ImportDocumentError> {
    let mut identity = String::with_capacity(
        revision_id
            .as_str()
            .len()
            .saturating_add(source_revision.as_str().len())
            .saturating_add(1),
    );
    identity.push_str(revision_id.as_str());
    identity.push('\0');
    identity.push_str(source_revision.as_str());
    MemoryEvidenceId::new(format!(
        "{IMPORT_EVIDENCE_ID_PREFIX}-{}",
        domain_separated_digest(
            b"autoharness-workspace-import-evidence-v1\0",
            identity.as_bytes(),
        )
    ))
    .map_err(|_| ImportDocumentError::InvalidDomainValue)
}

fn imported_source_identity(workspace_id: &WorkspaceId, relative_identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"autoharness-workspace-import-source-v1\0");
    for field in [
        workspace_id.as_str().as_bytes(),
        relative_identity.as_bytes(),
    ] {
        hasher.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(field);
    }
    let digest = hasher.finalize();
    encode_digest(&digest)
}

fn raw_sha256(bytes: &[u8]) -> Result<Sha256Digest, ImportDocumentError> {
    Sha256Digest::new(hex_digest(bytes)).map_err(|_| ImportDocumentError::InvalidDomainValue)
}

fn domain_separated_digest(domain: &[u8], value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
    let digest = hasher.finalize();
    encode_digest(&digest)
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    encode_digest(&digest)
}

fn encode_digest(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use autoharness_domain::{
        MemoryOperationPayload, MemoryRevisionStatus, MemoryValidationStatus,
    };
    use autoharness_store::{DEFAULT_MEMORY_PAGE_SIZE, MemoryStore as _};
    use autoharness_store_sqlite::SqliteStore;

    use super::*;

    fn workspace_id() -> WorkspaceId {
        WorkspaceId::new("workspace-import-test").expect("workspace ID")
    }

    fn imported_revision(command: &MemoryCommandEnvelope) -> &MemoryRevisionDraft {
        let MemoryCommandPayload::CreateMemory {
            scope,
            memory_kind,
            revision,
        } = command.payload()
        else {
            panic!("import must create one memory item");
        };
        assert_eq!(scope, &MemoryScope::Workspace(workspace_id()));
        assert_eq!(*memory_kind, MemoryKind::Fact);
        revision
    }

    #[test]
    fn invalid_paths_fail_before_document_read() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let outside = directory.path().join("outside.txt");
        std::fs::write(&outside, "outside").expect("outside file");

        assert_eq!(
            build_workspace_document_import(&workspace, Path::new(""), workspace_id()),
            Err(ImportDocumentError::InvalidRelativePath)
        );
        assert_eq!(
            build_workspace_document_import(
                &workspace,
                Path::new("../outside.txt"),
                workspace_id()
            ),
            Err(ImportDocumentError::InvalidRelativePath)
        );
        assert_eq!(
            build_workspace_document_import(&workspace, &outside, workspace_id()),
            Err(ImportDocumentError::InvalidRelativePath)
        );
        let canonical_workspace = workspace.canonicalize().expect("canonical workspace");
        let canonical_outside = outside.canonicalize().expect("canonical outside file");
        assert_eq!(
            ensure_contained(&canonical_workspace, &canonical_outside),
            Err(ImportDocumentError::PathEscapesWorkspace)
        );
    }

    #[test]
    fn missing_directory_and_unavailable_workspace_are_distinct() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::create_dir(workspace.join("docs")).expect("document directory");

        assert_eq!(
            build_workspace_document_import(&workspace, Path::new("missing.md"), workspace_id()),
            Err(ImportDocumentError::DocumentNotFound)
        );
        assert_eq!(
            build_workspace_document_import(&workspace, Path::new("docs"), workspace_id()),
            Err(ImportDocumentError::NotRegularFile)
        );
        assert_eq!(
            build_workspace_document_import(
                &directory.path().join("missing-workspace"),
                Path::new("doc.md"),
                workspace_id(),
            ),
            Err(ImportDocumentError::WorkspaceUnavailable)
        );
    }

    #[test]
    fn non_utf8_oversized_and_control_unsafe_documents_are_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path();
        std::fs::write(workspace.join("binary.bin"), [0xff, 0xfe, 0xfd]).expect("binary file");
        std::fs::write(
            workspace.join("large.txt"),
            vec![b'x'; MemoryContent::MAX_BYTES.saturating_add(1)],
        )
        .expect("large file");
        std::fs::write(workspace.join("control.txt"), b"safe\0unsafe").expect("control file");

        assert_eq!(
            build_workspace_document_import(workspace, Path::new("binary.bin"), workspace_id()),
            Err(ImportDocumentError::InvalidUtf8)
        );
        assert_eq!(
            build_workspace_document_import(workspace, Path::new("large.txt"), workspace_id()),
            Err(ImportDocumentError::DocumentTooLarge)
        );
        assert_eq!(
            build_workspace_document_import(workspace, Path::new("control.txt"), workspace_id()),
            Err(ImportDocumentError::UnsafeControlCharacter)
        );
    }

    #[test]
    fn source_identity_and_exact_raw_revision_are_deterministic_without_path_leakage() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path();
        std::fs::create_dir(workspace.join("docs")).expect("docs directory");
        const BYTES: &[u8] = b"The workspace uses Rust 2024.\r\n";
        std::fs::write(workspace.join("docs").join("decision.txt"), BYTES).expect("document");

        let first = build_workspace_document_import(
            workspace,
            Path::new("docs/decision.txt"),
            workspace_id(),
        )
        .expect("first import");
        let second = build_workspace_document_import(
            workspace,
            Path::new("docs/decision.txt"),
            workspace_id(),
        )
        .expect("second import");
        let other_workspace = build_workspace_document_import(
            workspace,
            Path::new("docs/decision.txt"),
            WorkspaceId::new("workspace-import-test-other").expect("other workspace ID"),
        )
        .expect("other workspace import");
        let first_revision = imported_revision(&first);
        let second_revision = imported_revision(&second);
        assert_eq!(first_revision.content().as_str().as_bytes(), BYTES);
        assert_eq!(first_revision.origin(), MemoryOrigin::ImportedDocument);
        assert_eq!(first_revision.trust_class(), TrustClass::Imported);
        assert_eq!(first_revision.evidence().len(), 1);
        assert_eq!(second_revision.evidence().len(), 1);
        let (
            MemoryEvidenceSource::ImportedDocument {
                source_key: first_key,
                source_revision: first_source_revision,
            },
            MemoryEvidenceSource::ImportedDocument {
                source_key: second_key,
                source_revision: second_source_revision,
            },
        ) = (
            first_revision.evidence()[0].source(),
            second_revision.evidence()[0].source(),
        )
        else {
            panic!("imports require typed document evidence");
        };
        assert_eq!(first_key, second_key);
        assert_eq!(first_source_revision, second_source_revision);
        assert_eq!(first_source_revision, &raw_sha256(BYTES).expect("raw hash"));
        assert_eq!(first_revision.subject_key(), second_revision.subject_key());
        assert!(first_key.as_str().starts_with(IMPORT_SOURCE_KEY_PREFIX));
        assert!(!first_key.as_str().contains("docs"));
        assert!(!first_key.as_str().contains("decision"));
        let MemoryCommandPayload::CreateMemory {
            revision: other_workspace_revision,
            ..
        } = other_workspace.payload()
        else {
            panic!("import must create one memory item");
        };
        let MemoryEvidenceSource::ImportedDocument {
            source_key: other_workspace_key,
            ..
        } = other_workspace_revision.evidence()[0].source()
        else {
            panic!("imports require typed document evidence");
        };
        assert_ne!(first_key, other_workspace_key);
        assert_ne!(first.memory_id(), second.memory_id());
        assert_ne!(first_revision.revision_id(), second_revision.revision_id());
    }

    #[test]
    fn imported_command_commits_only_a_reviewable_proposal() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::write(workspace.join("facts.txt"), "The project uses Rust 2024.")
            .expect("document");
        let command =
            build_workspace_document_import(&workspace, Path::new("facts.txt"), workspace_id())
                .expect("import command");
        let database = directory.path().join("import.sqlite3");
        let mut store = SqliteStore::open(&database).expect("open store");

        let commit = crate::memory_runtime::execute_memory_command(
            &mut store,
            &command,
            autoharness_domain::TimestampMillis::new(10),
        )
        .expect("commit imported proposal");
        assert_eq!(
            commit.validation().expect("validation").status(),
            MemoryValidationStatus::NeedsReview
        );
        let revisions = store
            .load_memory_revisions(command.memory_id())
            .expect("load revisions");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].status(), MemoryRevisionStatus::Proposed);
        assert_eq!(revisions[0].origin(), MemoryOrigin::ImportedDocument);
        let operations = store
            .load_memory_operations(command.memory_id(), 0, DEFAULT_MEMORY_PAGE_SIZE)
            .expect("load operations");
        assert!(operations.iter().all(|operation| !matches!(
            operation.payload(),
            MemoryOperationPayload::RevisionActivated { .. }
        )));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_after_canonicalization() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let outside = directory.path().join("outside.txt");
        std::fs::write(&outside, "outside").expect("outside document");
        symlink(&outside, workspace.join("linked.txt")).expect("symlink");

        assert_eq!(
            build_workspace_document_import(&workspace, Path::new("linked.txt"), workspace_id(),),
            Err(ImportDocumentError::PathEscapesWorkspace)
        );
    }

    #[test]
    fn maximum_sized_document_is_read_within_the_hard_bound() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("document.txt");
        let mut file = File::create(&path).expect("create document");
        file.write_all(&vec![b'x'; MemoryContent::MAX_BYTES])
            .expect("write maximum document");
        file.flush().expect("flush document");
        assert_eq!(
            read_document_bounded(&path)
                .expect("maximum document")
                .len(),
            MemoryContent::MAX_BYTES
        );
    }
}
