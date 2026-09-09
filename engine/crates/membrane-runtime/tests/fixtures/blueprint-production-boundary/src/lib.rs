//! Tiny deterministic fixture repository for Blueprint generation/schema
//! boundary tests. Kept intentionally small: one struct, one helper
//! function, and one function that calls it, so a real Blueprint graph
//! build produces a small but non-trivial, deterministic node/edge set.

pub struct BoundaryProbe {
    pub label: String,
}

impl BoundaryProbe {
    pub fn new(label: &str) -> Self {
        Self { label: label.to_string() }
    }
}

pub fn probe_label(probe: &BoundaryProbe) -> String {
    probe.label.clone()
}

pub fn describe_probe() -> String {
    let probe = BoundaryProbe::new("blueprint-production-boundary");
    probe_label(&probe)
}
