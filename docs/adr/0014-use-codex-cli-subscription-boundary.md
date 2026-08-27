# ADR-0014: Use the official Codex CLI subscription boundary

**Status:** Superseded by ADR-0015

**Date:** 2026-08-25

**Owners:** Project maintainers

## Context and problem statement

AutoHarness needs to use a user's ChatGPT Codex subscription without receiving, persisting, or reverse engineering the subscription credentials.

OpenAI documents ChatGPT subscription sign-in only for Codex surfaces such as the official Codex CLI.

The documented command is `codex login`, which completes browser authentication and receives credentials in Codex.

OpenAI does not publish a general-purpose OAuth client-registration, authorization, token, scope, PKCE, or device-authorization protocol for third-party native clients.

## Decision drivers

- Use a supported ChatGPT subscription path.
- Keep subscription tokens outside AutoHarness storage, logs, and memory.
- Preserve the provider-neutral engine boundary.
- Avoid undocumented OpenAI endpoints and credential-file scraping.

## Considered options

1. Implement a native OAuth or device-login client from inferred Codex endpoints.
2. Require an OpenAI Platform API key through the existing compatible-router adapter.
3. Bridge the documented official Codex CLI after its user-owned ChatGPT login.

## Decision outcome

Chosen option: **bridge the documented official Codex CLI after user-owned ChatGPT login**, because it uses the supported subscription surface while leaving authentication, token refresh, storage, and logout with Codex.

The adapter invokes only documented non-interactive Codex CLI execution and machine-readable event interfaces.

It runs with a read-only sandbox and does not expose Codex tool authority as AutoHarness tool authority.

AutoHarness detects authentication through the documented `codex login status` command.

The user completes `codex login` outside the AutoHarness terminal before testing or activating the connection.

## Consequences

### Positive

- ChatGPT Codex subscriptions can serve AutoHarness requests without copying bearer tokens into the AutoHarness vault.
- Codex retains its own supported browser login, automatic refresh, logout, and credential-store behavior.
- The adapter normalizes Codex CLI JSONL output into the existing provider stream.

### Negative

- The Codex CLI must be installed and authenticated on the local machine.
- Codex CLI model availability remains controlled by the authenticated Codex installation.
- The bridge must fail closed when the documented CLI contract is unavailable or malformed.

### Follow-up

- Keep the bridge conformance-tested against structural Codex CLI JSONL fixtures.
- Do not add native subscription OAuth until OpenAI publishes a third-party protocol suitable for this client.

## Evidence

- [OpenAI Codex authentication](https://developers.openai.com/codex/auth)
- [OpenAI Codex developer commands](https://developers.openai.com/codex/cli/reference)
- [Official Codex CLI JSONL event definitions](https://github.com/openai/codex/blob/main/codex-rs/exec/src/exec_events.rs)

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0006](0006-use-openai-compatible-router-boundary.md)
- [ADR-0009](0009-use-os-backed-provider-credential-profiles.md)
