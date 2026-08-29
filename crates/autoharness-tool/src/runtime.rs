use std::sync::Arc;
use std::time::Duration;

use autoharness_domain::{
    MAX_INLINE_TOOL_OUTPUT_BYTES, PermissionAnswer, PermissionOutcome, ToolCallSpec, ToolOutput,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::schema::Operation;
use crate::{
    ArtifactStore, AuthorizedToolCall, FilesystemCapability, HttpCapability, PermissionEvidence,
    PermissionPolicy, PlannedToolCall, ProcessCapability, ToolError, ToolErrorKind, authorize,
    replan,
};
use autoharness_domain::RetryAdvice;

/// Composed capability runtime with bounded concurrency, deadline, output, and artifacts.
pub struct ToolRuntime {
    filesystem: Arc<dyn FilesystemCapability>,
    process: Arc<dyn ProcessCapability>,
    http: Arc<dyn HttpCapability>,
    artifacts: Arc<dyn ArtifactStore>,
    policy: PermissionPolicy,
    concurrency: Arc<Semaphore>,
    timeout: Duration,
    inline_output_bytes: usize,
}

impl ToolRuntime {
    /// Composes independently testable capability ports.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        filesystem: Arc<dyn FilesystemCapability>,
        process: Arc<dyn ProcessCapability>,
        http: Arc<dyn HttpCapability>,
        artifacts: Arc<dyn ArtifactStore>,
        policy: PermissionPolicy,
        max_concurrency: usize,
        timeout: Duration,
        inline_output_bytes: usize,
    ) -> Result<Self, ToolError> {
        if max_concurrency == 0
            || timeout.is_zero()
            || inline_output_bytes == 0
            || inline_output_bytes > MAX_INLINE_TOOL_OUTPUT_BYTES
        {
            return Err(internal());
        }
        Ok(Self {
            filesystem,
            process,
            http,
            artifacts,
            policy,
            concurrency: Arc::new(Semaphore::new(max_concurrency)),
            timeout,
            inline_output_bytes,
        })
    }

    /// Returns the deterministic policy result for a trusted frozen plan.
    #[must_use]
    pub fn evaluate(&self, planned: &PlannedToolCall) -> PermissionOutcome {
        self.policy.evaluate(planned.spec())
    }

    /// Reconstructs an executable token only from matching durable permission state.
    pub fn authorize_replayed(
        &self,
        spec: ToolCallSpec,
        policy_outcome: PermissionOutcome,
        answer: Option<PermissionAnswer>,
    ) -> Result<(AuthorizedToolCall, PermissionEvidence), ToolError> {
        authorize(replan(spec)?, policy_outcome, answer)
    }

    /// Executes one authorized call through only its derived capability.
    pub async fn execute(
        &self,
        authorized: AuthorizedToolCall,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let permit = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled()),
            permit = self.concurrency.clone().acquire_owned() => permit.map_err(|_| internal())?,
        };
        let (_, operation) = authorized.planned.into_parts();
        let execution = self.execute_operation(operation, &cancellation);
        let bytes = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(self.timeout, execution) => {
                result.map_err(|_| ToolError::new(ToolErrorKind::Timeout, RetryAdvice::Never))?
            }
        }?;
        drop(permit);
        self.capture(bytes).await
    }

    async fn execute_operation(
        &self,
        operation: Operation,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ToolError> {
        match operation {
            Operation::RejectInvalid => Err(ToolError::new(
                ToolErrorKind::InvalidCall,
                RetryAdvice::Never,
            )),
            Operation::SubmitMemoryProposal { .. } => Err(ToolError::new(
                ToolErrorKind::MemoryProposalSinkRequired,
                RetryAdvice::Never,
            )),
            Operation::ReadFile { path } => self.filesystem.read(&path, cancellation).await,
            Operation::WriteFile { path, content } => {
                self.filesystem.write(&path, &content, cancellation).await
            }
            Operation::RunProcess {
                program,
                arguments,
                cwd,
            } => {
                let result = self
                    .process
                    .run(&program, &arguments, &cwd, cancellation)
                    .await?;
                let mut output = format!(
                    "exit_code={}\nstdout:\n",
                    result
                        .exit_code
                        .map_or_else(|| "unknown".to_owned(), |code| code.to_string())
                )
                .into_bytes();
                output.extend_from_slice(&result.stdout);
                output.extend_from_slice(b"\nstderr:\n");
                output.extend_from_slice(&result.stderr);
                if result.truncated {
                    output.extend_from_slice(b"\n[process output truncated at capability bound]");
                }
                Ok(output)
            }
            Operation::HttpRequest { method, url, body } => {
                let result = self.http.request(method, url, body, cancellation).await?;
                let mut output = format!("status={}\n", result.status).into_bytes();
                output.extend_from_slice(&result.body);
                Ok(output)
            }
        }
    }

    async fn capture(&self, bytes: Vec<u8>) -> Result<ToolOutput, ToolError> {
        let original_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let content = String::from_utf8_lossy(&bytes);
        if bytes.len() <= self.inline_output_bytes {
            return ToolOutput::new(content.into_owned(), None, original_bytes, false)
                .map_err(|_| internal());
        }
        let artifact = self
            .artifacts
            .put(&bytes, "application/octet-stream")
            .await?;
        let mut boundary = self.inline_output_bytes.min(content.len());
        while boundary > 0 && !content.is_char_boundary(boundary) {
            boundary -= 1;
        }
        ToolOutput::new(
            content[..boundary].to_owned(),
            Some(artifact),
            original_bytes,
            true,
        )
        .map_err(|_| internal())
    }
}

