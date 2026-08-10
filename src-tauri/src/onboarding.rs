use serde::{Deserialize, Serialize};

/// First-run product picker. Per D-S08, choosing Membrane implicitly includes Cortex
/// and is non-deselectable — this is enforced by type structure, not just UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnboardingChoice {
    /// Cortex only (code graphs)
    CortexOnly,
    /// Membrane (context + memory) — includes Cortex, auto-selected and non-deselectable.
    Membrane,
}

impl OnboardingChoice {
    pub fn includes_cortex(&self) -> bool {
        true // both variants include Cortex
    }
    pub fn includes_membrane(&self) -> bool {
        matches!(self, Self::Membrane)
    }
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::CortexOnly => "Cortex",
            Self::Membrane => "Membrane (includes Cortex)",
        }
    }
}

/// Persisted onboarding state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    pub choice: OnboardingChoice,
    pub completed: bool,
}

pub fn save_choice(path: &std::path::Path, choice: OnboardingChoice) -> Result<(), String> {
    let state = OnboardingState { choice, completed: true };
    let bytes = serde_json::to_vec(&state).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().ok_or("parent_missing")?).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

pub fn load_choice(path: &std::path::Path) -> Option<OnboardingChoice> {
    let bytes = std::fs::read(path).ok()?;
    let state: OnboardingState = serde_json::from_slice(&bytes).ok()?;
    Some(state.choice)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn membrane_includes_cortex() {
        assert!(OnboardingChoice::Membrane.includes_cortex());
        assert!(OnboardingChoice::Membrane.includes_membrane());
        assert!(OnboardingChoice::CortexOnly.includes_cortex());
        assert!(!OnboardingChoice::CortexOnly.includes_membrane());
    }
    #[test]
    fn membrane_only_variant_does_not_exist() {
        // This test documents that MembraneOnly is not a valid variant.
        // AC-6 verifies that code attempting to use it fails to compile via trybuild.
        // If this compiles, the trybuild test would incorrectly pass.
        let _ = OnboardingChoice::CortexOnly;
        let _ = OnboardingChoice::Membrane;
    }
}
