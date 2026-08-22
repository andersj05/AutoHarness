use autoharness_domain::{
    CapabilityKind, PermissionAnswer, PermissionOutcome, ResourceRef, ToolCallSpec, ToolName,
};

use crate::{PlannedToolCall, ToolError, ToolErrorKind};
use autoharness_domain::RetryAdvice;

/// One ordered permission rule scoped by tool, capability, and resource prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionRule {
    /// Optional exact tool-name scope.
    pub tool_name: Option<ToolName>,
    /// Exact capability class.
    pub capability: CapabilityKind,
    /// Canonical resource prefix.
    pub resource_prefix: ResourceRef,
    /// Result when the rule matches.
    pub outcome: PermissionOutcome,
}

/// Deterministic first-match permission policy with fail-closed default.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PermissionPolicy {
    rules: Vec<PermissionRule>,
}

impl PermissionPolicy {
    /// Creates an ordered policy. No match always denies.
    #[must_use]
    pub const fn new(rules: Vec<PermissionRule>) -> Self {
        Self { rules }
    }

    /// Evaluates the exact trusted call and canonical resource.
    #[must_use]
    pub fn evaluate(&self, call: &ToolCallSpec) -> PermissionOutcome {
        self.rules
            .iter()
            .find(|rule| {
                rule.capability == call.capability.kind
                    && rule
                        .tool_name
                        .as_ref()
                        .is_none_or(|name| name == &call.tool_name)
                    && call
                        .capability
                        .resource
                        .as_str()
                        .starts_with(rule.resource_prefix.as_str())
            })
            .map_or(PermissionOutcome::Deny, |rule| rule.outcome)
    }

    /// Safe local default: every registered local capability asks.
    #[must_use]
    pub fn local_default() -> Self {
        let workspace = ResourceRef::new("workspace:").expect("static resource is valid");
        Self::new(vec![
            PermissionRule {
                tool_name: Some(ToolName::new("fs_read").expect("static tool name is valid")),
                capability: CapabilityKind::FilesystemRead,
                resource_prefix: workspace.clone(),
                outcome: PermissionOutcome::Ask,
            },
            PermissionRule {
                tool_name: Some(ToolName::new("fs_write").expect("static tool name is valid")),
                capability: CapabilityKind::FilesystemWrite,
                resource_prefix: workspace,
                outcome: PermissionOutcome::Ask,
            },
            PermissionRule {
                tool_name: Some(ToolName::new("process_run").expect("static tool name is valid")),
                capability: CapabilityKind::ProcessExecute,
                resource_prefix: ResourceRef::new("program:").expect("static resource is valid"),
                outcome: PermissionOutcome::Ask,
            },
            PermissionRule {
                tool_name: Some(ToolName::new("http_request").expect("static tool name is valid")),
                capability: CapabilityKind::HttpRequest,
                resource_prefix: ResourceRef::new("http").expect("static resource is valid"),
                outcome: PermissionOutcome::Ask,
            },
        ])
    }
}

/// Proof consumed by execution after durable permission replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionEvidence {
    /// Trusted policy directly allowed the exact call.
    PolicyAllow,
    /// A human allowed this exact `ask` call once.
    HumanAllowOnce,
}

/// Executable plan whose constructor validates permission evidence.
pub struct AuthorizedToolCall {
    pub(crate) planned: PlannedToolCall,
}

/// Converts a plan to an executable token only with matching durable evidence.
pub fn authorize(
    planned: PlannedToolCall,
    policy_outcome: PermissionOutcome,
    answer: Option<PermissionAnswer>,
) -> Result<(AuthorizedToolCall, PermissionEvidence), ToolError> {
    let evidence = match (policy_outcome, answer) {
        (PermissionOutcome::Allow, None) => PermissionEvidence::PolicyAllow,
        (PermissionOutcome::Ask, Some(PermissionAnswer::AllowOnce)) => {
            PermissionEvidence::HumanAllowOnce
        }
        _ => {
            return Err(ToolError::new(
                ToolErrorKind::PermissionDenied,
                RetryAdvice::Never,
            ));
        }
    };
    Ok((AuthorizedToolCall { planned }, evidence))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use autoharness_domain::{ProviderCallId, ToolArguments, ToolCallId};

    use super::*;
    use crate::{IncomingToolCall, plan};

    #[test]
    fn policy_is_scoped_and_fail_closed() {
        let read = plan(IncomingToolCall {
            tool_call_id: ToolCallId::new("call-1").expect("ID"),
            provider_call_id: ProviderCallId::new("provider-1").expect("ID"),
            tool_name: ToolName::new("fs_read").expect("name"),
            arguments: ToolArguments::new(json!({"path":"README.md"})).expect("arguments"),
        })
        .expect("plan");
        let policy = PermissionPolicy::local_default();
        assert_eq!(policy.evaluate(read.spec()), PermissionOutcome::Ask);

        let mut changed = read.spec().clone();
        changed.tool_name = ToolName::new("unregistered").expect("name");
        assert_eq!(policy.evaluate(&changed), PermissionOutcome::Deny);
    }

    #[test]
    fn ask_cannot_execute_without_matching_human_allowance() {
        let planned = plan(IncomingToolCall {
            tool_call_id: ToolCallId::new("call-1").expect("ID"),
            provider_call_id: ProviderCallId::new("provider-1").expect("ID"),
            tool_name: ToolName::new("fs_write").expect("name"),
            arguments: ToolArguments::new(json!({"path":"a","content":"b"})).expect("arguments"),
        })
        .expect("plan");
        assert!(
            authorize(
                planned,
                PermissionOutcome::Ask,
                Some(PermissionAnswer::Deny)
            )
            .is_err()
        );
    }
}
