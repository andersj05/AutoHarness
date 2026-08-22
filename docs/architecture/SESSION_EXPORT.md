# Session export format

AutoHarness can export one session to a provider-neutral JSON file before it
is destructively deleted. The export is written by
`autoharness_app::export::export_session` next to the application database and
carries the complete authoritative event history, so the archived session can
be re-inspected or replayed against any future schema-v1 consumer without the
original SQLite database.

## File name

```
autoharness-session-{session_id}.export.v{schema_version}.json
```

Example: `autoharness-session-9f2c...c3.export.v1.json`

The file is created atomically: a temporary sibling file is written first and
renamed into place. A failed export never leaves a truncated archive.

## Document shape

```json
{
  "schema_version": 1,
  "session": {
    "session_id": "session-...",
    "status": "active",
    "title": null,
    "selected_provider_id": "google-ai-studio",
    "selected_model_id": "models/gemini-2.5-pro",
    "created_at_ms": 1724000000000,
    "updated_at_ms": 1724000100000
  },
  "event_count": 7,
  "events": [
    {
      "schema_version": 1,
      "event_id": "event-...",
      "session_id": "session-...",
      "sequence": 1,
      "occurred_at": 1724000000000,
      "causation": {"kind": "command", "id": "command-..."},
      "correlation_id": "correlation-...",
      "payload": {"kind": "session_created"}
    }
  ]
}
```

Field semantics:

| Field | Meaning |
| --- | --- |
| `schema_version` | Version of this export document format, currently `1`. |
| `session` | Projection summary at export time; `title` is `null` when never renamed. |
| `event_count` | Number of authoritative events, equal to `events.len()`. |
| `events` | The complete ordered schema-v1 event stream in durable sequence order. |

Event envelopes use exactly the serialized shape pinned by
`crates/autoharness-domain/tests/serialization.rs`.

## Guarantees

- The export contains only durable state. No credentials, environment values,
  tool artifacts, or in-flight attempt content appear in the document.
- Event order matches the authoritative per-session sequence numbers, so a
  consumer can reconstruct the identical aggregate through strict replay.
- Export succeeds independently of any provider; deletion of a session never
  removes an already-written export file.