fn cancelled() -> ToolError {
    ToolError::new(ToolErrorKind::Cancelled, RetryAdvice::Never)
}

fn internal() -> ToolError {
    ToolError::new(ToolErrorKind::Internal, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoharness_domain::{
        PermissionAnswer, ProviderCallId, ToolArguments, ToolCallId, ToolName,
    };
    use serde_json::json;

    use crate::{
        FileArtifactStore, IncomingToolCall, LocalFilesystem, LocalHttp, LocalProcess,
        PermissionPolicy, plan,
    };

    #[tokio::test]
    async fn authorized_read_uses_capability_and_retains_large_output_as_artifact() {
        let directory = tempfile::tempdir().expect("directory");
        std::fs::write(directory.path().join("large.txt"), "abcdefghij").expect("fixture");
        let artifacts = directory.path().join("artifacts");
        std::fs::create_dir(&artifacts).expect("artifact directory");
        let runtime = ToolRuntime::new(
            Arc::new(LocalFilesystem::new(directory.path(), 1024).expect("filesystem")),
            Arc::new(LocalProcess::new(directory.path(), 1024).expect("process")),
            Arc::new(LocalHttp::new(1024).expect("HTTP")),
            Arc::new(FileArtifactStore::new(&artifacts).expect("artifacts")),
            PermissionPolicy::local_default(),
            1,
            Duration::from_secs(1),
            4,
        )
        .expect("runtime");
        let planned = plan(IncomingToolCall {
            tool_call_id: ToolCallId::new("call-1").expect("call ID"),
            provider_call_id: ProviderCallId::new("provider-1").expect("provider ID"),
            tool_name: ToolName::new("fs_read").expect("tool name"),
            arguments: ToolArguments::new(json!({"path":"large.txt"})).expect("arguments"),
        })
        .expect("plan");
        let outcome = runtime.evaluate(&planned);
        let (authorized, evidence) =
            authorize(planned, outcome, Some(PermissionAnswer::AllowOnce)).expect("authorization");
        assert_eq!(evidence, PermissionEvidence::HumanAllowOnce);

        let output = runtime
            .execute(authorized, CancellationToken::new())
            .await
            .expect("execution");

        assert_eq!(output.content(), "abcd");
        assert!(output.truncated());
        assert_eq!(output.original_bytes(), 10);
        assert!(output.artifact().is_some());
        assert_eq!(std::fs::read_dir(artifacts).expect("artifacts").count(), 1);
    }

    #[tokio::test]
    async fn memory_proposal_cannot_report_success_without_application_sink() {
        let directory = tempfile::tempdir().expect("directory");
        let artifacts = directory.path().join("artifacts");
        std::fs::create_dir(&artifacts).expect("artifact directory");
        let runtime = ToolRuntime::new(
            Arc::new(LocalFilesystem::new(directory.path(), 1024).expect("filesystem")),
            Arc::new(LocalProcess::new(directory.path(), 1024).expect("process")),
            Arc::new(LocalHttp::new(1024).expect("HTTP")),
            Arc::new(FileArtifactStore::new(&artifacts).expect("artifacts")),
            PermissionPolicy::local_default(),
            1,
            Duration::from_secs(1),
            1024,
        )
        .expect("runtime");
        let planned = plan(IncomingToolCall {
            tool_call_id: ToolCallId::new("call-memory").expect("call ID"),
            provider_call_id: ProviderCallId::new("provider-memory").expect("provider ID"),
            tool_name: ToolName::new("memory_propose").expect("tool name"),
            arguments: ToolArguments::new(json!({
                "content":"The build uses Rust 2024.",
                "kind":"fact",
                "scope":"workspace",
                "sensitivity":"public"
            }))
            .expect("arguments"),
        })
        .expect("plan");
        assert!(planned.memory_proposal().is_some());
        let outcome = runtime.evaluate(&planned);
        let (authorized, _) = authorize(planned, outcome, None).expect("policy authorization");

        let error = runtime
            .execute(authorized, CancellationToken::new())
            .await
            .expect_err("application sink is required for a successful proposal");

        assert_eq!(error.kind(), ToolErrorKind::MemoryProposalSinkRequired);
        assert_eq!(
            error.to_string(),
            "The memory proposal requires the application review sink"
        );
        let failure = error.durable_failure();
        assert_eq!(failure.code().as_str(), "memory_proposal_sink_required");
        assert_eq!(
            failure.message().as_str(),
            "The memory proposal requires the application review sink"
        );
        assert!(!failure.message().as_str().contains("Rust 2024"));
        assert_eq!(std::fs::read_dir(artifacts).expect("artifacts").count(), 0);
    }
}
