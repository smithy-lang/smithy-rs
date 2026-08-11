/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static-stability credentials caching for AWS clients.
//!
//! `StaticStabilityCache` is the default identity cache for AWS clients (installed by codegen and
//! `aws-config`). It is a retain-always, partition-keyed cache that provides *static stability*: on
//! a failed credential refresh it keeps serving the previously-resolved identity past expiration
//! (subject to backoff), so applications continue signing requests through a credential-source
//! outage. It caches any `Identity` — credentials and bearer tokens alike — and reads its
//! static-stability eligibility from a generic identity property.
//!
//! The `invalidation` submodule carries the auth-failure detection half of invalidation;
//! the cache's `ResolveCachedIdentity::invalidate` is the action half.

pub mod invalidation;

use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::StaticStabilityEligible;
use aws_smithy_async::future::timeout::Timeout;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::identity::{
    Identity, IdentityCachePartition, IdentityFuture, ResolveCachedIdentity, ResolveIdentity,
    SharedIdentityCache, SharedIdentityResolver,
};
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};
use tracing::Instrument;

// Blocking refresh point before expiration
const DEFAULT_MANDATORY_WINDOW: Duration = Duration::from_secs(60);
// Refresh point before expiry for caching-only (ineligible) identities
const CACHING_ONLY_BUFFER_TIME: Duration = Duration::from_secs(10);
// Uniform backoff floor after a failed refresh.
const BACKOFF_MIN_SECS: u64 = 300;
// Uniform backoff jitter span (300..=600s total).
const BACKOFF_JITTER_SECS: u64 = 300;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(3100);

// NOTE: `pessimistic_load_timeout` below is deliberately duplicated from `aws-smithy-runtime`'s
// `LazyCache` rather than imported, to avoid the one-way `pub` API exposure.
//
// Derive a pessimistic load timeout from the configured retry/timeout so the source's own retries
// can finish before the cache kills the future.
fn pessimistic_load_timeout(config_bag: &ConfigBag) -> Duration {
    let retry_config = config_bag
        .load::<RetryConfig>()
        .cloned()
        .unwrap_or_else(RetryConfig::standard);
    let timeout_config = config_bag
        .load::<TimeoutConfig>()
        .cloned()
        .unwrap_or_else(TimeoutConfig::disabled);

    let attempts = retry_config.max_attempts();
    let initial_backoff = retry_config.initial_backoff().as_secs_f64();
    let max_backoff = retry_config.max_backoff().as_secs_f64();

    // Worst-case total backoff: sum of min(initial * 2^i, max_backoff) for each retry.
    let total_backoff: f64 = (0..attempts.saturating_sub(1))
        .map(|i| (initial_backoff * 2.0_f64.powi(i as i32)).min(max_backoff))
        .sum();

    // Per-attempt ceiling: connect_timeout (floored at the default, doubled to approximate a full
    // attempt) or operation_attempt_timeout when larger.
    let connect = timeout_config
        .connect_timeout()
        .unwrap_or(DEFAULT_CONNECT_TIMEOUT)
        .max(DEFAULT_CONNECT_TIMEOUT)
        .as_secs_f64();
    let attempt_ceiling = timeout_config
        .operation_attempt_timeout()
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let per_attempt = (connect * 2.0).max(attempt_ceiling);
    let total_attempts = attempts as f64 * per_attempt;

    // Floor: at least one attempt's worth of budget even if max_attempts is 0.
    let computed = total_backoff + total_attempts;
    Duration::from_secs_f64(computed.max(per_attempt))
}

type NonRecoverablePredicate = Arc<dyn Fn(&BoxError) -> bool + Send + Sync>;

/// The default identity cache for AWS clients: retain-always, partition-keyed, static-stability.
///
/// See the [module docs](self). Build one with [`StaticStabilityCache::builder`].
pub struct StaticStabilityCache {
    partitions: RwLock<HashMap<IdentityCachePartition, Arc<Partition>>>,
    load_timeout: Option<Duration>,
    mandatory_window: Duration,
    non_recoverable: Option<NonRecoverablePredicate>,
    // Tests only, `None` in production (jittered 300..=600s backoff).
    backoff_override: Option<Duration>,
    // Tests only, `None` in production (advisory window from the lifetime table).
    advisory_window_override: Option<Duration>,
}

impl fmt::Debug for StaticStabilityCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StaticStabilityCache")
            .field("load_timeout", &self.load_timeout)
            .field("mandatory_window", &self.mandatory_window)
            .field(
                "non_recoverable",
                &self.non_recoverable.as_ref().map(|_| "<predicate>"),
            )
            .finish()
    }
}

impl StaticStabilityCache {
    /// Returns a builder for [`StaticStabilityCache`].
    pub fn builder() -> StaticStabilityCacheBuilder {
        StaticStabilityCacheBuilder::default()
    }

