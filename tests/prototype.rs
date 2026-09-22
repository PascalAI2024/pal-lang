//! Focused automated tests for the PAL prototype:
//! - journaling + deterministic replay
//! - denied capability boundary
//! - human-approval pause / resume

use pal::{
    ApprovalDecision, Capability, CapabilitySet, Effect, Journal, Runtime, RuntimeError, TaskId,
    TaskStatus,
};
use tempfile::tempdir;

fn runtime_with(caps: impl IntoIterator<Item = Capability>) -> (tempfile::TempDir, Runtime) {
    let dir = tempdir().expect("tempdir");
    let journal = Journal::open(dir.path().join("pal.jsonl")).expect("open journal");
    let rt = Runtime::new(journal, CapabilitySet::with(caps));
    (dir, rt)
}

#[test]
fn journaling_and_deterministic_replay() {
    let (_dir, rt) = runtime_with([Capability::Log]);
    let tid = TaskId::new("task-replay-1");

    rt.start_task(tid.clone(), "fixture-task").unwrap();
    rt.complete_step(&tid, "plan", "sketched steps").unwrap();
    rt.attempt_effect(
        &tid,
        Effect::Log {
            message: "progress".into(),
        },
    )
    .unwrap();
    rt.finish(&tid, "all good").unwrap();

    let live = rt.replay(&tid).unwrap();

    // Re-open the same journal file and replay independently.
    let path = rt.journal().path().to_path_buf();
    let journal2 = Journal::open(&path).unwrap();
    let rt2 = Runtime::new(journal2, CapabilitySet::with([Capability::Log]));
    let rebuilt = rt2.replay(&tid).unwrap();

    assert_eq!(live, rebuilt);
    assert_eq!(rebuilt.status, TaskStatus::Completed);
    assert_eq!(rebuilt.steps, vec!["plan".to_string()]);
    assert_eq!(rebuilt.effects_allowed.len(), 1);
    assert_eq!(rebuilt.summary.as_deref(), Some("all good"));
    assert!(rebuilt.effects_denied.is_empty());
}

#[test]
fn denied_capability_is_recorded_and_errors() {
    // Only Log is granted; HttpFetch must be denied.
    let (_dir, rt) = runtime_with([Capability::Log]);
    let tid = TaskId::new("task-cap-1");

    rt.start_task(tid.clone(), "cap-boundary").unwrap();

    let err = rt
        .attempt_effect(
            &tid,
            Effect::HttpFetch {
                url: "https://example.invalid/secret".into(),
            },
        )
        .expect_err("http fetch must be denied without capability");

    match err {
        RuntimeError::CapabilityDenied(Capability::HttpFetch) => {}
        other => panic!("unexpected error: {other:?}"),
    }

    let snap = rt.replay(&tid).unwrap();
    assert_eq!(snap.effects_denied.len(), 1);
    assert!(snap.effects_allowed.is_empty());
    assert_eq!(
        snap.effects_denied[0],
        Effect::HttpFetch {
            url: "https://example.invalid/secret".into(),
        }
    );
    // Task remains running after a denied effect (caller may recover or fail).
    assert_eq!(snap.status, TaskStatus::Running);
}

#[test]
fn approval_pause_and_resume() {
    let (_dir, rt) = runtime_with([Capability::WriteLocal]);
    let tid = TaskId::new("task-approve-1");

    rt.start_task(tid.clone(), "needs-human").unwrap();
    rt.complete_step(&tid, "draft", "prepared write").unwrap();

    let paused = rt
        .request_approval(&tid, "confirm writing local artifact")
        .unwrap();
    assert_eq!(paused.status, TaskStatus::AwaitingApproval);
    assert_eq!(
        paused.pending_approval.as_deref(),
        Some("confirm writing local artifact")
    );

    // While paused, further work is blocked.
    let blocked = rt
        .complete_step(&tid, "should-not-run", "nope")
        .expect_err("must block while awaiting approval");
    assert!(matches!(blocked, RuntimeError::AwaitingApproval));

    // Resume with approval; same task continues.
    let resumed = rt
        .resume_with_approval(
            &tid,
            ApprovalDecision {
                approved: true,
                note: "looks fine".into(),
            },
        )
        .unwrap();
    assert_eq!(resumed.status, TaskStatus::Running);
    assert!(resumed.pending_approval.is_none());
    assert_eq!(
        resumed.last_approval,
        Some(ApprovalDecision {
            approved: true,
            note: "looks fine".into(),
        })
    );

    rt.attempt_effect(
        &tid,
        Effect::WriteLocal {
            path: "out.txt".into(),
            content: "ok".into(),
        },
    )
    .unwrap();
    let done = rt.finish(&tid, "approved path complete").unwrap();
    assert_eq!(done.status, TaskStatus::Completed);

    // Independent replay from disk matches.
    let rebuilt = Journal::open(rt.journal().path())
        .unwrap()
        .replay(&tid)
        .unwrap();
    assert_eq!(rebuilt, done);
}

#[test]
fn approval_rejection_finishes_as_denied() {
    let (_dir, rt) = runtime_with([]);
    let tid = TaskId::new("task-reject-1");

    rt.start_task(tid.clone(), "risky").unwrap();
    rt.request_approval(&tid, "destroy something").unwrap();

    let denied = rt
        .resume_with_approval(
            &tid,
            ApprovalDecision {
                approved: false,
                note: "too dangerous".into(),
            },
        )
        .unwrap();

    assert_eq!(denied.status, TaskStatus::Denied);
    assert_eq!(
        denied.summary.as_deref(),
        Some("rejected by human: too dangerous")
    );
}
