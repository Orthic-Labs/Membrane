use serde::{Deserialize, Serialize};

/// Dormant tab slot shown when a product is installed but not yet activated
/// via onboarding. Per D-S08, unchosen product stays dormant, enable-able later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DormantTab {
    pub product_id: String,
    pub display_name: String,
    pub reason: String,
}

pub fn dormant_for_uninstalled(product_id: &str) -> DormantTab {
    DormantTab {
        product_id: product_id.to_string(),
        display_name: if product_id == "cortex" { "Cortex".into() } else { "Membrane".into() },
        reason: "not_installed".into(),
    }
}

pub fn dormant_for_onboarding_pending(product_id: &str) -> DormantTab {
    DormantTab {
        product_id: product_id.to_string(),
        display_name: if product_id == "cortex" { "Cortex".into() } else { "Membrane".into() },
        reason: "onboarding_pending".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dormant_reason() {
        let d = dormant_for_uninstalled("cortex");
        assert_eq!(d.reason, "not_installed");
        let d2 = dormant_for_onboarding_pending("membrane");
        assert_eq!(d2.reason, "onboarding_pending");
    }
}
