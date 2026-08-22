use std::path::{Component, Path, PathBuf};

use autoharness_domain::{
    CapabilityKind, CapabilityRequest, ProviderCallId, ResourceRef, TOOL_SCHEMA_V1, ToolArguments,
    ToolCallId, ToolCallSpec, ToolName,
};
use reqwest::{Method, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{ToolError, ToolErrorKind};
use autoharness_domain::RetryAdvice;

const MAX_PROCESS_ARGUMENTS: usize = 256;
const MAX_PROCESS_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;

/// One versioned definition exposed to provider adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolDefinition {
    /// Stable registered name.
    pub name: ToolName,
    /// Human-readable purpose.
    pub description: &'static str,
    /// Supported schema version.
    pub schema_version: u16,
    /// JSON Schema object sent to models.
    pub parameters: Value,
}

/// Provider call data before the trusted registry derives authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingToolCall {
    /// Local identity allocated before durable admission.
    pub tool_call_id: ToolCallId,
    /// Provider identity required for result continuation.
    pub provider_call_id: ProviderCallId,
    /// Model-selected registered name.
    pub tool_name: ToolName,
    /// Model-authored arguments.
    pub arguments: ToolArguments,
}

/// Trusted parsed operation retained only in process memory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Operation {
    RejectInvalid,
    ReadFile {
        path: PathBuf,
    },
    WriteFile {
        path: PathBuf,
        content: Vec<u8>,
    },
    RunProcess {
        program: String,
        arguments: Vec<String>,
        cwd: PathBuf,
    },
    HttpRequest {
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
    },
}

/// Frozen durable call specification plus its trusted executable plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedToolCall {
    spec: ToolCallSpec,
    pub(crate) operation: Operation,
}

/// One trusted operation-specific field shown before a human permission answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionDetail {
    /// Stable human-readable field label.
    pub label: &'static str,
    /// Exact or conservatively summarized value required for an informed decision.
    pub value: String,
}

impl PlannedToolCall {
    /// Returns the durable model call and exact derived authority.
    #[must_use]
    pub const fn spec(&self) -> &ToolCallSpec {
        &self.spec
    }

    pub(crate) fn into_parts(self) -> (ToolCallSpec, Operation) {
        (self.spec, self.operation)
    }

    pub(crate) const fn rejects_execution(&self) -> bool {
        matches!(self.operation, Operation::RejectInvalid)
    }
}

/// Returns the complete built-in v1 schema registry.
#[must_use]
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        definition(
            "fs_read",
            "Read one UTF-8 or binary file relative to the workspace root.",
            json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"],
                "additionalProperties":false
            }),
        ),
        definition(
            "fs_write",
            "Create or replace one UTF-8 file relative to the workspace root.",
            json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"content":{"type":"string"}},
                "required":["path","content"],
                "additionalProperties":false
            }),
        ),
        definition(
            "process_run",
            "Run one executable directly without a command shell.",
            json!({
                "type":"object",
                "properties":{
                    "program":{"type":"string"},
                    "arguments":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string"}
                },
                "required":["program"],
                "additionalProperties":false
            }),
        ),
        definition(
            "http_request",
            "Issue one bounded HTTP or HTTPS request without following redirects.",
            json!({
                "type":"object",
                "properties":{
                    "method":{"type":"string","enum":["GET","POST","PUT","PATCH","DELETE","HEAD"]},
                    "url":{"type":"string"},
                    "body":{"type":"string"}
                },
                "required":["method","url"],
                "additionalProperties":false
            }),
        ),
    ]
}

/// Strictly parses one registered schema and derives its exact capability.
///
/// Unknown or invalid model calls become a deterministic no-authority plan so they can be
/// rejected durably and returned to the provider for bounded repair.
pub fn plan(incoming: IncomingToolCall) -> Result<PlannedToolCall, ToolError> {
    let name = incoming.tool_name.as_str();
    let value = incoming.arguments.to_value();
    let (operation, capability) = plan_supported(name, value).unwrap_or_else(|_| {
        (
            Operation::RejectInvalid,
            CapabilityRequest {
                kind: CapabilityKind::InvalidToolCall,
                resource: ResourceRef::new("tool-call:invalid")
                    .expect("static invalid-call resource is valid"),
            },
        )
    });

    Ok(PlannedToolCall {
        spec: ToolCallSpec {
            tool_call_id: incoming.tool_call_id,
            provider_call_id: incoming.provider_call_id,
            tool_name: incoming.tool_name,
            schema_version: TOOL_SCHEMA_V1,
            arguments: incoming.arguments,
            capability,
        },
        operation,
    })
}

