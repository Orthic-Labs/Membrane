//! Bounded native provider scheduling.
//!
//! Scheduling owns one request deadline.  Provider futures receive a clone of
//! the immutable request context with the same absolute instant and a child
//! cancellation token.  Results are collected by provider rank, never by
//! completion order.

use crate::deadline::Deadline;
use membrane_protocol::{ProviderId, ProviderOmissionV1, ProviderOutputV1, ReasonCode};
use membrane_provider_sdk::{ProviderContext, ProviderError, ProviderOutput};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub type ProviderFuture = Pin<Box<dyn Future<Output = Result<ProviderOutput, ProviderError>> + Send>>;
type JoinedProvider = (ProviderId, Instant, Instant, Result<ProviderOutput, ProviderError>);

/// Scheduling bounds that are independent from the request's absolute
/// deadline.  `shutdown_grace` is only the bounded drain window after a
/// cancellation/deadline signal; it is never passed to a provider as a new
/// timeout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerPolicy {
    pub max_in_flight: usize,
    pub shutdown_grace: Duration,
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self {
            max_in_flight: ProviderId::ALL.len(),
            shutdown_grace: Duration::from_millis(50),
        }
    }
}

impl SchedulerPolicy {
    pub fn bounded(max_in_flight: usize, shutdown_grace: Duration) -> Self {
        Self {
            max_in_flight: max_in_flight.max(1),
            shutdown_grace,
        }
    }
}

/// A provider lane plus its prerequisite lane IDs.  The closure is called at
/// most once, only after all prerequisites have completed successfully.
pub struct ProviderTask {
    pub provider: ProviderId,
    pub prerequisites: Vec<ProviderId>,
    run: Arc<dyn Fn(ProviderContext) -> ProviderFuture + Send + Sync>,
}

impl std::fmt::Debug for ProviderTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderTask")
            .field("provider", &self.provider)
            .field("prerequisites", &self.prerequisites)
            .finish_non_exhaustive()
    }
}

