use thiserror::Error;

use crate::capability::{Capability, CapabilitySet, Effect};
use crate::journal::{Journal, JournalError};
use crate::types::{ApprovalDecision, JournalEvent, TaskId, TaskSnapshot, TaskStatus};

/// Runtime errors for the PAL prototype.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("journal error: {0}")]
    Journal(#[from] JournalError),
    #[error("task is not awaiting approval (status: {0:?})")]
    NotAwaitingApproval(TaskStatus),
    #[error("task is awaiting approval; resume with a decision before continuing")]
    AwaitingApproval,
    #[error("task already finished with status {0:?}")]
    AlreadyFinished(TaskStatus),
    #[error("capability denied: {0:?} required for effect")]
    CapabilityDenied(Capability),
}

/// Small durable task runtime backed by a local journal.
///
/// The runtime:
/// - records steps and effect attempts as append-only events,
/// - gates external-style effects through an explicit [`CapabilitySet`],
/// - can pause for human approval and later resume with a decision,
/// - can rebuild state by replaying the journal.
pub struct Runtime {
    journal: Journal,
    capabilities: CapabilitySet,
}

impl Runtime {
    pub fn new(journal: Journal, capabilities: CapabilitySet) -> Self {
        Self {
            journal,
            capabilities,
        }
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Start a new durable task and return its snapshot.
    pub fn start_task(
        &self,
        task_id: TaskId,
        name: impl Into<String>,
    ) -> Result<TaskSnapshot, RuntimeError> {
        let name = name.into();
        self.journal.append(&JournalEvent::TaskStarted {
            task_id: task_id.clone(),
            name: name.clone(),
        })?;
        Ok(TaskSnapshot::new(task_id, name))
    }

    /// Record a completed logical step.
    pub fn complete_step(
        &self,
        task_id: &TaskId,
        step: impl Into<String>,
        detail: impl Into<String>,
    ) -> Result<TaskSnapshot, RuntimeError> {
        self.ensure_active(task_id)?;
        self.journal.append(&JournalEvent::StepCompleted {
            task_id: task_id.clone(),
            step: step.into(),
            detail: detail.into(),
        })?;
        Ok(self.journal.replay(task_id)?)
    }

    /// Attempt an effect under the capability boundary.
    ///
    /// On deny: journals a denied attempt and returns [`RuntimeError::CapabilityDenied`].
    /// On allow: journals an allowed attempt (effect is *not* executed for real).
    pub fn attempt_effect(
        &self,
        task_id: &TaskId,
        effect: Effect,
    ) -> Result<TaskSnapshot, RuntimeError> {
        self.ensure_active(task_id)?;
        match self.capabilities.authorize(&effect) {
            Ok(()) => {
                self.journal.append(&JournalEvent::EffectAttempted {
                    task_id: task_id.clone(),
                    effect,
                    allowed: true,
                    reason: "capability granted".into(),
                })?;
                Ok(self.journal.replay(task_id)?)
            }
            Err(missing) => {
                self.journal.append(&JournalEvent::EffectAttempted {
                    task_id: task_id.clone(),
                    effect,
                    allowed: false,
                    reason: format!("missing capability: {missing:?}"),
                })?;
                Err(RuntimeError::CapabilityDenied(missing))
            }
        }
    }

    /// Pause the task pending human approval. State is durable in the journal.
    pub fn request_approval(
        &self,
        task_id: &TaskId,
        reason: impl Into<String>,
    ) -> Result<TaskSnapshot, RuntimeError> {
        self.ensure_active(task_id)?;
        self.journal.append(&JournalEvent::ApprovalRequested {
            task_id: task_id.clone(),
            reason: reason.into(),
        })?;
        Ok(self.journal.replay(task_id)?)
    }

    /// Resume a task that is awaiting approval with an explicit decision.
    pub fn resume_with_approval(
        &self,
        task_id: &TaskId,
        decision: ApprovalDecision,
    ) -> Result<TaskSnapshot, RuntimeError> {
        let snap = self.journal.replay(task_id)?;
        if snap.status != TaskStatus::AwaitingApproval {
            return Err(RuntimeError::NotAwaitingApproval(snap.status));
        }
        self.journal.append(&JournalEvent::ApprovalResolved {
            task_id: task_id.clone(),
            approved: decision.approved,
            note: decision.note.clone(),
        })?;
        if !decision.approved {
            self.journal.append(&JournalEvent::TaskFinished {
                task_id: task_id.clone(),
                status: TaskStatus::Denied,
                summary: format!("rejected by human: {}", decision.note),
            })?;
        }
        Ok(self.journal.replay(task_id)?)
    }

    /// Mark the task completed.
    pub fn finish(
        &self,
        task_id: &TaskId,
        summary: impl Into<String>,
    ) -> Result<TaskSnapshot, RuntimeError> {
        let snap = self.journal.replay(task_id)?;
        match snap.status {
            TaskStatus::AwaitingApproval => return Err(RuntimeError::AwaitingApproval),
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Denied => {
                return Err(RuntimeError::AlreadyFinished(snap.status));
            }
            TaskStatus::Running => {}
        }
        self.journal.append(&JournalEvent::TaskFinished {
            task_id: task_id.clone(),
            status: TaskStatus::Completed,
            summary: summary.into(),
        })?;
        Ok(self.journal.replay(task_id)?)
    }

    /// Rebuild state purely from the durable journal (deterministic replay).
    pub fn replay(&self, task_id: &TaskId) -> Result<TaskSnapshot, RuntimeError> {
        Ok(self.journal.replay(task_id)?)
    }

    fn ensure_active(&self, task_id: &TaskId) -> Result<(), RuntimeError> {
        let snap = self.journal.replay(task_id)?;
        match snap.status {
            TaskStatus::Running => Ok(()),
            TaskStatus::AwaitingApproval => Err(RuntimeError::AwaitingApproval),
            other => Err(RuntimeError::AlreadyFinished(other)),
        }
    }
}