fn plan_supported(name: &str, value: Value) -> Result<(Operation, CapabilityRequest), ToolError> {
    let planned = match name {
        "fs_read" => {
            let arguments: FileReadArguments = parse(value)?;
            let path = workspace_path(&arguments.path)?;
            let resource = workspace_resource(&path)?;
            (
                Operation::ReadFile { path },
                CapabilityRequest {
                    kind: CapabilityKind::FilesystemRead,
                    resource,
                },
            )
        }
        "fs_write" => {
            let arguments: FileWriteArguments = parse(value)?;
            let path = workspace_path(&arguments.path)?;
            let resource = workspace_resource(&path)?;
            (
                Operation::WriteFile {
                    path,
                    content: arguments.content.into_bytes(),
                },
                CapabilityRequest {
                    kind: CapabilityKind::FilesystemWrite,
                    resource,
                },
            )
        }
        "process_run" => {
            let arguments: ProcessArguments = parse(value)?;
            validate_program(&arguments.program)?;
            if arguments.arguments.len() > MAX_PROCESS_ARGUMENTS
                || arguments.arguments.iter().map(String::len).sum::<usize>()
                    > MAX_PROCESS_ARGUMENT_BYTES
            {
                return Err(invalid_call());
            }
            let cwd = workspace_path(arguments.cwd.as_deref().unwrap_or("."))?;
            let resource = ResourceRef::new(format!(
                "program:{}@workspace:{}",
                arguments.program,
                display_relative(&cwd)
            ))
            .map_err(|_| invalid_call())?;
            (
                Operation::RunProcess {
                    program: arguments.program,
                    arguments: arguments.arguments,
                    cwd,
                },
                CapabilityRequest {
                    kind: CapabilityKind::ProcessExecute,
                    resource,
                },
            )
        }
        "http_request" => {
            let arguments: HttpArguments = parse(value)?;
            let method =
                Method::from_bytes(arguments.method.as_bytes()).map_err(|_| invalid_call())?;
            if !matches!(
                method,
                Method::GET
                    | Method::POST
                    | Method::PUT
                    | Method::PATCH
                    | Method::DELETE
                    | Method::HEAD
            ) {
                return Err(invalid_call());
            }
            let url = Url::parse(&arguments.url).map_err(|_| invalid_call())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.username() != ""
                || url.password().is_some()
                || url.host_str().is_none()
            {
                return Err(invalid_call());
            }
            let body = arguments.body.map(String::into_bytes);
            if body
                .as_ref()
                .is_some_and(|body| body.len() > MAX_REQUEST_BODY_BYTES)
            {
                return Err(invalid_call());
            }
            let origin = url.origin().ascii_serialization();
            (
                Operation::HttpRequest { method, url, body },
                CapabilityRequest {
                    kind: CapabilityKind::HttpRequest,
                    resource: ResourceRef::new(origin).map_err(|_| invalid_call())?,
                },
            )
        }
        _ => return Err(invalid_call()),
    };
    Ok(planned)
}

/// Rebuilds a trusted in-memory plan from a durable frozen call and rejects drift.
pub fn replan(spec: ToolCallSpec) -> Result<PlannedToolCall, ToolError> {
    if spec.schema_version != TOOL_SCHEMA_V1 {
        return Err(invalid_call());
    }
    let planned = plan(IncomingToolCall {
        tool_call_id: spec.tool_call_id.clone(),
        provider_call_id: spec.provider_call_id.clone(),
        tool_name: spec.tool_name.clone(),
        arguments: spec.arguments.clone(),
    })?;
    if planned.spec != spec {
        return Err(invalid_call());
    }
    Ok(planned)
}

/// Rebuilds a frozen call and returns its security-critical permission fields.
pub fn permission_details(spec: &ToolCallSpec) -> Result<Vec<PermissionDetail>, ToolError> {
    let planned = replan(spec.clone())?;
    let details = match planned.operation {
        Operation::RejectInvalid => Vec::new(),
        Operation::ReadFile { path } => vec![PermissionDetail {
            label: "Path",
            value: display_relative(&path),
        }],
        Operation::WriteFile { path, content } => vec![
            PermissionDetail {
                label: "Path",
                value: display_relative(&path),
            },
            PermissionDetail {
                label: "Content bytes",
                value: content.len().to_string(),
            },
        ],
        Operation::RunProcess {
            program,
            arguments,
            cwd,
        } => {
            let mut details = Vec::with_capacity(arguments.len().saturating_add(3));
            details.push(PermissionDetail {
                label: "Program",
                value: program,
            });
            details.push(PermissionDetail {
                label: "Working directory",
                value: display_relative(&cwd),
            });
            if arguments.is_empty() {
                details.push(PermissionDetail {
                    label: "Arguments",
                    value: "(none)".to_owned(),
                });
            } else {
                details.extend(arguments.into_iter().enumerate().map(|(index, value)| {
                    PermissionDetail {
                        label: "Argument",
                        value: format!("{}: {value}", index.saturating_add(1)),
                    }
                }));
            }
            details
        }
        Operation::HttpRequest { method, url, body } => vec![
            PermissionDetail {
                label: "Method",
                value: method.to_string(),
            },
            PermissionDetail {
                label: "URL",
                value: url.to_string(),
            },
            PermissionDetail {
                label: "Body bytes",
                value: body.as_ref().map_or(0, Vec::len).to_string(),
            },
        ],
    };
    Ok(details)
}

fn definition(name: &str, description: &'static str, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        name: ToolName::new(name).expect("static tool name is valid"),
        description,
        schema_version: TOOL_SCHEMA_V1,
        parameters,
    }
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(|_| invalid_call())
}