    fn new(
        load_timeout: Option<Duration>,
        mandatory_window: Duration,
        backoff_override: Option<Duration>,
        advisory_window_override: Option<Duration>,
    ) -> Self {
        Self {
            partitions: RwLock::new(HashMap::new()),
            load_timeout,
            mandatory_window,
            non_recoverable: Some(Arc::new(aws_non_recoverable)),
            backoff_override,
            advisory_window_override,
        }
    }

    // Get-or-create the per-source partition.
    // Read-mostly: the hit path takes only a shared read lock.
    fn partition(&self, key: IdentityCachePartition) -> Arc<Partition> {
        if let Some(p) = self.partitions.read().unwrap().get(&key).cloned() {
            return p;
        }
        // Unbounded: a client-level cache sees only its configured resolvers (a small, static set),
        // and per-operation `config_override` uses its own cache — so partitions don't grow with
        // request volume. `entry` also collapses the check-then-insert race with another writer.
        self.partitions
            .write()
            .unwrap()
            .entry(key)
            .or_insert_with(|| Arc::new(Partition::default()))
            .clone()
    }

    fn snapshot_partitions(&self) -> Vec<Arc<Partition>> {
        self.partitions.read().unwrap().values().cloned().collect()
    }

    // Refresh a partition from the source, then commit (success) or serve-cached/back-off/raise
    // (failure). The source `.await` is held under the async `refresh_gate` only (acquired by the
    // caller); the sync `state` lock is taken only for the brief snapshot/commit sections.
    async fn refresh(
        &self,
        part: &Partition,
        resolver: &SharedIdentityResolver,
        runtime_components: &RuntimeComponents,
        config_bag: &ConfigBag,
    ) -> Result<Identity, BoxError> {
        let prev = part.state.lock().unwrap().cached.clone();

        let sleep_impl = runtime_components.sleep_impl().expect("validated");
        let load_timeout = self
            .load_timeout
            .unwrap_or_else(|| pessimistic_load_timeout(config_bag));
        let timeout_future = sleep_impl.sleep(load_timeout);
        let resolved: Result<Identity, BoxError> = async move {
            match Timeout::new(
                resolver.resolve_identity(runtime_components, config_bag),
                timeout_future,
            )
            .await
            {
                Ok(result) => result,
                // Timeout: converts a *hung* source into a serve-cached decision (recoverable).
                Err(_elapsed) => {
                    Err(format!("credential resolution timed out after {:?}", load_timeout).into())
                }
            }
        }
        .instrument(tracing::debug_span!("load_identity"))
        .await;

        // The source call may have taken a while (up to the load timeout), so read `now` fresh from
        // the (validated) runtime time source rather than a pre-await value.
        let now = runtime_components.time_source().expect("validated").now();

        // An `Ok` response already expired by `now` is treated as a failed refresh (retain + back
        // off), not cached as fresh.
        let refreshed = resolved.and_then(|id| {
            if expired(id.expiration(), now) {
                Err("credential source returned already-expired credentials".into())
            } else {
                Ok(id)
            }
        });

        match refreshed {
            Ok(id) => {
                let mut st = part.state.lock().unwrap();
                st.cached = Some(id.clone());
                match id.expiration() {
                    Some(expiry) => {
                        st.expiration = Some(expiry);
                        if eligible(&id) {
                            // Static-stability overlay: advisory + mandatory windows.
                            let advisory_window = self
                                .advisory_window_override
                                .unwrap_or_else(|| advisory_window_for(lifetime(expiry, now)));
                            st.advisory_at = Some(sub(expiry, advisory_window));
                            st.mandatory_at = Some(sub(expiry, self.mandatory_window));
                        } else {
                            // Ineligible (custom/process): a single caching-only window at expiry.
                            let at = sub(expiry, CACHING_ONLY_BUFFER_TIME);
                            st.advisory_at = Some(at);
                            st.mandatory_at = Some(at);
                        }
                    }
                    // Non-expiring identity: no expiration-based refresh. Served indefinitely
                    // (classify returns Valid when `expiration` is None); only invalidation, which
                    // sets `expiration = now`, forces a refresh.
                    None => {
                        st.expiration = None;
                        st.advisory_at = None;
                        st.mandatory_at = None;
                    }
                }
                st.next_refresh_allowed_at = None; // clear backoff
                Ok(id)
            }
            Err(err) => {
                // A non-recoverable error raises immediately — before backoff and
                // before serve-cached. The cache holds only a predicate, naming no error types.
                if self.non_recoverable.as_ref().is_some_and(|p| p(&err)) {
                    return Err(err);
                }
                let mut st = part.state.lock().unwrap();
                // Backoff AND serve-stale are BOTH static stability — gated on eligibility.
                match prev {
                    Some(c) if eligible(&c) => {
                        st.next_refresh_allowed_at =
                            Some(now + self.backoff_override.unwrap_or_else(jittered_backoff));
                        tracing::warn!(
                            error = ?err,
                            "credential refresh failed; serving cached credentials (static stability)"
                        );
                        Ok(c)
                    }
                    // Ineligible: serve only if still valid, else raise.
                    Some(c) if !expired(st.expiration, now) => Ok(c),
                    _ => Err(err),
                }
            }
        }
    }
}

