use serde::{Deserialize, Serialize};

/// Named capabilities an agent may be granted.
///
/// Only capabilities present in a [`CapabilitySet`] may authorize
/// external-style effects. Local bookkeeping (journal append, step
/// recording) does not require a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Emit a local log line (side-effect stub; never leaves the process).
    Log,
    /// Write a local artifact path (stub; recorded, not performed on disk).
    WriteLocal,
    /// External-style network fetch (stub; never dials the network).
    HttpFetch,
}

/// An effect that would touch the outside world if executed for real.
///
/// In this prototype effects are *typed and gated*, not executed against
/// real I/O. The runtime records allow/deny decisions in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Log { message: String },
    WriteLocal { path: String, content: String },
    HttpFetch { url: String },
}

impl Effect {
    /// Capability required to perform this effect.
    pub fn required_capability(&self) -> Capability {
        match self {
            Effect::Log { .. } => Capability::Log,
            Effect::WriteLocal { .. } => Capability::WriteLocal,
            Effect::HttpFetch { .. } => Capability::HttpFetch,
        }
    }
}

/// Explicit set of granted capabilities for a task/session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    granted: Vec<Capability>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(caps: impl IntoIterator<Item = Capability>) -> Self {
        let mut set = Self::new();
        for c in caps {
            set.grant(c);
        }
        set
    }

    pub fn grant(&mut self, cap: Capability) {
        if !self.granted.contains(&cap) {
            self.granted.push(cap);
        }
    }

    pub fn is_granted(&self, cap: Capability) -> bool {
        self.granted.contains(&cap)
    }

    /// Returns `Ok(())` only when the required capability is present.
    pub fn authorize(&self, effect: &Effect) -> Result<(), Capability> {
        let need = effect.required_capability();
        if self.is_granted(need) {
            Ok(())
        } else {
            Err(need)
        }
    }

    pub fn granted(&self) -> &[Capability] {
        &self.granted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_effect_without_capability() {
        let caps = CapabilitySet::with([Capability::Log]);
        let fetch = Effect::HttpFetch {
            url: "https://example.invalid".into(),
        };
        assert_eq!(caps.authorize(&fetch), Err(Capability::HttpFetch));
    }

    #[test]
    fn allows_effect_with_capability() {
        let caps = CapabilitySet::with([Capability::Log]);
        let log = Effect::Log {
            message: "hello".into(),
        };
        assert!(caps.authorize(&log).is_ok());
    }
}
