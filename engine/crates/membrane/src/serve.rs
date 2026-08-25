//! Compatibility refusal for the retired standalone loopback runtime.
//!
//! Runtime residency is owned by the active Hub process. Library callers that
//! still reference this former entrypoint receive explicit Hub unavailability;
//! this function never binds a port or starts runtime logic.

pub fn run_loopback_api(_port: u16) -> Result<(), String> {
    Err("membrane_unavailable: hub_inactive (retryable)".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn retired_loopback_entrypoint_never_starts_a_runtime() {
        let error = super::run_loopback_api(47_851).unwrap_err();
        assert!(error.contains("hub_inactive"));
        assert!(error.contains("retryable"));
    }
}
