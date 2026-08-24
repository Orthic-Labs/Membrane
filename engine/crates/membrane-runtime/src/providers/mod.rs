//! Resident native language-service provider adapters — the D1 tier of the
//! diagnostic ladder (design `docs/design/membrane-live-diagnostics-final-
//! architecture.md` §3 lifecycle contract, §6 cost discipline, §13
//! containment).
//!
//! Both adapters implement [`crate::live_diagnostics::DiagnosticsProvider`]
//! synchronously over std threads and channels (never tokio): they probe an
//! injected allowlisted search path without ever installing anything, spawn
//! engine processes under a sanitized `PATH`/`HOME`/`TMPDIR` environment,
//! synchronize full document contents on every workspace epoch change, honor
//! absolute deadlines from their reader queues, map cancellation onto the
//! wire protocol where one exists, terminate the whole process tree on
//! deadline/shutdown/drop (Unix process groups; Windows Job Objects), and
//! surface bounded-queue overflow and containment drops as typed omissions
//! on their coverage lanes.
//!
//! * [`typescript_provider`] speaks the tsserver newline-delimited JSON
//!   protocol (`tsgo` probed before `tsserver`) with
//!   [`ConvergenceClass::PushVersionedExact`] convergence.
//! * [`rust_analyzer_provider`] speaks LSP 3.17 stdio with cargo build
//!   scripts, all-targets, and flycheck disabled so native analysis stays
//!   pure-analysis D1 while `cargo check` remains V1, with
//!   [`ConvergenceClass::PullExact`] convergence.
//!
//! [`blueprint_findings`] is the one production seam for Blueprint D0a/D0b:
//! it consumes Blueprint's public resident findings service over daemon IPC
//! and never fabricates generation/freshness evidence.
//!
//! [`identity`] derives real `WorkspaceEngineKey` digests (binary, toolchain,
//! project config, sandbox policy) from bytes on disk and effective policy —
//! no placeholder identities.
//!
//! [`child_process`] holds the shared, individually testable plumbing: PATH
//! probing, env sanitization, process-tree containment, both framing codecs,
//! and the bounded reader thread. It owns no policy.

pub mod blueprint_findings;
pub mod child_process;
pub mod identity;
pub mod rust_analyzer_provider;
pub mod typescript_provider;

pub use blueprint_findings::{
    BlueprintFindingsClient, BlueprintFindingsError, BlueprintFinding, BlueprintFindingsResult,
    DaemonFindingsClient,
};
pub use child_process::{
    default_search_path, drain_frames_until, lsp_frame_bytes, probe_search_path,
    recv_with_deadline, recv_within, sanitize_env_pairs, sanitized_child_env,
    spawn_bounded_reader, spawn_sanitized, spawn_stderr_drainer, tsserver_line_bytes,
    FrameDecoder, FrameOutcome, LspDecoder, LineFrameDecoder, ReaderPump, SanitizedProcess,
    SANITIZED_ENV_KEYS,
};
pub use rust_analyzer_provider::{qualified_capabilities as rust_analyzer_capabilities, RustAnalyzerProvider};
pub use typescript_provider::{qualified_capabilities as typescript_capabilities, TypeScriptProvider};
