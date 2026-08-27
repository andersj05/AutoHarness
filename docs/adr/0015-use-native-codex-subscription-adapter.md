# ADR-0015: Use a native Codex subscription adapter

**Status:** Accepted

**Date:** 2026-08-26

**Owners:** Project maintainers

## Context and problem statement

ADR-0014 required a separately installed and authenticated Codex CLI.
That boundary fails the product requirement that selecting Codex and pressing Enter opens browser sign-in directly from AutoHarness.
It also makes subscription access unavailable on machines where the `codex` executable is not installed, even though AutoHarness already owns the provider experience.

OpenAI documents the user-facing Codex browser authentication sequence and ChatGPT subscription access.
The open-source oh-my-pi project provides a working reference for a native PKCE loopback flow and the ChatGPT Codex Responses transport.
These protocol details are compatibility contracts rather than a published third-party OAuth registration standard, so they must remain isolated inside one provider adapter and covered by fail-closed tests.

## Decision drivers

- Make Enter open the user's default browser without an external CLI dependency.
- Keep the provider-neutral engine and TUI free of OAuth and native wire details.
- Store refreshable credentials only in the operating-system credential vault.
- Validate OAuth state, callback bounds, response bounds, and streaming events before use.
- Preserve cancellable login and explicit failure states in the terminal.

## Considered options

1. Keep requiring the separately installed Codex CLI from ADR-0014.
2. Require an OpenAI Platform API key through the compatible-router adapter.
3. Own the PKCE browser callback, vault persistence, token refresh, and Codex Responses transport inside the Codex provider adapter.

## Decision outcome

Chosen option: **own the native Codex subscription flow inside the provider adapter**.

The application coordinator starts a PKCE S256 authorization flow, opens the default browser, receives the fixed loopback callback, validates the state, exchanges the code, and stores one opaque credential payload through the existing operating-system vault boundary.
Only an opaque profile reference is written to settings.
The provider refreshes expiring credentials and replaces the same vault entry.
The vault adapter hides platform entry-size limits with bounded generation-scoped chunks behind that one logical reference.

The provider sends stateless streaming requests to the ChatGPT Codex Responses endpoint and normalizes bounded SSE events into the provider-neutral stream.
The adapter exposes no Codex tool authority to the engine and redacts access and refresh tokens from errors, traces, and model-visible content.

The persisted provider identifier remains `codex_cli` for settings compatibility even though no Codex executable is required.

## Consequences

### Positive

- Codex sign-in works from the TUI on machines without the Codex CLI.
- Login progress, cancellation, profile creation, activation, and credential persistence share the application's typed coordination path.
- Subscription token refresh survives restart through the operating-system vault.
- The terminal render loop remains free of network and storage work.

### Negative

- AutoHarness now handles sensitive OAuth tokens in zeroizing process memory and the operating-system vault.
- The adapter depends on compatibility behavior that OpenAI has not published as a general third-party OAuth registration contract.
- Protocol changes may require coordinated adapter updates even when the provider-neutral engine is unchanged.

### Follow-up

- Keep OAuth URL, callback, state, credential-redaction, request-shape, fragmented-SSE, cancellation, and PTY browser-handoff tests current.
- Fail closed when authorization claims, token responses, HTTP responses, or stream events are malformed or exceed bounds.
- Revisit the compatibility boundary if OpenAI publishes a supported third-party native-client contract.

## Evidence

- [OpenAI Codex authentication](https://learn.chatgpt.com/docs/auth)
- [oh-my-pi OpenAI Codex OAuth adapter](https://github.com/can1357/oh-my-pi/blob/main/packages/ai/src/registry/oauth/openai-codex.ts)
- [oh-my-pi OpenAI Codex Responses provider](https://github.com/can1357/oh-my-pi/blob/main/packages/ai/src/providers/openai-codex-responses.ts)

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0009](0009-use-os-backed-provider-credential-profiles.md)
- [ADR-0013](0013-use-durable-credential-mutation-recovery.md)
- [ADR-0014](0014-use-codex-cli-subscription-boundary.md)
