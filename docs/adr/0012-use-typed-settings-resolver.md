# ADR-0012: Use a versioned typed settings resolver with layered precedence

**Status:** Accepted

**Date:** 2026-08-22

**Owners:** Project maintainers

## Context and problem statement

Runtime configuration is currently environment-driven only, and the terminal has no named provider profiles or persistent settings.
Phase 3.3 requires settings that survive restart, remain inspectable, and stay safe when model-writable workspace files exist in the same tree.
Without one resolver, each feature would invent its own configuration source and the effective value of a setting would become unexplainable to the user.

## Decision drivers

- Every effective setting must be explainable: the user can see which layer supplied it.
- Precedence must be total and deterministic for identical inputs.
- Model-writable workspace files must not weaken credential, permission, sandbox, retention, or telemetry policy.
- Malformed user configuration must degrade safely instead of blocking offline use.
- Settings are non-secret; credentials follow [ADR-0009](0009-use-os-backed-provider-credential-profiles.md).

## Considered options

1. Keep environment-only configuration and add more variables per feature.
2. Adopt an external configuration crate with dynamic layered values.
3. Write a small typed resolver with explicit layers, schema versioning, and per-key allowlists.

## Decision outcome

Chosen option: **a versioned typed settings resolver**, because AutoHarness already treats typed schemas and explicit state as durable product constraints.

The resolver merges five layers in fixed order: built-in defaults, user file, workspace file, environment, and command-line overrides.
Each layer is parsed independently; a malformed layer is skipped with a safe diagnostic instead of failing startup, so AutoHarness remains usable offline.
Resolution produces a fully validated `Settings` value plus a provenance map that records which layer supplied every effective field.

Workspace files may override only keys on an explicit allowlist of presentation and convenience settings.
Credential selection from a workspace file may name a profile but can never supply secret material, weaken permission defaults, disable redaction, change retention or telemetry policy, or alter sandbox boundaries.
Unknown keys, invalid values, and disallowed overrides are reported as safe diagnostics and ignored rather than silently coerced.

Settings serialization carries a schema version.
A future-version file is refused with a clear diagnostic; older supported versions migrate forward through explicit steps before validation.

Provider profiles live inside the same resolved document as named records containing non-secret connection fields, default model and interaction mode, and an opaque credential reference string.
The reference names nothing about key material; resolving it into a live credential happens only through the vault port of ADR-0009 or the documented environment fallback.

## Consequences

### Positive

- One authority answers "why is this setting active" for users and diagnostics.
- Adding a setting is a typed change with compile-time updates at read sites.
- Workspace trust stays bounded by an allowlist reviewed in code, not by file contents.
- Recovery from hand-edited or partially written files is deterministic and diagnosable.

### Negative

- New settings require resolver and documentation changes instead of free-form environment reads.
- Layered parsing is more code than reading single environment variables.
- Diagnostics must be maintained as user-facing surface.

### Follow-up

- Extend the allowlist deliberately; treat additions as contract changes requiring review.
- Add a settings screen consumer before Phase 3.4 builds broader navigation on top.

## Evidence

- Resolver implementation and tests in `crates/autoharness-settings`, including precedence order, malformed-layer recovery, workspace allowlist rejection, schema-version handling, and profile validation.

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0005](0005-use-ephemeral-in-app-credentials.md)
- [ADR-0009](0009-use-os-backed-provider-credential-profiles.md)