impl ResolveCachedIdentity for StaticStabilityCache {
    fn validate_base_client_config(
        &self,
        runtime_components: &RuntimeComponentsBuilder,
        _cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        validate(
            runtime_components.time_source().is_some(),
            runtime_components.sleep_impl().is_some(),
        )
    }

    fn validate_final_config(
        &self,
        runtime_components: &RuntimeComponents,
        _cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        validate(
            runtime_components.time_source().is_some(),
            runtime_components.sleep_impl().is_some(),
        )
    }

    fn resolve_cached_identity<'a>(
        &'a self,
        resolver: SharedIdentityResolver,
        runtime_components: &'a RuntimeComponents,
        config_bag: &'a ConfigBag,
    ) -> IdentityFuture<'a> {
        IdentityFuture::new(async move {
            let now = runtime_components.time_source().expect("validated").now();
            let part = self.partition(resolver.cache_partition());

            // 1) snapshot + classify under the SYNC lock — no `.await` held
            let decision = { classify(&part.state.lock().unwrap(), now) };

            match decision {
                // Valid or backed-off: serve cached, no source contact.
                Decision::Valid(id) | Decision::RateLimited(id) => Ok(id),
                // Advisory: refresh only if we win the gate, else serve cached now.
                Decision::Advisory(cached) => match part.refresh_gate.try_lock() {
                    Ok(_permit) => {
                        self.refresh(&part, &resolver, runtime_components, config_bag)
                            .await
                    }
                    Err(_) => Ok(cached),
                },
                // Mandatory (incl. expired) and initial fetch: block on the gate, then
                // reuse an in-flight result if another task refreshed while we waited.
                Decision::Mandatory | Decision::Initial => {
                    let _permit = part.refresh_gate.lock().await;
                    if let Some(id) = part.recheck(now) {
                        return Ok(id);
                    }
                    // No rate-limiting during initial-fetch.
                    self.refresh(&part, &resolver, runtime_components, config_bag)
                        .await
                }
            }
        })
    }

    fn invalidate(&self, rejected: &Identity) {
        for part in self.snapshot_partitions() {
            let mut st = part.state.lock().unwrap();
            if st.cached.as_ref().is_some_and(|c| c.ptr_eq(rejected)) {
                // Route the next resolution through the mandatory path. classify() keys off
                // advisory_at/mandatory_at, so collapse them to the epoch (a definitely-past time)
                // — no time source needed here. Deliberately leave next_refresh_allowed_at
                // (backoff) and cached (static stability) untouched.
                st.expiration = Some(SystemTime::UNIX_EPOCH);
                st.advisory_at = Some(SystemTime::UNIX_EPOCH);
                st.mandatory_at = Some(SystemTime::UNIX_EPOCH);
                break; // an identity is cached in exactly one partition
            }
        }
    }
}

// One cache slot per credential source (per `IdentityCachePartition`).
#[derive(Debug, Default)]
struct Partition {
    // Guards the synchronous snapshot/commit sections. NEVER held across `.await`.
    state: Mutex<CachedState>,
    // Async single-flight gate for the source call. Held across `.await`.
    refresh_gate: tokio::sync::Mutex<()>,
}

