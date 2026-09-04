//! Ledger — rebuildable document navigation, projection, & hash-bound resolution.
//!
//! Ledger owns document indexes and source-bound section references. It does not own
//! canonical repository truth (Blueprint) or durable learned memory (Cortex).

pub mod db;
pub mod doc_candidate_provider;
pub mod doc_projection;
pub mod doc_shadow;
pub mod doc_spine;
pub mod document_conversion;
pub mod identifier;
pub mod index;
pub mod limits;
pub mod link_projection;
pub mod outline;
pub mod policy;
pub mod query;
pub mod query_alias;
pub(crate) mod reconcile;
pub mod resolve;
pub mod session_projection;

pub use db::LedgerDb;
