use serde::{Deserialize, Serialize};
use std::fmt;

use crate::capability::Effect;

/// Stable identifier for a durable task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle status of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    AwaitingApproval,
    Completed,
    Failed,
    Denied,
}

/// Human decision recorded when resuming an approval pause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approved: bool,
    pub note: String,
}

/// Append-only journal events. Order is authoritative for replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEvent {
    TaskStarted {
        task_id: TaskId,
        name: String,
    },
    StepCompleted {
        task_id: TaskId,
        step: String,
        detail: String,
    },
    EffectAttempted {
        task_id: TaskId,
        effect: Effect,
        allowed: bool,
        reason: String,
    },
    ApprovalRequested {
        task_id: TaskId,
        reason: String,
    },
    ApprovalResolved {
        task_id: TaskId,
        approved: bool,
        note: String,
    },
    TaskFinished {
        task_id: TaskId,
        status: TaskStatus,
        summary: String,
    },
}

/// Materialized view of a task after journal replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub task_id: TaskId,
    pub name: String,
    pub status: TaskStatus,
    pub steps: Vec<String>,
    pub effects_allowed: Vec<Effect>,
    pub effects_denied: Vec<Effect>,
    pub pending_approval: Option<String>,
    pub last_approval: Option<ApprovalDecision>,
    pub summary: Option<String>,
}

impl TaskSnapshot {
    pub fn new(task_id: TaskId, name: impl Into<String>) -> Self {
        Self {
            task_id,
            name: name.into(),
            status: TaskStatus::Running,
            steps: Vec::new(),
            effects_allowed: Vec::new(),
            effects_denied: Vec::new(),
            pending_approval: None,
            last_approval: None,
            summary: None,
        }
    }

    /// Fold a single journal event into this snapshot.
    pub fn apply(&mut self, event: &JournalEvent) {
        match event {
            JournalEvent::TaskStarted { task_id, name } => {
                if task_id == &self.task_id {
                    self.name = name.clone();
                    self.status = TaskStatus::Running;
                }
            }
            JournalEvent::StepCompleted { task_id, step, .. } => {
                if task_id == &self.task_id {
                    self.steps.push(step.clone());
                }
            }
            JournalEvent::EffectAttempted {
                task_id,
                effect,
                allowed,
                ..
            } => {
                if task_id == &self.task_id {
                    if *allowed {
                        self.effects_allowed.push(effect.clone());
                    } else {
                        self.effects_denied.push(effect.clone());
                    }
                }
            }
            JournalEvent::ApprovalRequested { task_id, reason } => {
                if task_id == &self.task_id {
                    self.status = TaskStatus::AwaitingApproval;
                    self.pending_approval = Some(reason.clone());
                }
            }
            JournalEvent::ApprovalResolved {
                task_id,
                approved,
                note,
            } => {
                if task_id == &self.task_id {
                    self.pending_approval = None;
                    self.last_approval = Some(ApprovalDecision {
                        approved: *approved,
                        note: note.clone(),
                    });
                    self.status = if *approved {
                        TaskStatus::Running
                    } else {
                        TaskStatus::Denied
                    };
                }
            }
            JournalEvent::TaskFinished {
                task_id,
                status,
                summary,
            } => {
                if task_id == &self.task_id {
                    self.status = *status;
                    self.summary = Some(summary.clone());
                    self.pending_approval = None;
                }
            }
        }
    }
}