impl Partition {
    // Re-classify after acquiring the gate: another task may have refreshed while we waited.
    fn recheck(&self, now: SystemTime) -> Option<Identity> {
        let st = self.state.lock().unwrap();
        match classify(&st, now) {
            Decision::Valid(id) | Decision::RateLimited(id) => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct CachedState {
    // Retain-always; only ever *replaced* on success, never cleared.
    cached: Option<Identity>,
    expiration: Option<SystemTime>,
    // Non-blocking refresh point.
    advisory_at: Option<SystemTime>,
    // Blocking refresh point (<= expiration).
    mandatory_at: Option<SystemTime>,
    // Backoff gate after a failed refresh.
    next_refresh_allowed_at: Option<SystemTime>,
}

enum Decision {
    Valid(Identity),
    RateLimited(Identity),
    Advisory(Identity),
    Mandatory,
    Initial,
}

fn classify(st: &CachedState, now: SystemTime) -> Decision {
    let Some(id) = st.cached.clone() else {
        return Decision::Initial;
    };
    // Non-expiring identity (source reported no expiration): serve indefinitely. Only invalidation
    // (which sets `expiration = now`) forces a refresh.
    if st.expiration.is_none() {
        return Decision::Valid(id);
    }
    if matches!(st.advisory_at, Some(a) if now < a) {
        return Decision::Valid(id);
    }
    // Backoff gate sits AFTER Valid, BEFORE the advisory/mandatory split so an
    // expired-but-backed-off credential is served without contacting the source.
    if matches!(st.next_refresh_allowed_at, Some(t) if now < t) {
        return Decision::RateLimited(id);
    }
    match st.mandatory_at {
        Some(m) if now < m => Decision::Advisory(id),
        _ => Decision::Mandatory, // at/after mandatory_at, includes past-expiry
    }
}

fn eligible(id: &Identity) -> bool {
    id.property::<StaticStabilityEligible>().is_some()
}

fn expired(expiration: Option<SystemTime>, now: SystemTime) -> bool {
    matches!(expiration, Some(exp) if exp <= now)
}

fn sub(t: SystemTime, d: Duration) -> SystemTime {
    t.checked_sub(d).unwrap_or(t)
}

fn lifetime(expiry: SystemTime, now: SystemTime) -> Duration {
    expiry.duration_since(now).unwrap_or_default()
}

// Advisory window tiers, selected by remaining credential lifetime:
// `<= 20min -> 5min`, `> 20min && < 90min -> 15min`, `>= 90min -> 60min`.
fn advisory_window_for(lifetime: Duration) -> Duration {
    if lifetime <= Duration::from_secs(20 * 60) {
        Duration::from_secs(5 * 60)
    } else if lifetime < Duration::from_secs(90 * 60) {
        Duration::from_secs(15 * 60)
    } else {
        Duration::from_secs(60 * 60)
    }
}

fn jittered_backoff() -> Duration {
    Duration::from_secs(BACKOFF_MIN_SECS + fastrand::u64(0..=BACKOFF_JITTER_SECS))
}

// Default non-recoverable predicate injected into the cache by the AWS layer: a terminal
// `CredentialsError::Unrecoverable` anywhere in the source chain bypasses backoff and static
// stability. Providers such as `ChainProvider` wrap the base-provider error, so walk the chain
// rather than inspecting only the outermost error.
fn aws_non_recoverable(err: &BoxError) -> bool {
    let mut source: Option<&(dyn Error + 'static)> = Some(&**err);
    while let Some(e) = source {
        if e.downcast_ref::<CredentialsError>()
            .is_some_and(CredentialsError::is_unrecoverable)
        {
            return true;
        }
        source = e.source();
    }
    false
}

fn validate(has_time_source: bool, has_sleep_impl: bool) -> Result<(), BoxError> {
    if !has_time_source {
        return Err("StaticStabilityCache requires a time source to be configured".into());
    }
    if !has_sleep_impl {
        return Err(
            "StaticStabilityCache requires an async sleep implementation to be configured".into(),
        );
    }
    Ok(())
}

/// Builder for [`StaticStabilityCache`].
#[derive(Clone, Debug, Default)]
pub struct StaticStabilityCacheBuilder {
    load_timeout: Option<Duration>,
    mandatory_window: Option<Duration>,
}

impl StaticStabilityCacheBuilder {
    /// Sets the timeout bounding a single credential-source resolution. When unset, a default
    /// timeout is derived from the configured retry/timeout.
    pub fn load_timeout(mut self, load_timeout: Duration) -> Self {
        self.load_timeout = Some(load_timeout);
        self
    }

    /// Builds a [`SharedIdentityCache`] wrapping the configured [`StaticStabilityCache`].
    pub fn build(self) -> SharedIdentityCache {
        StaticStabilityCache::new(
            self.load_timeout,
            self.mandatory_window.unwrap_or(DEFAULT_MANDATORY_WINDOW),
            None,
            None,
        )
        .into_shared()
    }
}

#[cfg(test)]
impl StaticStabilityCache {
    // Advisory window (expiration - advisory_at) of the sole partition, for suite assertions.
    fn advisory_window(&self) -> Option<Duration> {
        let parts = self.partitions.read().unwrap();
        let part = parts.values().next()?;
        let st = part.state.lock().unwrap();
        match (st.expiration, st.advisory_at) {
            (Some(exp), Some(adv)) => exp.duration_since(adv).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_async::test_util::tick_advance_sleep::tick_advance_time_and_sleep;
    use aws_smithy_async::test_util::ManualTimeSource;
    use serde::Deserialize;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Data-driven execution of the modeled credential-refresh test suite (test-data/). Our
    // implementation diverges from the suite in two spots, handled inline: invalidation matches by
    // pointer identity rather than access key id, and the refresh backoff uses a fixed test value
    // (production jitters it). The configured advisory window is applied via a test-only builder
    // knob. A `rateLimited` expectation coincides with `sourceContacted == false` for a credential
    // past its refresh point, so it needs no separate assertion.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Scenario {
        documentation: String,
        given: Given,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Given {
        cached_credentials: String,
        access_key_id: Option<String>,
        configured_advisory_window_seconds: Option<u64>,
        refresh_backoff_seconds: Option<u64>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "camelCase")]
    enum Step {
        #[serde(rename_all = "camelCase")]
        GetCredentials {
            response: Option<String>,
            lifetime_seconds: Option<u64>,
            expected: Expected,
        },
        #[serde(rename_all = "camelCase")]
        Invalidate {
            rejected_access_key_id: String,
        },
        AdvanceTime {
            seconds: u64,
        },
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Expected {
        result: String,
        source_contacted: bool,
        advisory_window_seconds: Option<u64>,
    }

    const SUITE: &str = include_str!("../test-data/credential-refresh-tests.json");
    // Seed lifetime 60min -> advisory_at=2700, mandatory_at=3540, expiry=3600.
    const SEED_LIFE: u64 = 3600;

    async fn source_get(
        cache: &StaticStabilityCache,
        resolver: &SharedIdentityResolver,
        rc: &RuntimeComponents,
        cfg: &ConfigBag,
        contacts: &AtomicUsize,
    ) -> (Result<Identity, BoxError>, bool) {
        let before = contacts.load(Ordering::SeqCst);
        let r = cache
            .resolve_cached_identity(resolver.clone(), rc, cfg)
            .await;
        (r, contacts.load(Ordering::SeqCst) > before)
    }

    async fn run_scenario(s: &Scenario) {
        let doc = s.documentation.as_str();
        let seeded = s.given.cached_credentials != "none";
        // Place the seeded credential in the requested window (seed lifetime 3600).
        let start = match s.given.cached_credentials.as_str() {
            "valid" => 1000,
            "advisory" => 3000,
            "mandatory" => 3550,
            "expired" => 3700,
            "none" => 0,
            other => panic!("{doc}: unknown cachedCredentials {other:?}"),
        };

        // Pre-walk the steps to build the source queue in order and assign fresh ids, computing
        // each fresh response's expiry against the clock at that step.
        let mut queue: Vec<Result<Identity, BoxError>> = Vec::new();
        if seeded {
            queue.push(Ok(identity(1, SEED_LIFE, true)));
        }
        let mut next_id = 1u32;
        let mut fresh_ids = std::collections::VecDeque::new();
        let mut clk = start;
        for step in &s.steps {
            match step {
                Step::AdvanceTime { seconds } => clk += seconds,
                Step::GetCredentials {
                    response,
                    lifetime_seconds,
                    ..
                } => match response.as_deref() {
                    Some("freshCredentials") => {
                        next_id += 1;
                        fresh_ids.push_back(next_id);
                        let life = lifetime_seconds.unwrap_or(SEED_LIFE);
                        queue.push(Ok(identity(next_id, clk + life, true)));
                    }
                    Some("staleCredentials") => {
                        next_id += 1;
                        queue.push(Ok(identity(next_id, clk, true))); // expiry <= now
                    }
                    Some("error") => queue.push(Err("recoverable".into())),
                    Some("nonRecoverableError") => {
                        queue.push(Err(CredentialsError::unrecoverable("terminal").into()))
                    }
                    Some(other) => panic!("{doc}: unknown response {other:?}"),
                    None => {}
                },
                Step::Invalidate { .. } => {}
            }
        }

        // Build an isolated harness: a concrete cache (for internal accessors) with a
        // fixed backoff.
        let time = ManualTimeSource::new(epoch(0));
        let (_tick, sleep) = tick_advance_time_and_sleep();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(time.clone()))
            .with_sleep_impl(Some(sleep))
            .build()
            .unwrap();
        let contacts = Arc::new(AtomicUsize::new(0));
        let resolver = SharedIdentityResolver::new(MockSource {
            results: Mutex::new(queue),
            contacts: contacts.clone(),
        });
        let cache = StaticStabilityCache::new(
            None,
            DEFAULT_MANDATORY_WINDOW,
            s.given.refresh_backoff_seconds.map(Duration::from_secs),
            s.given
                .configured_advisory_window_seconds
                .map(Duration::from_secs),
        );
        let cfg = ConfigBag::base();

        // Establish the given-state: fetch once to cache the seed, then move into the window.
        let mut cached_id = 1u32;
        let mut served: Option<Identity> = None;
        if seeded {
            let (r, _) = source_get(&cache, &resolver, &components, &cfg, &contacts).await;
            served = Some(r.expect("seed fetch"));
            time.set_time(epoch(start));
        }

        // Execute the steps.
        let mut clk = start;
        for step in &s.steps {
            match step {
                Step::AdvanceTime { seconds } => {
                    clk += seconds;
                    time.set_time(epoch(clk));
                }
                Step::Invalidate {
                    rejected_access_key_id,
                } => {
                    // ptr_eq translation: a matching access key id invalidates the served instance;
                    // a stale id invalidates a different allocation (a no-op).
                    if s.given.access_key_id.as_deref() == Some(rejected_access_key_id.as_str()) {
                        cache.invalidate(served.as_ref().expect("a served identity"));
                    } else {
                        cache.invalidate(&identity(999, SEED_LIFE, true));
                    }
                }
                Step::GetCredentials { expected, .. } => {
                    let (r, contacted) =
                        source_get(&cache, &resolver, &components, &cfg, &contacts).await;
                    assert_eq!(
                        contacted, expected.source_contacted,
                        "{doc}: sourceContacted"
                    );
                    match expected.result.as_str() {
                        "cachedCredentials" => {
                            let got = r.expect("cachedCredentials");
                            assert_eq!(id_of(&got), cached_id, "{doc}: cached id");
                            served = Some(got);
                        }
                        "newCredentials" => {
                            let got = r.expect("newCredentials");
                            cached_id = fresh_ids.pop_front().expect("a queued fresh id");
                            assert_eq!(id_of(&got), cached_id, "{doc}: new id");
                            served = Some(got);
                        }
                        "noCredentialsError" => assert!(r.is_err(), "{doc}: noCredentialsError"),
                        "nonRecoverableError" => {
                            let Err(e) = r else {
                                panic!("{doc}: expected nonRecoverableError");
                            };
                            assert!(
                                e.downcast_ref::<CredentialsError>()
                                    .is_some_and(CredentialsError::is_unrecoverable),
                                "{doc}: nonRecoverableError"
                            );
                        }
                        other => panic!("{doc}: unknown result {other:?}"),
                    }
                    if let Some(w) = expected.advisory_window_seconds {
                        assert_eq!(
                            cache.advisory_window(),
                            Some(Duration::from_secs(w)),
                            "{doc}: advisoryWindowSeconds"
                        );
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_suite() {
        let scenarios: Vec<Scenario> = serde_json::from_str(SUITE).expect("valid suite json");
        assert_eq!(scenarios.len(), 23, "expected the full modeled suite");
        for s in &scenarios {
            run_scenario(s).await;
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestCreds {
        id: u32,
    }

    fn epoch(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    // Build an identity with a distinguishing id, an absolute expiration, and (optionally) the
    // static-stability eligibility marker.
    fn identity(id: u32, expiry_secs: u64, eligible: bool) -> Identity {
        let mut b = Identity::builder()
            .data(TestCreds { id })
            .expiration(epoch(expiry_secs));
        if eligible {
            b = b.property(StaticStabilityEligible);
        }
        b.build().unwrap()
    }

    fn id_of(identity: &Identity) -> u32 {
        identity.data::<TestCreds>().unwrap().id
    }

    // Mock credential source: returns queued results in order and counts how many times it is
    // actually contacted (so tests can assert `sourceContacted`).
    #[derive(Debug)]
    struct MockSource {
        results: Mutex<Vec<Result<Identity, BoxError>>>,
        contacts: Arc<AtomicUsize>,
    }

    impl ResolveIdentity for MockSource {
        fn resolve_identity<'a>(
            &'a self,
            _rc: &'a RuntimeComponents,
            _cfg: &'a ConfigBag,
        ) -> IdentityFuture<'a> {
            self.contacts.fetch_add(1, Ordering::SeqCst);
            let mut list = self.results.lock().unwrap();
            let next = if list.is_empty() {
                Err("mock source: no more results".into())
            } else {
                list.remove(0)
            };
            IdentityFuture::ready(next)
        }
    }

    struct Harness {
        cache: SharedIdentityCache,
        resolver: SharedIdentityResolver,
        components: RuntimeComponents,
        config_bag: ConfigBag,
        time: ManualTimeSource,
        contacts: Arc<AtomicUsize>,
    }

    impl Harness {
        fn new(results: Vec<Result<Identity, BoxError>>) -> Self {
            let time = ManualTimeSource::new(epoch(0));
            // A tick-advance sleep never fires unless explicitly ticked, so the (immediately
            // resolving) mock source always wins the refresh Timeout race — no spurious timeouts.
            let (_tick, sleep) = tick_advance_time_and_sleep();
            let components = RuntimeComponentsBuilder::for_tests()
                .with_time_source(Some(time.clone()))
                .with_sleep_impl(Some(sleep))
                .build()
                .unwrap();
            let contacts = Arc::new(AtomicUsize::new(0));
            let resolver = SharedIdentityResolver::new(MockSource {
                results: Mutex::new(results),
                contacts: contacts.clone(),
            });
            Self {
                cache: StaticStabilityCache::builder().build(),
                resolver,
                components,
                config_bag: ConfigBag::base(),
                time,
                contacts,
            }
        }

        // One `getCredentials` step: returns the result and whether the source was contacted.
        async fn get(&self) -> (Result<Identity, BoxError>, bool) {
            let before = self.contacts.load(Ordering::SeqCst);
            let result = self
                .cache
                .resolve_cached_identity(self.resolver.clone(), &self.components, &self.config_bag)
                .await;
            let contacted = self.contacts.load(Ordering::SeqCst) > before;
            (result, contacted)
        }

        fn advance_to(&self, secs: u64) {
            self.time.set_time(epoch(secs));
        }
    }

    // Window selection (advisoryWindowSeconds) by remaining lifetime:
    // <=20min -> 5min, >20 && <90 -> 15min, >=90 -> 60min.
    #[test]
    fn advisory_window_selection() {
        let m = |mins: u64| Duration::from_secs(mins * 60);
        assert_eq!(advisory_window_for(m(10)), m(5));
        assert_eq!(advisory_window_for(m(20)), m(5)); // boundary, inclusive
        assert_eq!(advisory_window_for(m(21)), m(15));
        assert_eq!(advisory_window_for(m(60)), m(15));
        assert_eq!(advisory_window_for(m(89)), m(15));
        assert_eq!(advisory_window_for(m(90)), m(60)); // boundary
        assert_eq!(advisory_window_for(m(6 * 60)), m(60));
    }

    // A non-recoverable error wrapped by an outer provider error (as `ChainProvider` produces) is
    // still detected by walking the source chain, not just the outermost error.
    #[test]
    fn non_recoverable_walks_source_chain() {
        let wrapped: BoxError =
            CredentialsError::provider_error(CredentialsError::unrecoverable("expired SSO token"))
                .into();
        assert!(
            aws_non_recoverable(&wrapped),
            "a nested Unrecoverable must be detected"
        );

        let recoverable: BoxError = CredentialsError::provider_error("STS 503").into();
        assert!(
            !aws_non_recoverable(&recoverable),
            "no Unrecoverable anywhere in the chain"
        );
    }

    // Ineligible (custom/process) identity: no serve-stale.
    // expected: a failed refresh after expiry raises rather than serving stale.
    #[tokio::test]
    async fn ineligible_error_after_expiry_is_raised() {
        let h = Harness::new(vec![Ok(identity(1, 3600, false)), Err("transient".into())]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // expired
        let (r, _contacted) = h.get().await;
        assert!(
            r.is_err(),
            "ineligible creds must not be served past expiry on failure"
        );
    }

    // A failed refresh applies the real (jittered) backoff: within it the source is not contacted
    // (rate-limited); once it elapses the refresh is retried and succeeds. This is the only test
    // exercising `jittered_backoff()` — the JSON suite injects a fixed backoff.
    #[tokio::test]
    async fn backoff_rate_limits_then_recovers() {
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Err("transient".into()),
            Ok(identity(2, 7200, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // expired -> failed refresh -> serve cached + backoff
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1, "failed refresh serves cached");
        assert!(contacted, "a refresh was attempted");

        h.advance_to(3800); // < 300s later: strictly inside the backoff -> rate-limited
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(!contacted, "within backoff: source not contacted");

        h.advance_to(3700 + 601); // past the max backoff (600s) -> refresh retried
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            2,
            "refresh retried once backoff elapsed"
        );
        assert!(contacted);
    }

    // A source that reports no expiration is treated as non-expiring: fetched once and served
    // indefinitely, never refreshed on a timer. Only invalidation forces a refresh.
    #[tokio::test]
    async fn no_expiry_served_indefinitely() {
        let no_expiry = Identity::builder()
            .data(TestCreds { id: 1 })
            .property(StaticStabilityEligible)
            .build()
            .unwrap();
        assert_eq!(no_expiry.expiration(), None);
        // Seed a single response: a timer-driven refresh would exhaust it and flip `contacted`.
        let h = Harness::new(vec![Ok(no_expiry)]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        // Advance far past any window a synthetic expiry could have produced (the old default was
        // 15m). A non-expiring identity is still served without contacting the source.
        h.advance_to(100 * 3600); // 100 hours
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(
            !contacted,
            "a no-expiration identity is non-expiring: served indefinitely, never refreshed on a timer"
        );
    }

    // A non-expiring identity still refreshes when invalidated (auth failure) —
    // invalidation is the only refresh trigger for creds without an expiration.
    #[tokio::test]
    async fn no_expiry_refreshes_on_invalidation() {
        let no_expiry = Identity::builder()
            .data(TestCreds { id: 1 })
            .property(StaticStabilityEligible)
            .build()
            .unwrap();
        let h = Harness::new(vec![Ok(no_expiry), Ok(identity(2, 3600, true))]);

        let served = h.get().await.0.unwrap();
        assert_eq!(id_of(&served), 1);

        // Without invalidation it would never refresh; invalidating the served identity forces it.
        h.cache.invalidate(&served);

        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            2,
            "invalidation forces a refresh even for a no-expiration identity"
        );
        assert!(contacted);
    }

    // A source whose resolution blocks on a gate until released — for concurrency tests.
    #[derive(Debug)]
    struct GatedSource {
        gate: Arc<tokio::sync::Notify>,
        contacts: Arc<AtomicUsize>,
        results: Mutex<Vec<Result<Identity, BoxError>>>,
    }

    impl ResolveIdentity for GatedSource {
        fn resolve_identity<'a>(
            &'a self,
            _rc: &'a RuntimeComponents,
            _cfg: &'a ConfigBag,
        ) -> IdentityFuture<'a> {
            let next = self.results.lock().unwrap().remove(0);
            let (gate, contacts) = (self.gate.clone(), self.contacts.clone());
            IdentityFuture::new(async move {
                contacts.fetch_add(1, Ordering::SeqCst);
                gate.notified().await; // block until released
                next
            })
        }
    }

    // Within the advisory window, only ONE caller refreshes; the others return cached
    // immediately without waiting or contacting the source (non-blocking single-flight).
    #[tokio::test]
    async fn advisory_concurrent_single_flight() {
        let time = ManualTimeSource::new(epoch(0));
        // Tick-advance sleep: never ticked here, so the refresh Timeout never fires and a
        // gate-blocked source stays blocked until the driver releases it (deterministic, no
        // real time).
        let (_tick, sleep) = tick_advance_time_and_sleep();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(time.clone()))
            .with_sleep_impl(Some(sleep))
            .build()
            .unwrap();
        let cfg = ConfigBag::base();
        let gate = Arc::new(tokio::sync::Notify::new());
        let contacts = Arc::new(AtomicUsize::new(0));
        let resolver = SharedIdentityResolver::new(GatedSource {
            gate: gate.clone(),
            contacts: contacts.clone(),
            results: Mutex::new(vec![
                Ok(identity(1, 3600, true)),
                Ok(identity(2, 7200, true)),
            ]),
        });
        let cache = StaticStabilityCache::builder().build();

        // Seed the initial fetch: permit exactly one source resolution.
        gate.notify_one();
        let seed = cache
            .resolve_cached_identity(resolver.clone(), &components, &cfg)
            .await
            .unwrap();
        assert_eq!(id_of(&seed), 1);
        assert_eq!(contacts.load(Ordering::SeqCst), 1);

        time.set_time(epoch(3000)); // advisory window (2700 <= now < 3540)

        // Two concurrent advisory callers + a driver that releases the single in-flight refresh.
        let get1 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let get2 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let driver = async {
            tokio::task::yield_now().await;
            gate.notify_one();
        };
        let (r1, r2, ()) = tokio::join!(get1, get2, driver);

        let mut ids = [id_of(&r1.unwrap()), id_of(&r2.unwrap())];
        ids.sort();
        assert_eq!(
            ids,
            [1, 2],
            "one caller refreshed (id=2), the other served cached (id=1)"
        );
        assert_eq!(
            contacts.load(Ordering::SeqCst),
            2,
            "single-flight: only one refresh contacted the source"
        );
    }

    // Concurrency test 2: within the mandatory window / expired, one caller refreshes and all
    // others WAIT for it and reuse the result (no additional source contacts).
    #[tokio::test]
    async fn mandatory_concurrent_single_flight_all_reuse() {
        let time = ManualTimeSource::new(epoch(0));
        let (_tick, sleep) = tick_advance_time_and_sleep();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(time.clone()))
            .with_sleep_impl(Some(sleep))
            .build()
            .unwrap();
        let cfg = ConfigBag::base();
        let gate = Arc::new(tokio::sync::Notify::new());
        let contacts = Arc::new(AtomicUsize::new(0));
        let resolver = SharedIdentityResolver::new(GatedSource {
            gate: gate.clone(),
            contacts: contacts.clone(),
            results: Mutex::new(vec![
                Ok(identity(1, 3600, true)),
                Ok(identity(2, 10_000, true)),
            ]),
        });
        let cache = StaticStabilityCache::builder().build();

        gate.notify_one();
        let seed = cache
            .resolve_cached_identity(resolver.clone(), &components, &cfg)
            .await
            .unwrap();
        assert_eq!(id_of(&seed), 1);
        assert_eq!(contacts.load(Ordering::SeqCst), 1);

        time.set_time(epoch(3700)); // expired -> mandatory path (blocking lock + recheck)

        // Three concurrent callers + a driver that releases the single in-flight refresh.
        let get1 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let get2 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let get3 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let driver = async {
            tokio::task::yield_now().await;
            gate.notify_one();
        };
        let (r1, r2, r3, ()) = tokio::join!(get1, get2, get3, driver);

        // Every caller receives the one refreshed identity; only one refresh contacted the source.
        assert_eq!(id_of(&r1.unwrap()), 2);
        assert_eq!(id_of(&r2.unwrap()), 2);
        assert_eq!(id_of(&r3.unwrap()), 2);
        assert_eq!(
            contacts.load(Ordering::SeqCst),
            2,
            "one refresh shared by all waiters"
        );
    }
}
