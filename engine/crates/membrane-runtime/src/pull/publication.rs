//! Pull packet publication seam.
//!
//! Federation assembles provider evidence; this namespace is the only public
//! Pull route for converting admitted candidate sets into bounded packet and
//! receipt envelopes. The implementation is shared with federation so there
//! is one publication policy, not parallel serializers.

pub use super::federation::{envelope_from_ccs, EnvelopeInput};

