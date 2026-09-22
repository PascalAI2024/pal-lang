use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::types::{JournalEvent, TaskId, TaskSnapshot};

/// Errors from durable journal I/O.
#[derive(Debug, Error)]
pub enum JournalError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("task not found: {0}")]
    TaskNotFound(String),
}

/// Append-only durable journal persisted as JSONL on the local filesystem.
///
/// Each line is one [`JournalEvent`]. Events are never rewritten in place.
/// There are no network calls and no remote storage backends.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// Open or create a journal at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        // Ensure the file exists so readers do not fail on empty journals.
        OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a single event. Durability is best-effort `sync_all` after write.
    pub fn append(&self, event: &JournalEvent) -> Result<(), JournalError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    }

    /// Read every event in append order.
    pub fn read_all(&self) -> Result<Vec<JournalEvent>, JournalError> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            events.push(serde_json::from_str(trimmed)?);
        }
        Ok(events)
    }

    /// Filter events belonging to one task (by id match inside the event).
    pub fn events_for(&self, task_id: &TaskId) -> Result<Vec<JournalEvent>, JournalError> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|e| event_task_id(e) == Some(task_id))
            .collect())
    }

    /// Deterministically rebuild task state from the journal.
    pub fn replay(&self, task_id: &TaskId) -> Result<TaskSnapshot, JournalError> {
        let events = self.events_for(task_id)?;
        if events.is_empty() {
            return Err(JournalError::TaskNotFound(task_id.to_string()));
        }
        let name = match &events[0] {
            JournalEvent::TaskStarted { name, .. } => name.clone(),
            _ => "unknown".into(),
        };
        let mut snap = TaskSnapshot::new(task_id.clone(), name);
        for event in &events {
            snap.apply(event);
        }
        Ok(snap)
    }
}

fn event_task_id(event: &JournalEvent) -> Option<&TaskId> {
    match event {
        JournalEvent::TaskStarted { task_id, .. }
        | JournalEvent::StepCompleted { task_id, .. }
        | JournalEvent::EffectAttempted { task_id, .. }
        | JournalEvent::ApprovalRequested { task_id, .. }
        | JournalEvent::ApprovalResolved { task_id, .. }
        | JournalEvent::TaskFinished { task_id, .. } => Some(task_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TaskStatus;
    use tempfile::tempdir;

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tempdir().unwrap();
        let j = Journal::open(dir.path().join("tasks.jsonl")).unwrap();
        let tid = TaskId::new("t1");
        j.append(&JournalEvent::TaskStarted {
            task_id: tid.clone(),
            name: "demo".into(),
        })
        .unwrap();
        j.append(&JournalEvent::StepCompleted {
            task_id: tid.clone(),
            step: "s1".into(),
            detail: "did work".into(),
        })
        .unwrap();
        let events = j.read_all().unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn replay_reaches_same_status() {
        let dir = tempdir().unwrap();
        let j = Journal::open(dir.path().join("tasks.jsonl")).unwrap();
        let tid = TaskId::new("t-replay");
        j.append(&JournalEvent::TaskStarted {
            task_id: tid.clone(),
            name: "replay-demo".into(),
        })
        .unwrap();
        j.append(&JournalEvent::StepCompleted {
            task_id: tid.clone(),
            step: "prepare".into(),
            detail: "ok".into(),
        })
        .unwrap();
        j.append(&JournalEvent::TaskFinished {
            task_id: tid.clone(),
            status: TaskStatus::Completed,
            summary: "done".into(),
        })
        .unwrap();

        let snap = j.replay(&tid).unwrap();
        assert_eq!(snap.status, TaskStatus::Completed);
        assert_eq!(snap.steps, vec!["prepare".to_string()]);
        assert_eq!(snap.summary.as_deref(), Some("done"));
    }
}
