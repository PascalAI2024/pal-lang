# PAL (prototype)

**PAL** — a durable, capability-safe runtime sketch for supervised AI agents.

This repository is a **bounded local prototype**, not a language release. It shows the smallest runnable Rust design that captures four runtime properties:

1. **Durable append-only task journal** — events persist as local JSONL (`Journal`).
2. **Typed effect / capability boundary** — external-style effects require an explicit grant (`CapabilitySet` + `Effect`).
3. **Deterministic replay** — the same journal always rebuilds the same `TaskSnapshot`.
4. **Human-approval pause / resume** — a task can enter `AwaitingApproval`, survive process restart (via the journal), and continue with a recorded decision.

No network calls, credentials, remote services, or publish pipeline are involved.

## Quick start

```bash
cargo test
cargo fmt
```

## Layout

| Path | Role |
|------|------|
| `src/types.rs` | Task ids, status, journal events, snapshots |
| `src/journal.rs` | Append-only JSONL persistence + replay fold |
| `src/capability.rs` | `Capability`, `Effect`, authorize gate |
| `src/runtime.rs` | Start / step / effect / approve / finish |
| `tests/prototype.rs` | Journaling/replay, denied capability, approval |

## Example flow

```rust
use pal::{
    ApprovalDecision, Capability, CapabilitySet, Effect, Journal, Runtime, TaskId,
};

let journal = Journal::open("pal.jsonl")?;
let caps = CapabilitySet::with([Capability::Log]);
let rt = Runtime::new(journal, caps);
let tid = TaskId::new("demo-1");

rt.start_task(tid.clone(), "demo")?;
rt.complete_step(&tid, "think", "planned work")?;
// Denied unless Capability::HttpFetch is granted:
// rt.attempt_effect(&tid, Effect::HttpFetch { url: "...".into() })?;
rt.request_approval(&tid, "ship the change")?;
rt.resume_with_approval(
    &tid,
    ApprovalDecision {
        approved: true,
        note: "lgtm".into(),
    },
)?;
rt.finish(&tid, "done")?;
let snap = rt.replay(&tid)?;
```

## Design notes

- **Effects are stubs.** Allowed effects are *recorded*, not executed against the OS or network. `HttpFetch` never dials; `WriteLocal` never writes a real file. This keeps the prototype offline and safe.
- **Capabilities are session-scoped.** Grants live on `Runtime` construction, not as a full policy language or ACL hierarchy.
- **Journal is JSONL.** One event per line, append-only, best-effort `sync_all` after each write. SQLite would also fit; JSONL keeps dependencies minimal.
- **Replay is pure fold.** `TaskSnapshot::apply` is the single reducer. No wall-clock or RNG is consulted during rebuild.
- **Approval is a stub.** There is no UI or multi-party workflow—only durable state transitions and a decision record.

## Limitations (intentionally out of scope)

- No real language surface (parser, AST, interpreter) — only a runtime kernel.
- No multi-agent scheduling, timers, or distributed consensus.
- No cryptographic integrity / signed journals.
- No concurrent writers; single-process assumed.
- No real I/O adapters (filesystem, HTTP, shell).
- No credential store, sandbox, or OS-level confinement.
- Not published to crates.io (`publish = false`).

## Tests

```bash
cargo test
```

Covered scenarios:

- journaling + independent deterministic replay
- denied capability (`HttpFetch` without grant)
- approval pause blocks progress; resume continues the same task
- approval rejection finishes as `Denied`

## License

MIT (prototype; not a product release).
