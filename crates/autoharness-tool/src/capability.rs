use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use futures_util::StreamExt as _;
use reqwest::{Client, Method, Url, redirect};
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::{ToolError, ToolErrorKind};
use autoharness_domain::RetryAdvice;

/// Raw bounded process result from the process capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessResult {
    /// Platform exit code when one was available.
    pub exit_code: Option<i32>,
    /// Captured standard output prefix.
    pub stdout: Vec<u8>,
    /// Captured standard error prefix.
    pub stderr: Vec<u8>,
    /// Whether either stream exceeded its capture prefix.
    pub truncated: bool,
}

/// Raw bounded HTTP result from the network capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResult {
    /// Response status.
    pub status: u16,
    /// Captured response body.
    pub body: Vec<u8>,
}

/// Workspace-confined filesystem capability.
#[async_trait]
pub trait FilesystemCapability: Send + Sync {
    /// Reads one bounded relative path.
    async fn read(
        &self,
        path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ToolError>;

    /// Creates or replaces one relative path.
    async fn write(
        &self,
        path: &Path,
        content: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ToolError>;
}

/// Shell-free child-process capability.
#[async_trait]
pub trait ProcessCapability: Send + Sync {
    /// Runs one executable directly with an exact argument vector.
    async fn run(
        &self,
        program: &str,
        arguments: &[String],
        cwd: &Path,
        cancellation: &CancellationToken,
    ) -> Result<ProcessResult, ToolError>;
}

/// No-redirect bounded HTTP capability.
#[async_trait]
pub trait HttpCapability: Send + Sync {
    /// Sends one request to the exact planned URL.
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        cancellation: &CancellationToken,
    ) -> Result<HttpResult, ToolError>;
}

/// Local workspace filesystem adapter.
pub struct LocalFilesystem {
    root: PathBuf,
    max_read_bytes: usize,
}

impl LocalFilesystem {
    /// Opens a canonical workspace root and fixes a per-read hard bound.
    pub fn new(root: impl AsRef<Path>, max_read_bytes: usize) -> Result<Self, ToolError> {
        if max_read_bytes == 0 {
            return Err(internal());
        }
        let root = std::fs::canonicalize(root).map_err(|_| filesystem_error())?;
        if !root.is_dir() {
            return Err(filesystem_error());
        }
        Ok(Self {
            root,
            max_read_bytes,
        })
    }

    fn read_path(&self, path: &Path) -> Result<PathBuf, ToolError> {
        let resolved =
            std::fs::canonicalize(self.root.join(path)).map_err(|_| filesystem_error())?;
        if !resolved.starts_with(&self.root) || !resolved.is_file() {
            return Err(filesystem_error());
        }
        Ok(resolved)
    }

    fn write_path(&self, path: &Path) -> Result<PathBuf, ToolError> {
        let candidate = self.root.join(path);
        let parent = candidate.parent().ok_or_else(filesystem_error)?;
        let parent = std::fs::canonicalize(parent).map_err(|_| filesystem_error())?;
        if !parent.starts_with(&self.root) {
            return Err(filesystem_error());
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&candidate)
            && metadata.file_type().is_symlink()
        {
            return Err(filesystem_error());
        }
        Ok(candidate)
    }

    fn directory_path(&self, path: &Path) -> Result<PathBuf, ToolError> {
        let resolved =
            std::fs::canonicalize(self.root.join(path)).map_err(|_| filesystem_error())?;
        if !resolved.starts_with(&self.root) || !resolved.is_dir() {
            return Err(filesystem_error());
        }
        Ok(resolved)
    }
}

#[async_trait]
impl FilesystemCapability for LocalFilesystem {
    async fn read(
        &self,
        path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ToolError> {
        let path = self.read_path(path)?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| filesystem_error())?;
        read_hard_bounded(
            file,
            self.max_read_bytes,
            cancellation,
            ToolErrorKind::Filesystem,
        )
        .await
    }

    async fn write(
        &self,
        path: &Path,
        content: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ToolError> {
        let path = self.write_path(path)?;
        tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(cancelled()),
            result = tokio::fs::write(path, content) => {
                result.map_err(|_| filesystem_error())?;
                Ok(format!("wrote {} bytes", content.len()).into_bytes())
            }
        }
    }
}

/// Local shell-free process adapter confined to a canonical workspace directory.
pub struct LocalProcess {
    filesystem: LocalFilesystem,
    max_stream_bytes: usize,
}

impl LocalProcess {
    /// Creates a process capability with bounded stdout and stderr prefixes.
    pub fn new(root: impl AsRef<Path>, max_stream_bytes: usize) -> Result<Self, ToolError> {
        Ok(Self {
            filesystem: LocalFilesystem::new(root, max_stream_bytes)?,
            max_stream_bytes,
        })
    }
}

#[async_trait]
impl ProcessCapability for LocalProcess {
    async fn run(
        &self,
        program: &str,
        arguments: &[String],
        cwd: &Path,
        cancellation: &CancellationToken,
    ) -> Result<ProcessResult, ToolError> {
        let cwd = self.filesystem.directory_path(cwd)?;
        let mut child = Command::new(program)
            .args(arguments)
            .current_dir(cwd)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| process_error())?;
        let stdout = child.stdout.take().ok_or_else(internal)?;
        let stderr = child.stderr.take().ok_or_else(internal)?;
        let stdout_task = tokio::spawn(read_prefix(stdout, self.max_stream_bytes));
        let stderr_task = tokio::spawn(read_prefix(stderr, self.max_stream_bytes));
        let status = tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stdout_task.abort();
                stderr_task.abort();
                return Err(cancelled());
            }
            status = child.wait() => status.map_err(|_| process_error())?,
        };
        let (stdout, stdout_truncated) = stdout_task.await.map_err(|_| process_error())??;
        let (stderr, stderr_truncated) = stderr_task.await.map_err(|_| process_error())??;
        Ok(ProcessResult {
            exit_code: status.code(),
            stdout,
            stderr,
            truncated: stdout_truncated || stderr_truncated,
        })
    }
}