fn workspace_path(value: &str) -> Result<PathBuf, ToolError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(invalid_call());
    }
    let path = Path::new(value);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(invalid_call());
            }
        }
    }
    Ok(normalized)
}

fn workspace_resource(path: &Path) -> Result<ResourceRef, ToolError> {
    ResourceRef::new(format!("workspace:{}", display_relative(path))).map_err(|_| invalid_call())
}

fn display_relative(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_owned();
    }
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_program(program: &str) -> Result<(), ToolError> {
    let normalized = program.to_ascii_lowercase();
    let is_batch = normalized.ends_with(".bat") || normalized.ends_with(".cmd");
    let is_shell = matches!(
        normalized.as_str(),
        "sh" | "bash"
            | "dash"
            | "zsh"
            | "ksh"
            | "fish"
            | "cmd"
            | "cmd.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
    );
    if program.is_empty()
        || program.len() > 255
        || program.chars().any(char::is_whitespace)
        || program.contains(['/', '\\'])
        || is_shell
        || is_batch
        || !program.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(invalid_call());
    }
    Ok(())
}

fn invalid_call() -> ToolError {
    ToolError::new(ToolErrorKind::InvalidCall, RetryAdvice::Never)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReadArguments {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileWriteArguments {
    path: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessArguments {
    program: String,
    #[serde(default)]
    arguments: Vec<String>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpArguments {
    method: String,
    url: String,
    body: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(name: &str, arguments: Value) -> IncomingToolCall {
        IncomingToolCall {
            tool_call_id: ToolCallId::new("tool-1").expect("ID"),
            provider_call_id: ProviderCallId::new("provider-1").expect("ID"),
            tool_name: ToolName::new(name).expect("name"),
            arguments: ToolArguments::new(arguments).expect("object"),
        }
    }

    #[test]
    fn call_shape_cannot_change_derived_authority() {
        let call = plan(incoming(
            "fs_write",
            json!({"path":"src/lib.rs","content":"x"}),
        ))
        .expect("valid call");
        assert_eq!(call.spec().capability.kind, CapabilityKind::FilesystemWrite);
        assert_eq!(
            call.spec().capability.resource.as_str(),
            "workspace:src/lib.rs"
        );
        for rejected in [
            incoming("fs_read", json!({"path":"../secret"})),
            incoming("fs_read", json!({"path":"ok","extra":true})),
            incoming("unknown_tool", json!({"value":true})),
        ] {
            let rejected = plan(rejected).expect("invalid calls become no-authority plans");
            assert_eq!(
                rejected.spec().capability.kind,
                CapabilityKind::InvalidToolCall
            );
            assert_eq!(
                rejected.spec().capability.resource.as_str(),
                "tool-call:invalid"
            );
        }
    }

    #[test]
    fn process_never_accepts_a_shell_command_as_program() {
        let command = plan(incoming("process_run", json!({"program":"cmd /c whoami"})))
            .expect("rejected plan");
        let shell = plan(incoming(
            "process_run",
            json!({"program":"sh","arguments":["-c","whoami"]}),
        ))
        .expect("rejected plan");
        assert_eq!(
            command.spec().capability.kind,
            CapabilityKind::InvalidToolCall
        );
        assert_eq!(
            shell.spec().capability.kind,
            CapabilityKind::InvalidToolCall
        );
        assert!(
            plan(incoming(
                "process_run",
                json!({"program":"cargo","arguments":["check"]})
            ))
            .is_ok()
        );
    }

    #[test]
    fn permission_details_include_exact_process_and_http_actions() {
        let process = plan(incoming(
            "process_run",
            json!({"program":"cargo","arguments":["test","--locked"],"cwd":"crates"}),
        ))
        .expect("process plan");
        let process_details = permission_details(process.spec()).expect("process details");
        assert!(
            process_details
                .iter()
                .any(|detail| detail.value == "1: test")
        );
        assert!(
            process_details
                .iter()
                .any(|detail| detail.value == "2: --locked")
        );

        let http = plan(incoming(
            "http_request",
            json!({"method":"DELETE","url":"https://example.com/items/7?force=true","body":"confirm"}),
        ))
        .expect("HTTP plan");
        let http_details = permission_details(http.spec()).expect("HTTP details");
        assert!(http_details.iter().any(|detail| detail.value == "DELETE"));
        assert!(
            http_details
                .iter()
                .any(|detail| { detail.value == "https://example.com/items/7?force=true" })
        );
        assert!(http_details.iter().any(|detail| detail.value == "7"));
    }

    #[test]
    fn windows_batch_programs_are_rejected_on_every_platform() {
        for program in ["build.bat", "build.cmd"] {
            let planned = plan(incoming("process_run", json!({"program":program})))
                .expect("batch call becomes no-authority plan");
            assert_eq!(
                planned.spec().capability.kind,
                CapabilityKind::InvalidToolCall
            );
        }
        assert!(plan(incoming("process_run", json!({"program":"cargo"}))).is_ok());
    }
}
