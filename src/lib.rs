//! PAL prototype: a small, local, agent-first durable runtime.
//!
//! This crate demonstrates four core ideas without network I/O or remote services:
//! 1. An append-only durable task journal (JSONL on disk).
//! 2. An explicit typed capability boundary for external-style effects.
//! 3. Deterministic replay of a recorded journal.
//! 4. A human-approval pause/resume path that persists and resumes state.

pub mod capability;
pub mod journal;
pub mod runtime;
pub mod types;

pub use capability::{Capability, CapabilitySet, Effect};
pub use journal::Journal;
pub use runtime::{Runtime, RuntimeError};
pub use types::{ApprovalDecision, JournalEvent, TaskId, TaskSnapshot, TaskStatus};