/// Reqwest-based no-redirect HTTP adapter.
pub struct LocalHttp {
    client: Client,
    max_response_bytes: usize,
}

impl LocalHttp {
    /// Creates an adapter with redirects disabled and a hard response bound.
    pub fn new(max_response_bytes: usize) -> Result<Self, ToolError> {
        if max_response_bytes == 0 {
            return Err(internal());
        }
        let client = Client::builder()
            .redirect(redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| internal())?;
        Ok(Self {
            client,
            max_response_bytes,
        })
    }
}

#[async_trait]
impl HttpCapability for LocalHttp {
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        cancellation: &CancellationToken,
    ) -> Result<HttpResult, ToolError> {
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled()),
            response = request.send() => response.map_err(|_| http_error())?,
        };
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => return Err(cancelled()),
                next = stream.next() => next,
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|_| http_error())?;
            if body.len().saturating_add(chunk.len()) > self.max_response_bytes {
                return Err(ToolError::new(
                    ToolErrorKind::OutputLimit,
                    RetryAdvice::Never,
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResult { status, body })
    }
}

async fn read_hard_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
    cancellation: &CancellationToken,
    kind: ToolErrorKind,
) -> Result<Vec<u8>, ToolError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled()),
            result = reader.read(&mut buffer) => result.map_err(|_| ToolError::new(kind, RetryAdvice::Never))?,
        };
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > limit {
            return Err(ToolError::new(
                ToolErrorKind::OutputLimit,
                RetryAdvice::Never,
            ));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

async fn read_prefix<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> Result<(Vec<u8>, bool), ToolError> {
    let mut output = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|_| process_error())?;
        if read == 0 {
            return Ok((output, truncated));
        }
        let remaining = limit.saturating_sub(output.len());
        let retained = remaining.min(read);
        output.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
}

fn filesystem_error() -> ToolError {
    ToolError::new(ToolErrorKind::Filesystem, RetryAdvice::Never)
}

fn process_error() -> ToolError {
    ToolError::new(ToolErrorKind::Process, RetryAdvice::Never)
}

fn http_error() -> ToolError {
    ToolError::new(ToolErrorKind::Http, RetryAdvice::Backoff)
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

    #[tokio::test]
    async fn filesystem_rejects_parent_escape_and_bounds_reads() {
        let directory = tempfile::tempdir().expect("directory");
        std::fs::write(directory.path().join("small"), b"abc").expect("fixture");
        let capability = LocalFilesystem::new(directory.path(), 2).expect("capability");
        let cancellation = CancellationToken::new();

        assert!(
            capability
                .read(Path::new("small"), &cancellation)
                .await
                .is_err()
        );
        assert!(
            capability
                .read(Path::new("../small"), &cancellation)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cancelled_process_is_observed_as_cancelled() {
        let directory = tempfile::tempdir().expect("directory");
        let capability = LocalProcess::new(directory.path(), 1024).expect("capability");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = capability
            .run(
                if cfg!(windows) { "cmd" } else { "sh" },
                &[],
                Path::new("."),
                &cancellation,
            )
            .await;
        assert!(result.is_err_and(|error| error.kind() == ToolErrorKind::Cancelled));
    }

    #[tokio::test]
    async fn process_receives_no_ambient_parent_environment() {
        let directory = tempfile::tempdir().expect("directory");
        let capability = LocalProcess::new(directory.path(), 64 * 1024).expect("capability");
        let cancellation = CancellationToken::new();
        let (program, arguments) = if cfg!(windows) {
            assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
            let system_root = std::env::var("SystemRoot").expect("Windows system root");
            (
                PathBuf::from(system_root)
                    .join("System32")
                    .join("cmd.exe")
                    .to_string_lossy()
                    .into_owned(),
                vec![
                    "/D".to_owned(),
                    "/C".to_owned(),
                    "set CARGO_MANIFEST_DIR".to_owned(),
                ],
            )
        } else {
            let program = if Path::new("/usr/bin/env").exists() {
                "/usr/bin/env"
            } else {
                "/bin/env"
            };
            (program.to_owned(), Vec::new())
        };

        let result = capability
            .run(&program, &arguments, Path::new("."), &cancellation)
            .await
            .expect("environment inspection process");
        assert!(
            result.stdout.is_empty(),
            "child must not inherit the parent environment"
        );
        if !cfg!(windows) {
            assert!(result.stderr.is_empty());
        }
    }
}
