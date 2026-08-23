use serde::{Deserialize, Serialize};

/// Parent Membrane service state — frozen canonical mapping.
///
/// Derived ONLY from:
/// - resident supervisor liveness
/// - resident /health ok
/// - live snapshot validity
///
/// Child/subsystem states NEVER participate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembraneParentState {
    Running,
    Degraded,
    Offline,
}

impl MembraneParentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Degraded => "degraded",
            Self::Offline => "offline",
        }
    }
}

/// The six semantic Membrane subsystems — distinct from the eight operational
/// Hub resources (`deliveries`, `providers`, `repositories`, `adapters`,
/// `devices`, `memory`, `sentinel`, `alerts`).
pub const SUBSYSTEM_NAMES: [&str; 6] = ["pull", "push", "cortex", "blueprint", "guide", "adapt"];

/// Frozen production mapping for parent state.
///
/// - supervisor must be Running; Unavailable/CrashLoop => Offline
/// - health_ok == None (unreachable) => Offline
/// - health_ok == Some(false) => Degraded (explicit unhealthy)
/// - live_snapshot_available == false => Degraded (snapshot invalid while resident healthy)
/// - otherwise => Running
pub fn membrane_parent_state(
    supervisor_running: bool,
    health_ok: Option<bool>,
    live_snapshot_available: bool,
) -> MembraneParentState {
    if !supervisor_running {
        return MembraneParentState::Offline;
    }
    match health_ok {
        None => MembraneParentState::Offline,
        Some(false) => MembraneParentState::Degraded,
        Some(true) => {
            if live_snapshot_available {
                MembraneParentState::Running
            } else {
                MembraneParentState::Degraded
            }
        }
    }
}

/// Variant that takes Tauri supervisor enum names.
pub fn membrane_parent_state_from_supervisor_str(
    supervisor_state: &str,
    health_ok: Option<bool>,
    live_snapshot_available: bool,
) -> MembraneParentState {
    let running = supervisor_state.eq_ignore_ascii_case("running");
    membrane_parent_state(running, health_ok, live_snapshot_available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_frozen_mapping() {
        // Running: healthy resident + valid snapshot + supervisor running
        assert_eq!(
            membrane_parent_state(true, Some(true), true),
            MembraneParentState::Running
        );
        // Degraded: unhealthy resident + valid snapshot
        assert_eq!(
            membrane_parent_state(true, Some(false), true),
            MembraneParentState::Degraded
        );
        // Degraded: healthy resident + invalid snapshot
        assert_eq!(
            membrane_parent_state(true, Some(true), false),
            MembraneParentState::Degraded
        );
        // Degraded takes precedence over snapshot when unhealthy
        assert_eq!(
            membrane_parent_state(true, Some(false), false),
            MembraneParentState::Degraded
        );
        // Offline: unreachable health
        assert_eq!(
            membrane_parent_state(true, None, true),
            MembraneParentState::Offline
        );
        // Offline: supervisor unavailable
        assert_eq!(
            membrane_parent_state(false, Some(true), true),
            MembraneParentState::Offline
        );
        // Offline: supervisor crash_loop even with healthy snapshot
        assert_eq!(
            membrane_parent_state_from_supervisor_str("crash_loop", Some(true), true),
            MembraneParentState::Offline
        );
        assert_eq!(
            membrane_parent_state_from_supervisor_str("unavailable", Some(true), true),
            MembraneParentState::Offline
        );
    }

    #[test]
    fn child_states_do_not_affect_parent() {
        // Every child unavailable, yet parent remains Running when resident healthy
        assert_eq!(
            membrane_parent_state(true, Some(true), true),
            MembraneParentState::Running
        );
    }
}