impl ProviderTask {
    pub fn new<F, Fut>(provider: ProviderId, run: F) -> Self
    where
        F: Fn(ProviderContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ProviderOutput, ProviderError>> + Send + 'static,
    {
        Self {
            provider,
            prerequisites: Vec::new(),
            run: Arc::new(move |context| Box::pin(run(context))),
        }
    }

    pub fn with_prerequisites<I>(mut self, prerequisites: I) -> Self
    where
        I: IntoIterator<Item = ProviderId>,
    {
        self.prerequisites = prerequisites.into_iter().collect();
        self
    }

    fn invoke(&self, context: ProviderContext) -> ProviderFuture {
        (self.run)(context)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderTiming {
    pub provider: ProviderId,
    pub queue_ms: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub remaining_deadline_ms: u64,
}

#[derive(Debug, Default)]
pub struct ScheduleResult {
    pub outputs: Vec<ProviderOutputV1>,
    pub omissions: Vec<ProviderOmissionV1>,
    pub timings: Vec<ProviderTiming>,
    pub deadline_exhausted: bool,
    pub cancelled: bool,
}

impl ScheduleResult {
    pub fn canonicalize(&mut self) {
        self.outputs.sort_by_key(|output| output.provider.rank());
        self.omissions.sort_by(|left, right| {
            left.provider
                .rank()
                .cmp(&right.provider.rank())
                .then_with(|| left.reason.as_str().cmp(right.reason.as_str()))
                .then_with(|| left.detail_id.cmp(&right.detail_id))
        });
        self.timings.sort_by_key(|timing| timing.provider.rank());
    }
}

/// Execute provider lanes with one absolute deadline and bounded concurrency.
pub async fn schedule_providers(
    context: ProviderContext,
    deadline: Deadline,
    tasks: Vec<ProviderTask>,
    policy: SchedulerPolicy,
) -> ScheduleResult {
    let mut result = ScheduleResult::default();
    let mut pending = tasks;
    pending.sort_by_key(|task| task.provider.rank());
    let mut completed = HashSet::new();
    let mut failed = HashSet::new();
    let request_cancellation = context.cancellation.child_token();
    let started_at = Instant::now();
    let mut running: JoinSet<JoinedProvider> = JoinSet::new();
    // JoinError does not carry provider metadata, so retain the task-ID to
    // lane mapping returned by JoinSet::spawn.  This covers panic and abort
    // paths as well as ordinary completed outputs.
    let mut running_ids = HashMap::new();
    let mut stopped = false;

    loop {
        // Launch only dependency-ready lanes, in canonical provider order.
        while !stopped && running.len() < policy.max_in_flight {
            let now = Instant::now();
            if deadline.is_exhausted_at(now) {
                result.deadline_exhausted = true;
                stopped = true;
                break;
            }
            if context.cancellation.is_cancelled() {
                result.cancelled = true;
                stopped = true;
                break;
            }

            let blocked = pending.iter().position(|task| {
                task.prerequisites.iter().any(|dependency| failed.contains(dependency))
            });
            if let Some(index) = blocked {
                let task = pending.remove(index);
                failed.insert(task.provider);
                result.omissions.push(omission(
                    task.provider,
                    ReasonCode::ProviderUnavailable,
                    "prerequisite_failed",
                ));
                continue;
            }

            let Some(index) = pending.iter().position(|task| {
                task.prerequisites.iter().all(|dependency| completed.contains(dependency))
            }) else {
                break;
            };
            let task = pending.remove(index);
            let queue_ms = elapsed_ms(started_at, now);
            let child_context = child_context(&context, deadline, request_cancellation.child_token());
            let provider = task.provider;
            let run = task.invoke(child_context);
            let task_started = Instant::now();
            let task_id = running.spawn(async move {
                let output = run.await;
                (provider, now, task_started, output)
            });
            running_ids.insert(task_id.id(), provider);
            // Queue timing is recorded at launch; completion fills remaining
            // timing fields without retaining request or provider content.
            result.timings.push(ProviderTiming {
                provider,
                queue_ms,
                start_ms: elapsed_ms(started_at, task_started),
                end_ms: 0,
                remaining_deadline_ms: deadline.remaining_ms_at(task_started),
            });
        }

        if stopped {
            cancel_and_drain(
                &mut running,
                &request_cancellation,
                policy.shutdown_grace,
                &mut result,
                started_at,
                deadline,
                &mut running_ids,
                &mut completed,
                &mut failed,
            )
            .await;
            break;
        }

        if running.is_empty() {
            // Remaining tasks are unresolved only when their prerequisite
            // graph is malformed.  They are omissions, never detached work.
            for task in pending.drain(..) {
                result.omissions.push(omission(
                    task.provider,
                    ReasonCode::ProviderFailed,
                    "prerequisite_cycle",
                ));
            }
            break;
        }

        let deadline_sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline.instant()));
        tokio::pin!(deadline_sleep);
        tokio::select! {
            joined = running.join_next_with_id() => {
                if let Some(joined) = joined {
                    consume_joined(
                        joined,
                        &mut result,
                        started_at,
                        deadline,
                        &mut running_ids,
                        &mut completed,
                        &mut failed,
                    );
                }
            }
            _ = &mut deadline_sleep => {
                result.deadline_exhausted = true;
                stopped = true;
            }
            _ = context.cancellation.cancelled() => {
                result.cancelled = true;
                stopped = true;
            }
        }
    }

    let reason = if result.cancelled {
        ReasonCode::ProviderCancelled
    } else if result.deadline_exhausted {
        ReasonCode::ProviderTimeout
    } else {
        ReasonCode::ProviderUnavailable
    };
    for task in pending {
        result.omissions.push(omission(task.provider, reason, "not_started"));
    }
    finalize_unfinished(&mut result, reason);
    result.canonicalize();
    result
}

/// Short alias for composition code that already calls the operation a
/// scheduler.
pub async fn schedule(
    context: ProviderContext,
    deadline: Deadline,
    tasks: Vec<ProviderTask>,
    policy: SchedulerPolicy,
) -> ScheduleResult {
    schedule_providers(context, deadline, tasks, policy).await
}

fn child_context(
    context: &ProviderContext,
    deadline: Deadline,
    cancellation: CancellationToken,
) -> ProviderContext {
    // The concrete cancellation type is intentionally inferred from the SDK
    // context, keeping this crate's public contract independent of its wire
    // representation.
    let mut child = context.clone();
    child.deadline = deadline.instant();
    child.cancellation = cancellation;
    child
}

fn consume_joined(
    joined: Result<(tokio::task::Id, JoinedProvider), tokio::task::JoinError>,
    result: &mut ScheduleResult,
    started_at: Instant,
    deadline: Deadline,
    running_ids: &mut HashMap<tokio::task::Id, ProviderId>,
    completed: &mut HashSet<ProviderId>,
    failed: &mut HashSet<ProviderId>,
) {
    match joined {
        Ok((task_id, (provider, _queued_at, _task_started, output))) => {
            running_ids.remove(&task_id);
            record_timing(result, provider, started_at, deadline);
            match output {
                Ok(output) => {
                    completed.insert(provider);
                    result.outputs.push(output);
                }
                Err(error) => {
                    failed.insert(provider);
                    result.omissions.push(omission(provider, reason_for(&error), "provider"));
                }
            }
        }
        Err(error) => {
            // JoinError retains its task ID even when the task panics or is
            // aborted.  Resolve that ID before emitting a lane omission.
            let provider = running_ids.remove(&error.id());
            if let Some(provider) = provider {
                record_timing(result, provider, started_at, deadline);
                let reason = if error.is_cancelled() {
                    if result.deadline_exhausted {
                        ReasonCode::ProviderTimeout
                    } else {
                        ReasonCode::ProviderCancelled
                    }
                } else {
                    ReasonCode::ProviderFailed
                };
                failed.insert(provider);
                result.omissions.push(omission(provider, reason, "provider_task"));
            }
        }
    }
}

async fn cancel_and_drain(
    running: &mut JoinSet<JoinedProvider>,
    cancellation: &CancellationToken,
    grace: Duration,
    result: &mut ScheduleResult,
    started_at: Instant,
    deadline: Deadline,
    running_ids: &mut HashMap<tokio::task::Id, ProviderId>,
    completed: &mut HashSet<ProviderId>,
    failed: &mut HashSet<ProviderId>,
) {
    cancellation.cancel();
    let drain = tokio::time::sleep(grace);
    tokio::pin!(drain);
    loop {
        tokio::select! {
            joined = running.join_next_with_id() => {
                let Some(joined) = joined else { break };
                consume_joined(joined, result, started_at, deadline, running_ids, completed, failed);
            }
            _ = &mut drain => {
                running.abort_all();
                while let Some(joined) = running.join_next_with_id().await {
                    consume_joined(joined, result, started_at, deadline, running_ids, completed, failed);
                }
                break;
            }
        }
    }
}

fn record_timing(
    result: &mut ScheduleResult,
    provider: ProviderId,
    started_at: Instant,
    deadline: Deadline,
) {
    let end = Instant::now();
    if let Some(timing) = result
        .timings
        .iter_mut()
        .find(|timing| timing.provider == provider && timing.end_ms == 0)
    {
        timing.end_ms = elapsed_ms(started_at, end);
        timing.remaining_deadline_ms = deadline.remaining_ms_at(end);
    }
}

fn reason_for(error: &ProviderError) -> ReasonCode {
    match error {
        ProviderError::Cancelled => ReasonCode::ProviderCancelled,
        ProviderError::DeadlineExceeded => ReasonCode::ProviderTimeout,
        ProviderError::Unavailable(_) | ProviderError::MissingSource(_) => ReasonCode::ProviderUnavailable,
        ProviderError::MalformedOutput(_) | ProviderError::Incomplete(_) | ProviderError::IdentityMismatch(_) => ReasonCode::ProviderMalformed,
        _ => ReasonCode::ProviderFailed,
    }
}

fn omission(provider: ProviderId, reason: ReasonCode, stage: &'static str) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider,
        reason,
        candidate_id: None,
        detail_id: Some(stage.to_owned()),
        stage: Some(stage.to_owned()),
    }
}

fn finalize_unfinished(result: &mut ScheduleResult, reason: ReasonCode) {
    let represented: HashSet<ProviderId> = result
        .outputs
        .iter()
        .map(|output| output.provider)
        .chain(result.omissions.iter().map(|omission| omission.provider))
        .collect();
    for timing in &result.timings {
        if !represented.contains(&timing.provider) {
            result
                .omissions
                .push(omission(timing.provider, reason, "cancelled_running"));
        }
    }
}

fn elapsed_ms(start: Instant, end: Instant) -> u64 {
    end.duration_since(start).as_millis().min(u128::from(u64::MAX)) as u64
}
