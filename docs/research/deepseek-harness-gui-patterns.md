# DeepSeek Harness GUI reference review

**Reviewed:** 2026-08-30

**Upstream snapshot:** [`0a53fb55bea101816fa226bb964ae2bed71c343b`](https://github.com/deepseek-ai/deepseek-harness/tree/0a53fb55bea101816fa226bb964ae2bed71c343b)

**AutoHarness decision:** [ADR-0019](../adr/0019-use-tauri-web-rendered-desktop-client.md)

## Purpose

This review identifies which DeepSeek Harness GUI architecture patterns should guide AutoHarness and which assumptions do not fit AutoHarness's Rust, native-desktop, durability, and capability boundaries.
It records an immutable upstream snapshot because DeepSeek Harness is a fast-moving developer preview.
It is a reference review, not a compatibility promise or a plan to copy upstream branding, assets, or implementation code.

## What DeepSeek Harness currently ships

At the reviewed commit, DeepSeek Harness exposes a browser application through `dsh web` at a loopback address.
Its public repository does not contain the production Tauri desktop shell selected for AutoHarness.
The checked application manifest describes `apps/web` as a thin Vite entry over reusable client packages and reports version `0.1.2-alpha.2`.
The reviewed frontend toolchain includes React 18, TypeScript 6, Vite 6, Vitest, and Playwright.

The important reference is therefore its client architecture and protocol discipline, not its packaging choice.

Primary upstream evidence:

- [Repository and launch model](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/README.md).
- [Web Client architecture](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/docs/subsystems/web-client.md).
- [GUI layering and RPC protocol](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/.agents/notes/archived/architecture/2026-07-19-gui-layering-and-rpc-protocol.md).
- [Web application manifest](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/apps/web/package.json).

## The architectural pattern worth following

DeepSeek Harness defines the dependency direction as host state, remote transport, client model, UI adapter, conversation or presentation, slots, and finally React.
The host owns authoritative state, persistence, mutation ordering, access policy, and stream production.
React-free client models retain identity-stable projections, reconcile stream and unary races, and expose narrow commands.
Presentation packages consume those models without receiving the host context or physical transport.

Its session model also separates three concerns that are easy to conflate:

1. Durable history and sequence validation.
2. Transient control state such as running work and approval requests.
3. Presentation state such as selection and disclosure.

Every physical connection generation begins from an authoritative baseline.
Ordered increments follow that baseline, and reconnect replaces client state before later changes apply.
Prompt correlation lets an optimistic local echo converge with the durable user event.
Pending approvals use a separate answerable control path while their durable audit continues through session history.

These properties matter more than the particular HTTP and WebSocket carrier used upstream.

## What AutoHarness adopts

### Host authority

The existing Rust runtime remains the only authority for sessions, events, attempts, tools, permissions, credentials, provider activity, settings, and recovery.
The GUI cannot mutate storage or execute capabilities directly.

### Renderer-neutral contract

`autoharness-client` owns versioned wire-safe commands, projections, frames, identifiers, bounds, and public failures.
Neither the React client nor the Tauri adapter defines a second business contract.

### React-free client model

The frontend store consumes the transport and implements snapshot replacement, revision validation, gap detection, request correlation, and selectors without importing React.
React bindings remain a thin subscription adapter.

### Baseline and resynchronization

The first vertical slice sends complete snapshots because this is the simplest correct migration bridge.
Every frame carries a monotonic transport revision.
A missing revision forces an authoritative snapshot replacement before incremental delivery resumes.

Whole-transcript snapshots are temporary.
Long-session parity requires keyed transcript changes or bounded committed deltas so a streamed chunk does not serialize all prior history.

### Separate durable and transient paths

Durable session events remain the replay authority.
Cancellation, streaming liveness, authentication prompts, request receipts, and permission answers remain explicit control-plane concepts.
The GUI does not fabricate a durable outcome from a successful mailbox receipt.

### Fixture transport

Browser-only fixture mode implements the same logical client transport as Tauri mode.
It provides fast deterministic UI development without giving fixture behavior a production-only code path.

### Presentation seams

Initial extension points are typed slots and trusted components for navigation, transcript items, composer accessories, inspector content, routes, and modal ownership.
Features exchange props, selectors, and callbacks rather than importing another feature's implementation.

## What AutoHarness adapts

### Physical carrier

DeepSeek Harness currently uses loopback HTTP and WebSocket transport for its browser application.
AutoHarness uses in-process Tauri commands and ordered channels so ordinary desktop use opens no localhost listener and needs no Node host or sidecar.
The logical contract remains independent of Tauri so another carrier can be introduced later without changing business semantics.

### Protocol versioning

DeepSeek Harness currently release-binds its host and client and therefore does not expose a protocol version.
AutoHarness starts with schema version 1 because packaged assets, update rollback, crash recovery, and a future daemon can create real version skew.

### DTO boundary

DeepSeek Harness can reuse many TypeScript core types directly in its TypeScript browser packages.
AutoHarness crosses a Rust-to-TypeScript serialization boundary and therefore uses deliberate public projections.
Raw domain types with local-only values, secret-bearing types, storage handles, and provider-native structures never cross that boundary.

### Capability model

AutoHarness already durably records tool proposals, permission decisions, effect-start boundaries, outcomes, and interruption recovery.
The first real GUI chat slice must preserve permission preemption instead of treating approval as a later decorative card.

### Desktop security

Tauri capabilities expose only the narrow AutoHarness commands required by the window.
No generic shell, process, filesystem, SQL, or network plugin is available to frontend code.
Remote scripts, remote page content, arbitrary HTML, and unrestricted extension code remain prohibited.

## What AutoHarness rejects

- An everything-is-a-plugin runtime as the first migration step.
- A required Node process or localhost server for ordinary desktop execution.
- A browser-owned database, credential store, provider client, or tool runtime.
- Client authority inferred from an optimistic echo or transport acknowledgement.
- Unbounded event queues or repaint work for each token fragment.
- Arbitrary third-party JavaScript inside the privileged application webview.
- Upstream branding, visual assets, terminology, or layout copied as product identity.

## Concrete mapping

| DeepSeek Harness role | AutoHarness implementation |
| --- | --- |
| Host application and controllers | Existing Rust coordinator, engine, store, providers, tools, memory, and settings |
| Channel-independent API contract | `autoharness-client` |
| Web HTTP and WebSocket carrier | Tauri command and channel adapter |
| React-free client models | TypeScript client store |
| Fixture API client | Browser fixture transport |
| UI adapters and slots | Typed selectors, feature props, and presentation slots |
| Vite application | `apps/gui` |

## First-slice acceptance questions

The implementation is not accepted merely because the shell renders.
Review must answer all of these questions with tests or observed evidence:

- Can the GUI start from authoritative replay while the provider is offline?
- Can a prompt be admitted, streamed, cancelled, retried, and observed after restart?
- Does a missing transport revision force resynchronization without retaining impossible mixed state?
- Does a permission request preempt navigation, menus, and ordinary dialogs?
- Does the secret ingress clear the DOM and avoid snapshots, frames, logs, storage, and diagnostics?
- Does closing the window preserve the existing conservative cancellation and unknown-outcome rules?
- Do fixture and Tauri transports implement the same client-facing interface?
- Does long-session work stay bounded before performance parity is claimed?
- Do compact, standard, and wide layouts preserve focus, selection, and the composer?
- Does the frontend remain useful with keyboard navigation, reduced motion, high contrast, and 200 percent zoom?

## Result

The DeepSeek Harness review supports the decision to adopt a web-rendered GUI architecture while keeping the AutoHarness Rust runtime authoritative.
It does not support replacing the current Rust application with a TypeScript host or serving the desktop interface over localhost.
The resulting AutoHarness architecture follows the reusable layering and recovery principles while selecting a native carrier and stricter secret and capability boundaries.
